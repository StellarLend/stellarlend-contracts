//! # Cross-Asset Module
//!
//! Manages multi-asset collateral and borrow positions. All value aggregation
//! normalises per-asset oracle prices to a shared internal scale before
//! summing, so assets with different `price_decimals` (e.g. 6 vs 18) cannot
//! silently mis-value a position.
//!
//! ## Internal scale
//! Every dollar-value computed here is expressed in `INTERNAL_DECIMALS` (18)
//! fixed-point units.  A helper [`normalize_price`] converts an asset's raw
//! price (stored with `price_decimals` fractional digits) to that scale using
//! checked 128-bit arithmetic.

#![allow(unused)]

use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Common internal fixed-point scale for value aggregation (10^18).
pub const INTERNAL_DECIMALS: u32 = 18;

/// Lower bound (inclusive) for `AssetConfig::collateral_factor_bps`.
pub const MIN_COLLATERAL_FACTOR_BPS: i128 = 0;

/// Upper bound (inclusive) for `AssetConfig::collateral_factor_bps` (100 %).
pub const MAX_COLLATERAL_FACTOR_BPS: i128 = 10_000;

// ---------------------------------------------------------------------------
// Admin storage
// ---------------------------------------------------------------------------

/// Storage key for the module's admin address.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrossAssetAdminKey {
    Admin,
}

/// Store an admin address (called once during protocol initialisation).
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage()
        .persistent()
        .set(&CrossAssetAdminKey::Admin, admin);
}

/// Return the stored admin address, or `None` if not yet set.
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage()
        .persistent()
        .get::<CrossAssetAdminKey, Address>(&CrossAssetAdminKey::Admin)
}

/// Admin-only: set the maximum number of distinct debt assets a single user may hold.
/// Pass `None` to remove the cap (unlimited).
/// When setting a value it must be >= 1.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - Must be the stored admin address
/// * `max` - New cap value, or None to disable
///
/// # Errors
/// * `CrossAssetError::Unauthorized` - If caller is not admin
/// * `CrossAssetError::InvalidMaxDebtAssets` - If max is Some(0)
pub fn set_max_debt_assets_per_user(
    env: &Env,
    caller: &Address,
    max: Option<u32>,
) -> Result<(), CrossAssetError> {
    crate::admin::require_admin(env, caller).map_err(|_| CrossAssetError::Unauthorized)?;

    if let Some(v) = max {
        if v < 1 {
            return Err(CrossAssetError::InvalidMaxDebtAssets);
        }
    }

    let key = CrossAssetDataKey::MaxDebtAssetsPerUser;
    match max {
        Some(v) => env.storage().persistent().set(&key, &v),
        None => env.storage().persistent().remove(&key),
    }
    Ok(())
}

/// Read-only getter for the current max-debt-assets-per-user cap.
/// Returns None when no cap is configured (unlimited).
///
/// # Arguments
/// * `env` - The Soroban environment
pub fn get_max_debt_assets_per_user(env: &Env) -> Option<u32> {
    let key = CrossAssetDataKey::MaxDebtAssetsPerUser;
    env.storage()
        .persistent()
        .get::<CrossAssetDataKey, u32>(&key)
}

