//! # Cross-Asset Lending Registry
//!
//! Manages multi-asset lending positions within the StellarLend protocol.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
//! │  AssetConfig  │     │  AssetState   │     │  Positions   │
//! │  (per asset)  │     │  supply/borrow│     │  (per user)  │
//! └──────┬───────┘     └──────┬───────┘     └──────┬───────┘
//!        │                    │                    │
//!        └────────────────────┴────────────────────┘
//!                             │
//!                    ┌────────┴────────┐
//!                    │  Health Factor   │
//!                    │  Computation     │
//!                    └─────────────────┘
//! ```
//!
//! ## Features
//! - Per-asset configuration: collateral factor (LTV), liquidation threshold,
//!   reserve factor, supply/borrow caps
//! - Oracle-based price feeds with staleness protection (1-hour window)
//! - Unified position summary with health factor across all assets
//! - Checked arithmetic throughout — all math uses `checked_*` to prevent overflow
//!
//! ## Health Factor
//! Computed as `weighted_collateral_value * 10_000 / weighted_debt_value`.
//! - `>= 10_000` (1.0×): healthy
//! - `< 10_000`: liquidatable
//! - No debt: `i128::MAX` (infinite health)
//!
//! ## Invariants
//! 1. Withdrawals and borrows are rejected if they would lower health factor below 1.0.
//! 2. Prices must not be stale (> 1 hour old) for position calculations.
//! 3. Assets cannot be re-initialized once registered.
//! 4. LTV (collateral_factor) must always be <= liquidation_threshold.
//! 5. All basis-point fields must be in [0, 10_000].
//!
//! ## Security Model
//! - **Admin**: Can initialize assets, update configs, update prices. Set once via
//!   `initialize()`, cannot be changed through this module.
//! - **Oracle trust**: Price updates are admin-gated. In production, integrate a
//!   decentralized oracle and validate signatures. The current design trusts the
//!   admin to relay correct prices.
//! - **Reentrancy**: No external token transfers occur in this module. State is
//!   always updated before any reads that depend on it. The deposit/withdraw/borrow/
//!   repay functions update storage atomically.

#![allow(dead_code)]
use soroban_sdk::{
    contracterror, contractevent, contracttype, symbol_short, Address, Env, Map, Symbol, Vec,
};
use crate::prelude::*;

// ============================================================================
// Types
// ============================================================================

/// Configuration for a single asset in the cross-asset lending registry.
///
/// All factor fields use basis points (1 bp = 0.01%, 10_000 bp = 100%).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetConfig {
    /// Asset contract address (`None` for native XLM).
    pub asset: Option<Address>,
    /// Collateral factor / Loan-to-Value in basis points (e.g., 7500 = 75%).
    /// Maximum fraction of collateral value that counts toward borrow capacity.
    pub collateral_factor: i128,
    /// Liquidation threshold in basis points (e.g., 8000 = 80%).
    /// When weighted debt exceeds this fraction of collateral, the position is
    /// liquidatable. Must be >= `collateral_factor`.
    pub liquidation_threshold: i128,
    /// Reserve factor in basis points (e.g., 1000 = 10%).
    /// Fraction of interest income directed to the protocol reserve.
    pub reserve_factor: i128,
    /// Maximum total supply cap across all users. 0 = unlimited.
    pub max_supply: i128,
    /// Maximum total borrow cap (debt ceiling) across all users. 0 = unlimited.
    pub max_borrow: i128,
    /// Whether the asset can be used as collateral.
    pub can_collateralize: bool,
    /// Whether the asset can be borrowed.
    pub can_borrow: bool,
    /// Borrow factor in basis points (e.g., 8000 = 80%).
    /// Weights the debt value in health calculations.
    pub borrow_factor: i128,
    /// Asset price in base units, normalized to 7 decimals.
    /// E.g., $1.00 = 10_000_000.
    pub price: i128,
    /// Ledger timestamp of the last price update.
    pub price_updated_at: u64,
}

/// A user's position for a single asset.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetPosition {
    /// Collateral balance in the asset's native units.
    pub collateral: i128,
    /// Outstanding debt principal in the asset's native units.
    pub debt_principal: i128,
    /// Accrued interest in the asset's native units.
    pub accrued_interest: i128,
    /// Ledger timestamp of the last position update.
    pub last_updated: u64,
}

/// Aggregated position summary across all assets for a single user.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPositionSummary {
    /// Total collateral value in USD (7 decimals).
    pub total_collateral_value: i128,
    /// Collateral value weighted by each asset's liquidation threshold.
    pub weighted_collateral_value: i128,
    /// Total debt value in USD (7 decimals).
    pub total_debt_value: i128,
    /// Debt value (currently unweighted — 1:1 with total_debt_value).
    pub weighted_debt_value: i128,
    /// Health factor scaled by 10_000. E.g., 15_000 = 1.5×.
    pub health_factor: i128,
    /// `true` when `health_factor < 10_000` and debt exists.
    pub is_liquidatable: bool,
    /// Remaining borrow capacity in USD (7 decimals).
    pub borrow_capacity: i128,
}

/// Discriminator for native XLM vs. token contract assets.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetKey {
    /// Native XLM (no contract address).
    Native,
    /// A Soroban token contract.
    Token(Address),
}