/// Require that `caller` is the stored admin; returns `Unauthorized` otherwise.
///
/// Calls `caller.require_auth()` so that Soroban enforces a cryptographic
/// signature check, consistent with `admin::require_admin` and
/// `bridge::require_guardian`.  A pure address-equality check without
/// `require_auth` would allow any account to spoof the admin address as a
/// plain argument with no proof of key ownership.
fn require_admin(env: &Env, caller: &Address) -> Result<(), CrossAssetError> {
    caller.require_auth();
    let admin = get_admin(env).ok_or(CrossAssetError::Unauthorized)?;
    if &admin != caller {
        return Err(CrossAssetError::Unauthorized);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur in cross-asset operations.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CrossAssetError {
    /// Asset is not registered in the protocol.
    AssetNotFound = 1,
    /// Asset is already registered.
    AssetAlreadyExists = 2,
    /// Supplied amount is zero or negative.
    InvalidAmount = 3,
    /// Borrowing is not enabled for this asset.
    BorrowNotAllowed = 4,
    /// Collateralisation is not enabled for this asset.
    CollateralNotAllowed = 5,
    /// User has insufficient collateral to borrow or withdraw.
    InsufficientCollateral = 6,
    /// Arithmetic overflow during value normalization.
    Overflow = 7,
    /// price_decimals value is out of the allowed range (0..=38).
    InvalidDecimals = 8,
    /// `collateral_factor_bps` is outside the allowed range [0, 10_000].
    InvalidCollateralFactor = 9,
    /// Caller is not the protocol admin.
    Unauthorized = 10,
    /// `collateral_factor_bps` exceeds `liquidation_threshold`.
    LtvExceedsThreshold = 11,
    /// `price_decimals` is zero — silently mis-scales all oracle prices.
    ZeroDecimals = 12,
    /// `set_max_debt_assets_per_user` called with `max = Some(0)`.
    InvalidMaxDebtAssets = 13,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted by [`update_asset_config`] on every successful configuration change.
/// All fields reflect the **post-update** state of the asset config.
///
/// Topics: `("crossAsst", "cfgUpd")`
#[contracttype]
#[derive(Clone, Debug)]
pub struct ConfigUpdatedEvent {
    /// Asset key identifying the updated asset.
    pub asset_key: AssetKey,
    /// Post-update collateral factor in basis points.
    pub collateral_factor_bps: i128,
    /// Post-update liquidation threshold in basis points.
    pub liquidation_threshold: i128,
    /// Post-update maximum supply cap (0 = unlimited).
    pub max_supply: i128,
    /// Post-update maximum borrow cap (0 = unlimited).
    pub max_borrow: i128,
    /// Post-update `can_collateralize` flag.
    pub can_collateralize: bool,
    /// Post-update `can_borrow` flag.
    pub can_borrow: bool,
}

/// Emit a [`ConfigUpdatedEvent`].
pub fn emit_config_updated(env: &Env, event: ConfigUpdatedEvent) {
    env.events()
        .publish((symbol_short!("crossAsst"), symbol_short!("cfgUpd")), event);
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Per-record storage keys used by the cross-asset module.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetKey {
    /// Native / sentinel "no address" slot.
    Native,
    /// A specific token address.
    Token(Address),
}

/// Persistent storage keys for the hello-world cross-asset module.
///
/// Documented in [`docs/CROSS_ASSET_STORAGE_LAYOUT.md`](../docs/CROSS_ASSET_STORAGE_LAYOUT.md).
/// New variants must be appended to preserve upgrade compatibility.
#[contracttype]
#[derive(Clone, Debug)]
pub enum CrossAssetDataKey {
    Config(AssetKey),
    AssetList,
    UserSupply(AssetKey, Address),
    UserDebt(AssetKey, Address),
    TotalSupply(AssetKey),
    TotalDebt(AssetKey),
    /// Optional cap on distinct debt assets per user (`None` / absent = unlimited).
    MaxDebtAssetsPerUser,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-asset borrow-power breakdown entry.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetBorrowPower {
    pub asset_key: AssetKey,
    pub collateral_value: i128,
    pub borrow_capacity: i128,
    pub collateral_factor_bps: i128,
}

/// Configuration for a single asset registered in the protocol.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetConfig {
    /// Per-asset collateral factor in basis points (e.g. 7500 = 75 %).
    /// Must be in `0..=10_000`.
    pub collateral_factor_bps: i128,
    /// Liquidation threshold in basis points.
    pub liquidation_threshold: i128,
    /// Maximum total supply allowed (0 = unlimited).
    pub max_supply: i128,
    /// Maximum total borrows allowed (0 = unlimited).
    pub max_borrow: i128,
    /// Whether this asset can be used as collateral.
    pub can_collateralize: bool,
    /// Whether this asset can be borrowed.
    pub can_borrow: bool,
    /// Most-recent oracle price (raw units, scaled by 10^price_decimals).
    pub price: i128,
    /// Number of decimal places for the oracle price feed. Must be in 1..=38.
    pub price_decimals: u32,
    /// Ledger timestamp when the asset price was last updated.
    pub last_update_ts: u64,
}

/// A user's supply/debt balances for a single asset.
#[contracttype]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssetPosition {
    pub supplied: i128,
    pub borrowed: i128,
}

/// Aggregated position summary across all assets (18-decimal fixed-point).
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct UserPositionSummary {
    pub total_collateral_value: i128,
    pub total_debt_value: i128,
    pub borrow_capacity: i128,
    /// 1 if healthy, 0 if under-water.
    pub is_healthy: u32,
}

// ---------------------------------------------------------------------------
// Decimal normalization
// ---------------------------------------------------------------------------

fn pow10_checked(exp: u32) -> Option<i128> {
    let mut acc: i128 = 1;
    for _ in 0..exp {
        acc = acc.checked_mul(10)?;
    }
    Some(acc)
}

/// Normalise `raw_price` to the common `INTERNAL_DECIMALS` (18) scale.
/// Uses floor division — conservative for collateral values.
pub fn normalize_price(raw_price: i128, asset_decimals: u32) -> Option<i128> {
    if asset_decimals == INTERNAL_DECIMALS {
        return Some(raw_price);
    }
    if asset_decimals < INTERNAL_DECIMALS {
        let scale = pow10_checked(INTERNAL_DECIMALS - asset_decimals)?;
        raw_price.checked_mul(scale)
    } else {
        let scale = pow10_checked(asset_decimals - INTERNAL_DECIMALS)?;
        Some(raw_price / scale)
    }
}

/// Same as [`normalize_price`] but rounds up — conservative for debt values.
pub fn normalize_price_ceil(raw_price: i128, asset_decimals: u32) -> Option<i128> {
    if asset_decimals <= INTERNAL_DECIMALS {
        normalize_price(raw_price, asset_decimals)
    } else {
        let scale = pow10_checked(asset_decimals - INTERNAL_DECIMALS)?;
        let adjusted = raw_price.checked_add(scale.checked_sub(1)?)?;
        Some(adjusted / scale)
    }
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn asset_key(asset: Option<Address>) -> AssetKey {
    match asset {
        Some(a) => AssetKey::Token(a),
        None => AssetKey::Native,
    }
}

fn load_config(env: &Env, key: &AssetKey) -> Result<AssetConfig, CrossAssetError> {
    env.storage()
        .persistent()
        .get::<CrossAssetDataKey, AssetConfig>(&CrossAssetDataKey::Config(key.clone()))
        .ok_or(CrossAssetError::AssetNotFound)
}

fn save_config(env: &Env, key: &AssetKey, cfg: &AssetConfig) {
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::Config(key.clone()), cfg);
}

fn load_user_position(env: &Env, key: &AssetKey, user: &Address) -> AssetPosition {
    let supply = env
        .storage()
        .persistent()
        .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::UserSupply(key.clone(), user.clone()))
        .unwrap_or(0);
    let borrow = env
        .storage()
        .persistent()
        .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::UserDebt(key.clone(), user.clone()))
        .unwrap_or(0);
    AssetPosition {
        supplied: supply,
        borrowed: borrow,
    }
}

fn save_user_supply(env: &Env, key: &AssetKey, user: &Address, amount: i128) {
    env.storage().persistent().set(
        &CrossAssetDataKey::UserSupply(key.clone(), user.clone()),
        &amount,
    );
}

fn save_user_debt(env: &Env, key: &AssetKey, user: &Address, amount: i128) {
    env.storage().persistent().set(
        &CrossAssetDataKey::UserDebt(key.clone(), user.clone()),
        &amount,
    );
}

fn load_total_supply(env: &Env, key: &AssetKey) -> i128 {
    env.storage()
        .persistent()
        .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::TotalSupply(key.clone()))
        .unwrap_or(0)
}

fn save_total_supply(env: &Env, key: &AssetKey, v: i128) {
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::TotalSupply(key.clone()), &v);
}

fn load_total_debt(env: &Env, key: &AssetKey) -> i128 {
    env.storage()
        .persistent()
        .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::TotalDebt(key.clone()))
        .unwrap_or(0)
}

fn save_total_debt(env: &Env, key: &AssetKey, v: i128) {
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::TotalDebt(key.clone()), &v);
}

fn load_asset_list(env: &Env) -> Vec<AssetKey> {
    env.storage()
        .persistent()
        .get::<CrossAssetDataKey, Vec<AssetKey>>(&CrossAssetDataKey::AssetList)
        .unwrap_or_else(|| Vec::new(env))
}

fn save_asset_list(env: &Env, list: &Vec<AssetKey>) {
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::AssetList, list);
}

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

#[cfg(test)]
use soroban_sdk::{contract, contractimpl};

/// Minimal no-op contract used in tests to establish a contract execution context.
#[cfg(test)]
#[contract]
pub struct NoOpContract;

#[cfg(test)]
#[contractimpl]
impl NoOpContract {}

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

/// Initialize the cross-asset module, setting the admin address for subsequent
/// operations that require authorization.
pub fn initialize(env: &Env, admin: Address) -> Result<(), CrossAssetError> {
    set_admin(env, &admin);
    Ok(())
}