/// Combined key for per-user, per-asset position lookups.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserAssetKey {
    pub user: Address,
    pub asset: AssetKey,
}

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur during cross-asset lending operations.
///
/// Error codes are stable — never renumber existing variants.
#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CrossAssetError {
    /// The specified asset has no configuration registered.
    AssetNotConfigured = 1,
    /// The asset is configured but disabled for the requested operation.
    AssetDisabled = 2,
    /// Insufficient collateral for the requested withdrawal.
    InsufficientCollateral = 3,
    /// Borrow would exceed the user's remaining borrow capacity.
    ExceedsBorrowCapacity = 4,
    /// Operation would result in a health factor below 1.0.
    UnhealthyPosition = 5,
    /// Deposit would exceed the asset's supply cap.
    SupplyCapExceeded = 6,
    /// Borrow would exceed the asset's global borrow cap.
    BorrowCapExceeded = 7,
    /// Price is zero or negative.
    InvalidPrice = 8,
    /// Asset price is older than the staleness threshold (1 hour).
    PriceStale = 9,
    /// Caller is not authorized for this operation.
    NotAuthorized = 10,
    /// Asset has already been initialized (re-initialization rejected).
    AlreadyInitialized = 11,
    /// A configuration parameter is invalid (e.g., LTV > liquidation threshold).
    InvalidConfig = 12,
    /// Arithmetic overflow or underflow during computation.
    Overflow = 13,
    /// Amount must be greater than zero.
    InvalidAmount = 14,
}

// ============================================================================
// Events
// ============================================================================

/// Emitted when a new asset is registered.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AssetInitializedEvent {
    pub asset: Option<Address>,
    pub collateral_factor: i128,
    pub liquidation_threshold: i128,
    pub reserve_factor: i128,
    pub price: i128,
    pub timestamp: u64,
}

/// Emitted when an asset's configuration is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AssetConfigUpdatedEvent {
    pub asset: Option<Address>,
    pub collateral_factor: i128,
    pub liquidation_threshold: i128,
    pub borrow_factor: i128,
    pub timestamp: u64,
}

/// Emitted when an asset's price is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AssetPriceUpdatedEvent {
    pub asset: Option<Address>,
    pub old_price: i128,
    pub new_price: i128,
    pub timestamp: u64,
}

/// Emitted on cross-asset deposit.
#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossAssetDepositEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub new_collateral: i128,
    pub timestamp: u64,
}

/// Emitted on cross-asset withdrawal.
#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossAssetWithdrawEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub remaining_collateral: i128,
    pub timestamp: u64,
}

/// Emitted on cross-asset borrow.
#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossAssetBorrowEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub new_debt: i128,
    pub timestamp: u64,
}

/// Emitted on cross-asset repayment.
#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossAssetRepayEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub remaining_debt: i128,
    pub timestamp: u64,
}

// ============================================================================
// Storage Keys
// ============================================================================



/// Storage key for the map of asset configurations: `Map<AssetKey, AssetConfig>`.
const ASSET_CONFIGS: Symbol = symbol_short!("configs");

/// Storage key for the map of user positions: `Map<UserAssetKey, AssetPosition>`.
const USER_POSITIONS: Symbol = symbol_short!("positions");

/// Storage key for the map of total supplies per asset: `Map<AssetKey, i128>`.
const TOTAL_SUPPLIES: Symbol = symbol_short!("supplies");

/// Storage key for the map of total borrows per asset: `Map<AssetKey, i128>`.
const TOTAL_BORROWS: Symbol = symbol_short!("borrows");

/// Storage key for the global list of registered assets: `Vec<AssetKey>`.
const ASSET_LIST: Symbol = symbol_short!("assets");

/// Maximum number of registered assets iterated in a single position summary call.
///
/// Bounds the computational work (CPU instructions + memory) in
/// `get_user_position_summary` so that a contract with many registered assets
/// cannot exhaust the Soroban resource budget in a single invocation.
///
/// # Security
/// Without this cap, an admin could register enough assets to make the summary
/// function permanently unusable by any user. 64 assets is conservative; the
/// realistic upper bound for a lending protocol is far lower.
pub const MAX_ASSETS_PER_SUMMARY: u32 = 64;

/// Price staleness threshold in seconds (1 hour).
const PRICE_STALENESS_THRESHOLD: u64 = 3600;

/// Price precision: 7 decimals (10^7).
const PRICE_PRECISION: i128 = 10_000_000;

/// Basis-point denominator (10_000 = 100%).
const BPS_DENOMINATOR: i128 = 10_000;

/// Health factor precision (10_000 = 1.0×).
const HEALTH_FACTOR_PRECISION: i128 = 10_000;

// ============================================================================
// Admin Initialization
// ============================================================================

pub fn initialize(env: &Env, admin: Address) -> Result<(), CrossAssetError> {
    if crate::admin::has_admin(env) {
        let existing_admin = crate::admin::get_admin(env).unwrap();
        if existing_admin == admin {
            return Ok(()); // Already initialized with the same admin
        }
        return Err(CrossAssetError::AlreadyInitialized);
    }

    admin.require_auth();
    crate::admin::set_admin(env, admin).unwrap();

    Ok(())
}