/// Register a new asset with its initial configuration.
///
/// # Access control
/// `caller` must equal the stored admin address, else
/// [`CrossAssetError::Unauthorized`] is returned before any state is touched.
///
/// # Errors
/// - [`CrossAssetError::Unauthorized`] — caller is not the protocol admin.
/// - [`CrossAssetError::AssetAlreadyExists`] — asset key already registered.
/// - [`CrossAssetError::InvalidDecimals`] — `config.price_decimals > 38`.
/// - [`CrossAssetError::InvalidCollateralFactor`] — factor outside `[0, 10_000]`.
pub fn initialize_asset(
    env: &Env,
    caller: &Address,
    asset: Option<Address>,
    config: AssetConfig,
) -> Result<(), CrossAssetError> {
    require_admin(env, caller)?;

    if config.price_decimals > 38 {
        return Err(CrossAssetError::InvalidDecimals);
    }
    if config.collateral_factor_bps < MIN_COLLATERAL_FACTOR_BPS
        || config.collateral_factor_bps > MAX_COLLATERAL_FACTOR_BPS
    {
        return Err(CrossAssetError::InvalidCollateralFactor);
    }
    let key = asset_key(asset);
    if env
        .storage()
        .persistent()
        .has(&CrossAssetDataKey::Config(key.clone()))
    {
        return Err(CrossAssetError::AssetAlreadyExists);
    }
    let mut cfg = config;
    if cfg.last_update_ts == 0 {
        cfg.last_update_ts = env.ledger().timestamp();
    }
    save_config(env, &key, &cfg);
    let mut list = load_asset_list(env);
    list.push_back(key);
    save_asset_list(env, &list);
    Ok(())
}

/// Update mutable fields of an existing asset's configuration.
///
/// Only `Some(...)` fields are changed; `None` fields are no-ops.
///
/// # Access control
/// `caller` must equal the stored admin address, else
/// [`CrossAssetError::Unauthorized`] is returned before any state is touched.
///
/// # Validation (checked against the **post-update** config)
/// | Rule | Error |
/// |------|-------|
/// | `collateral_factor_bps` ∈ \[0, 10_000\] | `InvalidCollateralFactor` |
/// | `collateral_factor_bps` ≤ `liquidation_threshold` | `LtvExceedsThreshold` |
/// | `price_decimals` ≠ 0 | `ZeroDecimals` |
/// | `price_decimals` ≤ 38 | `InvalidDecimals` |
///
/// # Events
/// Emits [`ConfigUpdatedEvent`] on success; no event on failure.
#[allow(clippy::too_many_arguments)]
pub fn update_asset_config(
    env: &Env,
    caller: &Address,
    asset: Option<Address>,
    collateral_factor_bps: Option<i128>,
    liquidation_threshold: Option<i128>,
    max_supply: Option<i128>,
    max_borrow: Option<i128>,
    can_collateralize: Option<bool>,
    can_borrow: Option<bool>,
    price_decimals: Option<u32>,
) -> Result<(), CrossAssetError> {
    require_admin(env, caller)?;

    let key = asset_key(asset);
    let mut cfg = load_config(env, &key)?;

    if let Some(v) = collateral_factor_bps {
        if v < MIN_COLLATERAL_FACTOR_BPS || v > MAX_COLLATERAL_FACTOR_BPS {
            return Err(CrossAssetError::InvalidCollateralFactor);
        }
        cfg.collateral_factor_bps = v;
    }
    if let Some(v) = liquidation_threshold {
        cfg.liquidation_threshold = v;
    }
    if let Some(v) = max_supply {
        cfg.max_supply = v;
    }
    if let Some(v) = max_borrow {
        cfg.max_borrow = v;
    }
    if let Some(v) = can_collateralize {
        cfg.can_collateralize = v;
    }
    if let Some(v) = can_borrow {
        cfg.can_borrow = v;
    }
    if let Some(v) = price_decimals {
        if v == 0 {
            return Err(CrossAssetError::ZeroDecimals);
        }
        if v > 38 {
            return Err(CrossAssetError::InvalidDecimals);
        }
        cfg.price_decimals = v;
    }

    // Cross-field invariant: LTV must not exceed the liquidation threshold.
    if cfg.collateral_factor_bps > cfg.liquidation_threshold {
        return Err(CrossAssetError::LtvExceedsThreshold);
    }

    save_config(env, &key, &cfg);

    emit_config_updated(
        env,
        ConfigUpdatedEvent {
            asset_key: key,
            collateral_factor_bps: cfg.collateral_factor_bps,
            liquidation_threshold: cfg.liquidation_threshold,
            max_supply: cfg.max_supply,
            max_borrow: cfg.max_borrow,
            can_collateralize: cfg.can_collateralize,
            can_borrow: cfg.can_borrow,
        },
    );

    Ok(())
}

/// Store the latest oracle price for an asset and update its timestamp.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - Optional token address (`None` for native asset)
/// * `price` - Positive raw oracle price value
///
/// # Errors
/// * [`CrossAssetError::InvalidAmount`] - If `price <= 0`
/// * [`CrossAssetError::AssetNotFound`] - If the specified asset is not registered
pub fn update_asset_price(
    env: &Env,
    caller: &Address,
    asset: Option<Address>,
    price: i128,
) -> Result<(), CrossAssetError> {
    require_admin(env, caller)?;

    if price <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }
    let key = asset_key(asset);
    let mut cfg = load_config(env, &key)?;
    cfg.price = price;
    cfg.last_update_ts = env.ledger().timestamp();
    save_config(env, &key, &cfg);
    Ok(())
}

/// Return how old (in seconds) the stored oracle price for an asset is.
///
/// Age is calculated as `now - price_timestamp` where `now` is the current
/// ledger timestamp (`env.ledger().timestamp()`) and `price_timestamp` is
/// `cfg.last_update_ts`. Uses saturating subtraction to prevent underflow.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - Optional token address (`None` for native asset)
///
/// # Errors
/// * [`CrossAssetError::AssetNotFound`] - If the specified asset is not registered
pub fn get_asset_price_age(
    env: &Env,
    asset: Option<Address>,
) -> Result<u64, CrossAssetError> {
    let key = asset_key(asset);
    let cfg = load_config(env, &key)?;
    let now = env.ledger().timestamp();
    Ok(now.saturating_sub(cfg.last_update_ts))
}