/// Verify caller is the registered admin. Panics via `require_auth()` if not.
///
/// # Errors
/// * `NotAuthorized` — No admin set or caller is not admin.
fn require_admin(env: &Env) -> Result<(), CrossAssetError> {
    let admin: Address = crate::admin::get_admin(env)
        .ok_or(CrossAssetError::NotAuthorized)?;

    admin.require_auth();
    Ok(())
}

// ============================================================================
// Asset Initialization
// ============================================================================

/// Register a new asset with the cross-asset lending module.
///
/// Creates the asset configuration and appends the asset key to the global
/// asset list. The asset must not already be registered.
///
/// # Arguments
/// * `env` — The contract environment
/// * `asset` — Asset to configure (`None` for native XLM)
/// * `config` — Full asset configuration (factors, caps, price)
///
/// # Errors
/// * `NotAuthorized` — Caller is not the admin
/// * `AlreadyInitialized` — Asset is already registered
/// * `InvalidConfig` — A basis-point field is out of [0, 10_000], LTV > liquidation
///   threshold, or caps are negative
/// * `InvalidPrice` — Price is zero or negative
///
/// # Security
/// * Only admin can call.
/// * Rejects re-initialization to prevent config corruption.
pub fn initialize_asset(
    env: &Env,
    asset: Option<Address>,
    config: AssetConfig,
) -> Result<(), CrossAssetError> {
    require_admin(env)?;

    let asset_key = AssetKey::from_option(asset.clone());

    // Prevent re-initialization
    let configs: Map<AssetKey, AssetConfig> = env
        .storage()
        .persistent()
        .get(&ASSET_CONFIGS)
        .unwrap_or(Map::new(env));

    if configs.contains_key(asset_key.clone()) {
        return Err(CrossAssetError::AlreadyInitialized);
    }

    require_valid_config(&config)?;

    // Store config
    let mut configs = configs;
    configs.set(asset_key.clone(), config.clone());
    env.storage().persistent().set(&ASSET_CONFIGS, &configs);

    // Add to asset list
    let mut asset_list: Vec<AssetKey> = env
        .storage()
        .persistent()
        .get(&ASSET_LIST)
        .unwrap_or(Vec::new(env));

    asset_list.push_back(asset_key);
    env.storage().persistent().set(&ASSET_LIST, &asset_list);

    // Emit event
    AssetInitializedEvent {
        asset,
        collateral_factor: config.collateral_factor,
        liquidation_threshold: config.liquidation_threshold,
        reserve_factor: config.reserve_factor,
        price: config.price,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(())
}

// ============================================================================
// Asset Configuration Updates
// ============================================================================

/// Selectively update an existing asset's configuration.
///
/// Only the provided `Some` fields are updated; `None` fields keep their
/// current values. After applying updates, the resulting config is validated
/// to ensure LTV <= liquidation threshold and all basis-point fields are in
/// bounds.
///
/// # Arguments
/// * `env` — The contract environment
/// * `asset` — Asset to update (`None` for XLM)
/// * `collateral_factor` — Optional new LTV (basis points)
/// * `liquidation_threshold` — Optional new liquidation threshold (basis points)
/// * `max_supply` — Optional new supply cap
/// * `max_borrow` — Optional new borrow cap / debt ceiling
/// * `can_collateralize` — Optional flag to enable/disable as collateral
/// * `can_borrow` — Optional flag to enable/disable borrowing
///
/// # Errors
/// * `NotAuthorized` — Caller is not the admin
/// * `AssetNotConfigured` — Asset has not been initialized
/// * `InvalidConfig` — Resulting config would be invalid (LTV > threshold, out of bounds)
///
/// # Security
/// * Only admin can call.
/// * Validates the *resulting* config, not just the delta. This prevents
///   unsafe config transitions (e.g., raising LTV above liquidation threshold).
#[allow(clippy::too_many_arguments)]
pub fn update_asset_config(
    env: &Env,
    asset: Option<Address>,
    collateral_factor: Option<i128>,
    liquidation_threshold: Option<i128>,
    max_supply: Option<i128>,
    max_borrow: Option<i128>,
    can_collateralize: Option<bool>,
    can_borrow: Option<bool>,
    borrow_factor: Option<i128>,
) -> Result<(), CrossAssetError> {
    require_admin(env)?;

    let asset_key = AssetKey::from_option(asset.clone());
    let mut config = get_asset_config(env, &asset_key)?;

    if let Some(cf) = collateral_factor {
        config.collateral_factor = cf;
    }
    if let Some(lt) = liquidation_threshold {
        config.liquidation_threshold = lt;
    }
    if let Some(ms) = max_supply {
        config.max_supply = ms;
    }
    if let Some(mb) = max_borrow {
        config.max_borrow = mb;
    }
    if let Some(cc) = can_collateralize {
        config.can_collateralize = cc;
    }
    if let Some(cb) = can_borrow {
        config.can_borrow = cb;
    }
    if let Some(bf) = borrow_factor {
        config.borrow_factor = bf;
    }

    // Validate the *resulting* config holistically
    require_valid_basis_points(config.collateral_factor)?;
    require_valid_basis_points(config.liquidation_threshold)?;
    require_valid_basis_points(config.reserve_factor)?;
    require_valid_basis_points(config.borrow_factor)?;

    if config.liquidation_threshold < config.collateral_factor {
        return Err(CrossAssetError::InvalidConfig);
    }

    // Persist
    let mut configs: Map<AssetKey, AssetConfig> = env
        .storage()
        .persistent()
        .get(&ASSET_CONFIGS)
        .unwrap_or(Map::new(env));

    configs.set(asset_key, config.clone());
    env.storage().persistent().set(&ASSET_CONFIGS, &configs);

    // Emit event
    AssetConfigUpdatedEvent {
        asset,
        collateral_factor: config.collateral_factor,
        liquidation_threshold: config.liquidation_threshold,
        borrow_factor: config.borrow_factor,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(())
}

// ============================================================================
// Price Updates
// ============================================================================

/// Update the oracle price for an asset.
///
/// Records the new price and the current ledger timestamp for staleness checks.
///
/// # Arguments
/// * `env` — The contract environment
/// * `asset` — Asset to update price for (`None` for XLM)
/// * `price` — New price in base units (7 decimals, must be > 0)
///
/// # Errors
/// * `NotAuthorized` — Caller is not the admin
/// * `InvalidPrice` — Price is zero or negative
/// * `AssetNotConfigured` — Asset has not been initialized
///
/// # Security
/// * Only admin / trusted oracle relay can call.
/// * **Trust assumption**: The caller is trusted to provide accurate prices.
///   In production, integrate a decentralized oracle with signature verification
///   (e.g., Pyth, Switchboard) and validate the price source on-chain.
pub fn update_asset_price(
    env: &Env,
    asset: Option<Address>,
    price: i128,
) -> Result<(), CrossAssetError> {
    require_admin(env)?;

    if price <= 0 {
        return Err(CrossAssetError::InvalidPrice);
    }

    let asset_key = AssetKey::from_option(asset.clone());
    let mut config = get_asset_config(env, &asset_key)?;

    let old_price = config.price;
    config.price = price;
    config.price_updated_at = env.ledger().timestamp();

    let mut configs: Map<AssetKey, AssetConfig> = env
        .storage()
        .persistent()
        .get(&ASSET_CONFIGS)
        .unwrap_or(Map::new(env));

    configs.set(asset_key, config);
    env.storage().persistent().set(&ASSET_CONFIGS, &configs);

    // Emit event
    AssetPriceUpdatedEvent {
        asset,
        old_price,
        new_price: price,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(())
}

// ============================================================================
// Deposit / Withdraw / Borrow / Repay
// ============================================================================

/// Deposit collateral for a specific asset.
///
/// Requires user authorization. Validates the asset is enabled for collateral
/// and that the deposit does not exceed the supply cap. Uses checked arithmetic
/// for all balance updates.
///
/// # Arguments
/// * `env` — The contract environment
/// * `user` — User depositing collateral (must authorize)
/// * `asset` — Asset to deposit (`None` for XLM)
/// * `amount` — Amount to deposit (must be > 0)
///
/// # Returns
/// Updated [`AssetPosition`] after the deposit.
///
/// # Errors
/// * `InvalidAmount` — Amount is zero or negative
/// * `AssetNotConfigured` — Asset is not registered
/// * `AssetDisabled` — Asset is not enabled for collateral
/// * `SupplyCapExceeded` — Deposit would exceed the asset's supply cap
/// * `Overflow` — Arithmetic overflow
///
/// # Security
/// * Only the depositing user can call (via `require_auth()`).
/// * State is updated atomically — no external calls between reads and writes.
pub fn cross_asset_deposit(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    user.require_auth();
    require_positive_amount(amount)?;

    let asset_key = AssetKey::from_option(asset.clone());
    let config = get_asset_config(env, &asset_key)?;

    if !config.can_collateralize {
        return Err(CrossAssetError::AssetDisabled);
    }

    // Check supply cap
    if config.max_supply > 0 {
        let total_supply = get_total_supply(env, &asset_key);
        let new_supply = checked_add(total_supply, amount)?;
        if new_supply > config.max_supply {
            return Err(CrossAssetError::SupplyCapExceeded);
        }
    }

    // Update position
    let mut position = get_user_asset_position(env, &user, asset.clone());
    position.collateral = checked_add(position.collateral, amount)?;
    position.last_updated = env.ledger().timestamp();

    set_user_asset_position(env, &user, asset.clone(), position.clone());
    update_total_supply(env, &asset_key, amount)?;

    // Emit event
    CrossAssetDepositEvent {
        user,
        asset,
        amount,
        new_collateral: position.collateral,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(position)
}

/// Withdraw collateral for a specific asset.
///
/// Requires user authorization. Checks that the user has sufficient collateral
/// and that the withdrawal does not bring the health factor below 1.0. If the
/// health check fails the withdrawal is rolled back.
///
/// # Arguments
/// * `env` — The contract environment
/// * `user` — User withdrawing collateral (must authorize)
/// * `asset` — Asset to withdraw (`None` for XLM)
/// * `amount` — Amount to withdraw (must be > 0)
///
/// # Returns
/// Updated [`AssetPosition`] after the withdrawal.
///
/// # Errors
/// * `InvalidAmount` — Amount is zero or negative
/// * `InsufficientCollateral` — User's collateral balance is below `amount`
/// * `UnhealthyPosition` — Withdrawal would drop health factor below 1.0
/// * `PriceStale` — Stale price prevents health factor calculation
/// * `Overflow` — Arithmetic overflow/underflow
///
/// # Security
/// * Only the position owner can call.
/// * Uses optimistic update + rollback pattern: position is updated first,
///   health factor is checked, and if unhealthy the change is reverted.
pub fn cross_asset_withdraw(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    user.require_auth();
    require_positive_amount(amount)?;

    let asset_key = AssetKey::from_option(asset.clone());

    let mut position = get_user_asset_position(env, &user, asset.clone());

    if position.collateral < amount {
        return Err(CrossAssetError::InsufficientCollateral);
    }

    // Optimistic update
    position.collateral = checked_sub(position.collateral, amount)?;
    position.last_updated = env.ledger().timestamp();
    set_user_asset_position(env, &user, asset.clone(), position.clone());

    // Health check — only required when user has outstanding debt.
    // If the summary fails due to stale prices but there's no debt, the
    // withdrawal is safe (health factor is infinite regardless of price).
    match get_user_position_summary(env, &user) {
        Ok(summary) => {
            if summary.total_debt_value > 0 && summary.health_factor < HEALTH_FACTOR_PRECISION {
                // Rollback
                position.collateral = checked_add(position.collateral, amount)?;
                set_user_asset_position(env, &user, asset, position);
                return Err(CrossAssetError::UnhealthyPosition);
            }
        }
        Err(CrossAssetError::PriceStale) => {
            // If user has any debt at all, stale prices are unacceptable
            // because we can't verify the health factor. Check cheaply.
            if user_has_any_debt(env, &user) {
                position.collateral = checked_add(position.collateral, amount)?;
                set_user_asset_position(env, &user, asset, position);
                return Err(CrossAssetError::PriceStale);
            }
            // No debt → health factor is infinite, withdrawal is safe
        }
        Err(e) => {
            // Any other error → rollback
            position.collateral = checked_add(position.collateral, amount)?;
            set_user_asset_position(env, &user, asset, position);
            return Err(e);
        }
    }

    update_total_supply(env, &asset_key, -amount)?;

    // Emit event
    CrossAssetWithdrawEvent {
        user,
        asset,
        amount,
        remaining_collateral: position.collateral,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(position)
}

/// Borrow a specific asset against cross-asset collateral.
///
/// Requires user authorization. Validates the asset is enabled for borrowing,
/// checks the global borrow cap, and verifies the post-borrow health factor
/// stays above 1.0. If the health check fails the borrow is rolled back.
///
/// # Arguments
/// * `env` — The contract environment
/// * `user` — User borrowing (must authorize)
/// * `asset` — Asset to borrow (`None` for XLM)
/// * `amount` — Amount to borrow (must be > 0)
///
/// # Returns
/// Updated [`AssetPosition`] after the borrow.
///
/// # Errors
/// * `InvalidAmount` — Amount is zero or negative
/// * `AssetNotConfigured` — Asset is not registered
/// * `AssetDisabled` — Asset is not enabled for borrowing
/// * `BorrowCapExceeded` — Borrow would exceed the asset's global borrow cap
/// * `ExceedsBorrowCapacity` — Health factor would drop below 1.0
/// * `PriceStale` — Stale price prevents health factor calculation
/// * `Overflow` — Arithmetic overflow
///
/// # Security
/// * Only the borrowing user can call.
/// * Uses optimistic update + rollback pattern for health factor validation.
pub fn cross_asset_borrow(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    user.require_auth();
    require_positive_amount(amount)?;

    let asset_key = AssetKey::from_option(asset.clone());
    let config = get_asset_config(env, &asset_key)?;

    if !config.can_borrow {
        return Err(CrossAssetError::AssetDisabled);
    }

    // Check borrow cap
    if config.max_borrow > 0 {
        let total_borrow = get_total_borrow(env, &asset_key);
        let new_borrow = checked_add(total_borrow, amount)?;
        if new_borrow > config.max_borrow {
            return Err(CrossAssetError::BorrowCapExceeded);
        }
    }

    // Optimistic update
    let mut position = get_user_asset_position(env, &user, asset.clone());
    position.debt_principal = checked_add(position.debt_principal, amount)?;
    position.last_updated = env.ledger().timestamp();
    set_user_asset_position(env, &user, asset.clone(), position.clone());

    // Health check
    let summary = get_user_position_summary(env, &user)?;
    if summary.health_factor < HEALTH_FACTOR_PRECISION {
        // Rollback
        position.debt_principal = checked_sub(position.debt_principal, amount)?;
        set_user_asset_position(env, &user, asset, position);
        return Err(CrossAssetError::ExceedsBorrowCapacity);
    }

    update_total_borrow(env, &asset_key, amount)?;

    // Emit event
    CrossAssetBorrowEvent {
        user,
        asset,
        amount,
        new_debt: position.debt_principal,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(position)
}

/// Repay debt for a specific asset.
///
/// Requires user authorization. Repayment is capped at the total outstanding
/// debt (principal + accrued interest). Interest is paid first, then principal.
/// Uses checked arithmetic throughout.
///
/// # Arguments
/// * `env` — The contract environment
/// * `user` — User repaying debt (must authorize)
/// * `asset` — Asset to repay (`None` for XLM)
/// * `amount` — Amount to repay (capped at total debt, must be > 0)
///
/// # Returns
/// Updated [`AssetPosition`] after the repayment.
///
/// # Errors
/// * `InvalidAmount` — Amount is zero or negative
/// * `Overflow` — Arithmetic overflow/underflow
///
/// # Security
/// * Only the debt owner can call.
/// * Repayment is capped at total debt — overpayment is silently reduced.
pub fn cross_asset_repay(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    user.require_auth();
    require_positive_amount(amount)?;

    let asset_key = AssetKey::from_option(asset.clone());

    let mut position = get_user_asset_position(env, &user, asset.clone());

    let total_debt = checked_add(position.debt_principal, position.accrued_interest)?;
    let repay_amount = amount.min(total_debt);

    // Pay interest first, then principal
    if repay_amount <= position.accrued_interest {
        position.accrued_interest = checked_sub(position.accrued_interest, repay_amount)?;
    } else {
        let remaining = checked_sub(repay_amount, position.accrued_interest)?;
        position.accrued_interest = 0;
        position.debt_principal = checked_sub(position.debt_principal, remaining)?;
    }

    position.last_updated = env.ledger().timestamp();

    set_user_asset_position(env, &user, asset.clone(), position.clone());
    update_total_borrow(env, &asset_key, -repay_amount)?;

    let remaining_debt = checked_add(position.debt_principal, position.accrued_interest)?;

    // Emit event
    CrossAssetRepayEvent {
        user,
        asset,
        amount: repay_amount,
        remaining_debt,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(position)
}

// ============================================================================
// Read-Only Queries
// ============================================================================

/// Get user's position for a specific asset.
///
/// Returns a default empty position if the user has no position for this asset.
/// This function performs no mutation.
///
/// # Arguments
/// * `env` — The contract environment
/// * `user` — User address
/// * `asset` — Asset address (`None` for XLM)
///
/// # Returns
/// The user's [`AssetPosition`] for the requested asset.
pub fn get_user_asset_position(env: &Env, user: &Address, asset: Option<Address>) -> AssetPosition {
    let key = UserAssetKey::new(user.clone(), asset);
    let positions: Map<UserAssetKey, AssetPosition> = env
        .storage()
        .persistent()
        .get(&USER_POSITIONS)
        .unwrap_or(Map::new(env));

    positions.get(key).unwrap_or(AssetPosition {
        collateral: 0,
        debt_principal: 0,
        accrued_interest: 0,
        last_updated: env.ledger().timestamp(),
    })
}

/// Calculate a unified position summary across all registered assets.
///
/// Iterates over up to [`MAX_ASSETS_PER_SUMMARY`] configured assets, aggregates
/// collateral and debt values weighted by their respective factors, and computes
/// the health factor. Prices older than 1 hour are rejected for any asset with
/// a non-zero position.
///
/// # Arguments
/// * `env` — The contract environment
/// * `user` — User address
///
/// # Returns
/// [`UserPositionSummary`] with health factor, liquidation status, and borrow capacity.
///
/// # Errors
/// * `PriceStale` — Any asset with a non-zero position has a price older than 1 hour
/// * `Overflow` — Arithmetic overflow during aggregation
///
/// # Security
/// * Read-only — does not mutate state.
/// * Rejects stale prices to prevent stale-price manipulation attacks.
/// * Iteration is capped at [`MAX_ASSETS_PER_SUMMARY`] (64) to bound CPU and
///   memory usage per transaction. If more assets are registered only the first
///   64 are considered; the summary may be partial in that case. This cap
///   prevents a DoS vector where a large asset registry exhausts the Soroban
///   per-transaction resource budget for every user who calls this function.
pub fn get_user_position_summary(
    env: &Env,
    user: &Address,
) -> Result<UserPositionSummary, CrossAssetError> {
    let asset_list: Vec<AssetKey> = env
        .storage()
        .persistent()
        .get(&ASSET_LIST)
        .unwrap_or(Vec::new(env));

    let configs: Map<AssetKey, AssetConfig> = env
        .storage()
        .persistent()
        .get(&ASSET_CONFIGS)
        .unwrap_or(Map::new(env));

    let mut total_collateral_value: i128 = 0;
    let mut weighted_collateral_value: i128 = 0;
    let mut total_debt_value: i128 = 0;
    let mut weighted_debt_value: i128 = 0;

    // #530: Bound the iteration so that large asset registries cannot exhaust
    // Soroban's per-transaction CPU/memory budget.
    //
    // # Security
    // If more assets than MAX_ASSETS_PER_SUMMARY are registered, only the first
    // MAX_ASSETS_PER_SUMMARY are considered. Callers should be aware that the
    // summary may be partial when the registry is at capacity. This is a
    // deliberate trade-off between completeness and DoS-resistance.
    let asset_count = asset_list.len().min(MAX_ASSETS_PER_SUMMARY);

    for i in 0..asset_count {
        let asset_key = asset_list.get(i).unwrap();

        if let Some(config) = configs.get(asset_key.clone()) {
            let asset_option = asset_key.to_option();
            let position = get_user_asset_position(env, user, asset_option);

            if position.collateral == 0 && position.debt_principal == 0 {
                continue;
            }

            // Staleness check
            let current_time = env.ledger().timestamp();
            if current_time > config.price_updated_at
                && current_time - config.price_updated_at > PRICE_STALENESS_THRESHOLD
            {
                return Err(CrossAssetError::PriceStale);
            }

            // Collateral value: collateral * price / 10^7
            let collateral_value = checked_mul(position.collateral, config.price)?
                .checked_div(PRICE_PRECISION)
                .ok_or(CrossAssetError::Overflow)?;
            total_collateral_value = checked_add(total_collateral_value, collateral_value)?;

            if config.can_collateralize {
                let weighted = checked_mul(collateral_value, config.liquidation_threshold)?
                    .checked_div(BPS_DENOMINATOR)
                    .ok_or(CrossAssetError::Overflow)?;
                weighted_collateral_value = checked_add(weighted_collateral_value, weighted)?;
            }

            // Debt value: (principal + interest) * price / 10^7
            let total_debt = checked_add(position.debt_principal, position.accrued_interest)?;
            let debt_value = checked_mul(total_debt, config.price)?
                .checked_div(PRICE_PRECISION)
                .ok_or(CrossAssetError::Overflow)?;
            total_debt_value = checked_add(total_debt_value, debt_value)?;
            let weighted_debt = checked_mul(debt_value, config.borrow_factor)?
                .checked_div(BPS_DENOMINATOR)
                .ok_or(CrossAssetError::Overflow)?;
            weighted_debt_value = checked_add(weighted_debt_value, weighted_debt)?;
        }
    }

    // Health factor = weighted_collateral / weighted_debt * 10_000
    let health_factor = if weighted_debt_value > 0 {
        checked_mul(weighted_collateral_value, HEALTH_FACTOR_PRECISION)?
            .checked_div(weighted_debt_value)
            .ok_or(CrossAssetError::Overflow)?
    } else {
        i128::MAX
    };

    let is_liquidatable = health_factor < HEALTH_FACTOR_PRECISION && weighted_debt_value > 0;

    let borrow_capacity = if weighted_collateral_value > weighted_debt_value {
        checked_sub(weighted_collateral_value, weighted_debt_value)?
    } else {
        0
    };

    Ok(UserPositionSummary {
        total_collateral_value,
        weighted_collateral_value,
        total_debt_value,
        weighted_debt_value,
        health_factor,
        is_liquidatable,
        borrow_capacity,
    })
}

/// Return the list of all registered asset keys.
///
/// Returns an empty vector if no assets have been configured.
/// Read-only — no mutation.
pub fn get_asset_list(env: &Env) -> Vec<AssetKey> {
    env.storage()
        .persistent()
        .get(&ASSET_LIST)
        .unwrap_or(Vec::new(env))
}

/// Look up the configuration for a specific asset by address.
///
/// # Arguments
/// * `env` — The contract environment
/// * `asset` — Asset address (`None` for native XLM)
///
/// # Returns
/// The [`AssetConfig`] for the requested asset.
///
/// # Errors
/// * `AssetNotConfigured` — No configuration exists for this asset.
pub fn get_asset_config_by_address(
    env: &Env,
    asset: Option<Address>,
) -> Result<AssetConfig, CrossAssetError> {
    let asset_key = AssetKey::from_option(asset);
    get_asset_config(env, &asset_key)
}

/// Get total supply for a specific asset.
///
/// # Arguments
/// * `env` — The contract environment
/// * `asset` — Asset address (`None` for native XLM)
///
/// # Returns
/// Total supply amount. Returns 0 if no supply recorded.
pub fn get_total_supply_for(env: &Env, asset: Option<Address>) -> i128 {
    let asset_key = AssetKey::from_option(asset);
    get_total_supply(env, &asset_key)
}

/// Get total borrows for a specific asset.
///
/// # Arguments
/// * `env` — The contract environment
/// * `asset` — Asset address (`None` for native XLM)
///
/// # Returns
/// Total borrow amount. Returns 0 if no borrows recorded.
pub fn get_total_borrow_for(env: &Env, asset: Option<Address>) -> i128 {
    let asset_key = AssetKey::from_option(asset);
    get_total_borrow(env, &asset_key)
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Update user's position for a specific asset in persistent storage.
fn set_user_asset_position(
    env: &Env,
    user: &Address,
    asset: Option<Address>,
    position: AssetPosition,
) {
    let key = UserAssetKey::new(user.clone(), asset);
    let mut positions: Map<UserAssetKey, AssetPosition> = env
        .storage()
        .persistent()
        .get(&USER_POSITIONS)
        .unwrap_or(Map::new(env));

    positions.set(key, position);
    env.storage().persistent().set(&USER_POSITIONS, &positions);
}

/// Quick check whether a user has any outstanding debt across all assets.
/// Used to decide whether stale-price errors are blocking for withdrawals.
fn user_has_any_debt(env: &Env, user: &Address) -> bool {
    let asset_list: Vec<AssetKey> = env
        .storage()
        .persistent()
        .get(&ASSET_LIST)
        .unwrap_or(Vec::new(env));

    // #530: Bound the iteration so that large asset registries cannot exhaust
    // Soroban's per-transaction CPU/memory budget.
    //
    // # Security
    // If more assets than MAX_ASSETS_PER_SUMMARY are registered, only the first
    // MAX_ASSETS_PER_SUMMARY are considered. Callers should be aware that the
    // summary may be partial when the registry is at capacity. This is a
    // deliberate trade-off between completeness and DoS-resistance.
    let asset_count = asset_list.len().min(MAX_ASSETS_PER_SUMMARY);

    for i in 0..asset_count {
        let asset_key = asset_list.get(i).unwrap();
        let position = get_user_asset_position(env, user, asset_key.to_option());
        if position.debt_principal > 0 || position.accrued_interest > 0 {
            return true;
        }
    }
    false
}

/// Look up asset config from the global config map.
fn get_asset_config(env: &Env, asset_key: &AssetKey) -> Result<AssetConfig, CrossAssetError> {
    let configs: Map<AssetKey, AssetConfig> = env
        .storage()
        .persistent()
        .get(&ASSET_CONFIGS)
        .unwrap_or(Map::new(env));

    configs
        .get(asset_key.clone())
        .ok_or(CrossAssetError::AssetNotConfigured)
}

/// Validate a complete asset configuration.
///
/// Checks:
/// 1. All basis-point fields are in [0, 10_000]
/// 2. Liquidation threshold >= collateral factor (LTV)
/// 3. Price > 0
/// 4. Caps are non-negative
fn require_valid_config(config: &AssetConfig) -> Result<(), CrossAssetError> {
    require_valid_basis_points(config.collateral_factor)?;
    require_valid_basis_points(config.liquidation_threshold)?;
    require_valid_basis_points(config.reserve_factor)?;

    if config.price <= 0 {
        return Err(CrossAssetError::InvalidPrice);
    }

    if config.liquidation_threshold < config.collateral_factor {
        return Err(CrossAssetError::InvalidConfig);
    }

    if config.max_supply < 0 || config.max_borrow < 0 {
        return Err(CrossAssetError::InvalidConfig);
    }

    Ok(())
}

/// Validate that a value is in the valid basis-point range [0, 10_000].
fn require_valid_basis_points(value: i128) -> Result<(), CrossAssetError> {
    if !(0..=BPS_DENOMINATOR).contains(&value) {
        return Err(CrossAssetError::InvalidConfig);
    }
    Ok(())
}

/// Validate that an amount is strictly positive.
fn require_positive_amount(amount: i128) -> Result<(), CrossAssetError> {
    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }
    Ok(())
}

// -- Checked arithmetic wrappers --

fn checked_add(a: i128, b: i128) -> Result<i128, CrossAssetError> {
    a.checked_add(b).ok_or(CrossAssetError::Overflow)
}

fn checked_sub(a: i128, b: i128) -> Result<i128, CrossAssetError> {
    a.checked_sub(b).ok_or(CrossAssetError::Overflow)
}

fn checked_mul(a: i128, b: i128) -> Result<i128, CrossAssetError> {
    a.checked_mul(b).ok_or(CrossAssetError::Overflow)
}

// -- Total supply / borrow accounting --

fn get_total_supply(env: &Env, asset_key: &AssetKey) -> i128 {
    let supplies: Map<AssetKey, i128> = env
        .storage()
        .persistent()
        .get(&TOTAL_SUPPLIES)
        .unwrap_or(Map::new(env));

    supplies.get(asset_key.clone()).unwrap_or(0)
}

fn update_total_supply(
    env: &Env,
    asset_key: &AssetKey,
    delta: i128,
) -> Result<(), CrossAssetError> {
    let mut supplies: Map<AssetKey, i128> = env
        .storage()
        .persistent()
        .get(&TOTAL_SUPPLIES)
        .unwrap_or(Map::new(env));

    let current = supplies.get(asset_key.clone()).unwrap_or(0);
    let new_value = checked_add(current, delta)?;
    // Prevent underflow below zero
    if new_value < 0 {
        return Err(CrossAssetError::Overflow);
    }
    supplies.set(asset_key.clone(), new_value);
    env.storage().persistent().set(&TOTAL_SUPPLIES, &supplies);
    Ok(())
}

fn get_total_borrow(env: &Env, asset_key: &AssetKey) -> i128 {
    let borrows: Map<AssetKey, i128> = env
        .storage()
        .persistent()
        .get(&TOTAL_BORROWS)
        .unwrap_or(Map::new(env));

    borrows.get(asset_key.clone()).unwrap_or(0)
}

fn update_total_borrow(
    env: &Env,
    asset_key: &AssetKey,
    delta: i128,
) -> Result<(), CrossAssetError> {
    let mut borrows: Map<AssetKey, i128> = env
        .storage()
        .persistent()
        .get(&TOTAL_BORROWS)
        .unwrap_or(Map::new(env));

    let current = borrows.get(asset_key.clone()).unwrap_or(0);
    let new_value = checked_add(current, delta)?;
    // Prevent underflow below zero
    if new_value < 0 {
        return Err(CrossAssetError::Overflow);
    }
    borrows.set(asset_key.clone(), new_value);
    env.storage().persistent().set(&TOTAL_BORROWS, &borrows);
    Ok(())
}

// ============================================================================
// Impl blocks
// ============================================================================

impl UserAssetKey {
    pub fn new(user: Address, asset: Option<Address>) -> Self {
        Self {
            user,
            asset: AssetKey::from_option(asset),
        }
    }
}

impl AssetKey {
    /// Convert an `Option<Address>` into an `AssetKey` (`None` → `Native`).
    pub fn from_option(asset: Option<Address>) -> Self {
        match asset {
            Some(addr) => AssetKey::Token(addr),
            None => AssetKey::Native,
        }
    }

    /// Convert back to `Option<Address>` (`Native` → `None`).
    pub fn to_option(&self) -> Option<Address> {
        match self {
            AssetKey::Native => None,
            AssetKey::Token(addr) => Some(addr.clone()),
        }
    }
}