/// Return the configuration for a given asset.
pub fn get_asset_config_by_address(
    env: &Env,
    asset: Option<Address>,
) -> Result<AssetConfig, CrossAssetError> {
    load_config(env, &asset_key(asset))
}

/// Return the list of all registered asset keys.
pub fn get_asset_list(env: &Env) -> Vec<AssetKey> {
    load_asset_list(env)
}

/// Return total protocol-wide supply for an asset (raw token units).
pub fn get_total_supply_for(env: &Env, asset: Option<Address>) -> i128 {
    load_total_supply(env, &asset_key(asset))
}

/// Return total protocol-wide debt for an asset (raw token units).
pub fn get_total_borrow_for(env: &Env, asset: Option<Address>) -> i128 {
    load_total_debt(env, &asset_key(asset))
}

/// Return a user's supply/debt balances for a single asset.
pub fn get_user_asset_position(env: &Env, user: &Address, asset: Option<Address>) -> AssetPosition {
    load_user_position(env, &asset_key(asset), user)
}

/// Compute the user's aggregated position across all registered assets.
///
/// Collateral values use floor normalisation; debt values use ceiling.
pub fn get_user_position_summary(
    env: &Env,
    user: &Address,
) -> Result<UserPositionSummary, CrossAssetError> {
    let list = load_asset_list(env);
    let mut total_collateral: i128 = 0;
    let mut total_debt: i128 = 0;
    let mut borrow_capacity: i128 = 0;

    for i in 0..list.len() {
        let key = list.get(i).unwrap();
        let cfg = load_config(env, &key)?;
        let pos = load_user_position(env, &key, user);

        let norm_price =
            normalize_price(cfg.price, cfg.price_decimals).ok_or(CrossAssetError::Overflow)?;
        let norm_price_ceil =
            normalize_price_ceil(cfg.price, cfg.price_decimals).ok_or(CrossAssetError::Overflow)?;

        if pos.supplied > 0 && cfg.can_collateralize {
            let val = (pos.supplied as i128)
                .checked_mul(norm_price)
                .ok_or(CrossAssetError::Overflow)?
                / pow10_checked(INTERNAL_DECIMALS).ok_or(CrossAssetError::Overflow)?;
            total_collateral = total_collateral
                .checked_add(val)
                .ok_or(CrossAssetError::Overflow)?;
            let cap = val
                .checked_mul(cfg.collateral_factor_bps)
                .ok_or(CrossAssetError::Overflow)?
                / 10_000;
            borrow_capacity = borrow_capacity
                .checked_add(cap)
                .ok_or(CrossAssetError::Overflow)?;
        }

        if pos.borrowed > 0 {
            let val_num = (pos.borrowed as i128)
                .checked_mul(norm_price_ceil)
                .ok_or(CrossAssetError::Overflow)?;
            let scale = pow10_checked(INTERNAL_DECIMALS).ok_or(CrossAssetError::Overflow)?;
            let val = (val_num + scale - 1) / scale;
            total_debt = total_debt
                .checked_add(val)
                .ok_or(CrossAssetError::Overflow)?;
        }
    }

    let is_healthy = if total_debt == 0 || borrow_capacity >= total_debt {
        1
    } else {
        0
    };

    Ok(UserPositionSummary {
        total_collateral_value: total_collateral,
        total_debt_value: total_debt,
        borrow_capacity,
        is_healthy,
    })
}

/// Return per-asset borrow-power breakdown for `user`.
pub fn get_borrow_power_by_asset(
    env: &Env,
    user: &Address,
) -> Result<Vec<AssetBorrowPower>, CrossAssetError> {
    let list = load_asset_list(env);
    let mut result = Vec::new(env);

    for i in 0..list.len() {
        let key = list.get(i).unwrap();
        let cfg = load_config(env, &key)?;
        let pos = load_user_position(env, &key, user);

        if pos.supplied == 0 || !cfg.can_collateralize {
            continue;
        }

        let norm_price =
            normalize_price(cfg.price, cfg.price_decimals).ok_or(CrossAssetError::Overflow)?;

        let collateral_value = (pos.supplied as i128)
            .checked_mul(norm_price)
            .ok_or(CrossAssetError::Overflow)?
            / pow10_checked(INTERNAL_DECIMALS).ok_or(CrossAssetError::Overflow)?;

        let borrow_capacity = collateral_value
            .checked_mul(cfg.collateral_factor_bps)
            .ok_or(CrossAssetError::Overflow)?
            / 10_000;

        result.push_back(AssetBorrowPower {
            asset_key: key,
            collateral_value,
            borrow_capacity,
            collateral_factor_bps: cfg.collateral_factor_bps,
        });
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Cross-asset operations
// ---------------------------------------------------------------------------

/// Deposit `amount` of an asset for `user`.
pub fn cross_asset_deposit(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }
    let key = asset_key(asset);
    let _cfg = load_config(env, &key)?;

    let mut pos = load_user_position(env, &key, &user);
    pos.supplied = pos
        .supplied
        .checked_add(amount)
        .ok_or(CrossAssetError::Overflow)?;
    save_user_supply(env, &key, &user, pos.supplied);

    let total = load_total_supply(env, &key)
        .checked_add(amount)
        .ok_or(CrossAssetError::Overflow)?;
    save_total_supply(env, &key, total);

    Ok(pos)
}

/// Withdraw `amount` of a previously deposited asset.
pub fn cross_asset_withdraw(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }
    let key = asset_key(asset);
    let mut pos = load_user_position(env, &key, &user);
    if pos.supplied < amount {
        return Err(CrossAssetError::InsufficientCollateral);
    }
    pos.supplied -= amount;
    save_user_supply(env, &key, &user, pos.supplied);

    let total = load_total_supply(env, &key) - amount;
    save_total_supply(env, &key, total);

    Ok(pos)
}

/// Borrow `amount` of an asset for `user`.
pub fn cross_asset_borrow(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }
    let key = asset_key(asset.clone());
    let cfg = load_config(env, &key)?;
    if !cfg.can_borrow {
        return Err(CrossAssetError::BorrowNotAllowed);
    }

    let mut pos = load_user_position(env, &key, &user);
    pos.borrowed = pos
        .borrowed
        .checked_add(amount)
        .ok_or(CrossAssetError::Overflow)?;
    save_user_debt(env, &key, &user, pos.borrowed);

    let total = load_total_debt(env, &key)
        .checked_add(amount)
        .ok_or(CrossAssetError::Overflow)?;
    save_total_debt(env, &key, total);

    let summary = get_user_position_summary(env, &user)?;
    if summary.is_healthy == 0 {
        pos.borrowed -= amount;
        save_user_debt(env, &key, &user, pos.borrowed);
        save_total_debt(env, &key, total - amount);
        return Err(CrossAssetError::InsufficientCollateral);
    }

    Ok(pos)
}

/// Repay `amount` of a borrowed asset.
pub fn cross_asset_repay(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<AssetPosition, CrossAssetError> {
    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }
    let key = asset_key(asset);
    let mut pos = load_user_position(env, &key, &user);
    let repay = amount.min(pos.borrowed);
    pos.borrowed -= repay;
    save_user_debt(env, &key, &user, pos.borrowed);

    let total = (load_total_debt(env, &key) - repay).max(0);
    save_total_debt(env, &key, total);

    Ok(pos)
}
