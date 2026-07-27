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

// Re-export shared price-normalisation utilities from the protocol's common
// crate so all call sites use identical arithmetic and scale constants.
pub use stellar_lend_common::{normalize_price, normalize_price_ceil, pow10_checked, INTERNAL_DECIMALS};

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

/// Require that `caller` is the stored admin; returns `Unauthorized` otherwise.
fn require_admin(env: &Env, caller: &Address) -> Result<(), CrossAssetError> {
    let admin = get_admin(env).ok_or(CrossAssetError::Unauthorized)?;
    if &admin != caller {
        return Err(CrossAssetError::Unauthorized);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Lower bound (inclusive) for `AssetConfig::collateral_factor_bps`.
///
/// A factor of 0 means the asset can be supplied but contributes no
/// borrow capacity — it's a recognised position but cannot underwrite debt.
pub const MIN_COLLATERAL_FACTOR_BPS: i128 = 0;

/// Upper bound (inclusive) for `AssetConfig::collateral_factor_bps`.
///
/// 10_000 bps == 100 % == full LTV.
pub const MAX_COLLATERAL_FACTOR_BPS: i128 = 10_000;

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
    /// `collateral_factor_bps` exceeds `liquidation_threshold`, which would
    /// allow positions to be born underwater (LTV > liquidation ratio).
    LtvExceedsThreshold = 11,
    /// `price_decimals` is zero, which is a misconfiguration that silently
    /// mis-scales all oracle prices for this asset.
    ZeroDecimals = 12,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted by [`update_asset_config`] on every successful configuration change.
///
/// All fields reflect the **post-update** state of the asset config.
/// Indexers should compare against the previous on-chain state to determine
/// which fields changed.
///
/// Topics: `("cross_asset", "config_updated")`
#[contracttype]
#[derive(Clone, Debug)]
pub struct ConfigUpdatedEvent {
    /// Asset key identifying the updated asset (`AssetKey::Native` or
    /// `AssetKey::Token(address)`).
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
///
/// Topics: `("cross_asset", "config_updated")`
pub fn emit_config_updated(env: &Env, event: ConfigUpdatedEvent) {
    env.events()
        .publish((symbol_short!("crossAsst"), symbol_short!("cfgUpd")), event);
}

// ---------------------------------------------------------------------------
// Storage key
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

#[contracttype]
#[derive(Clone, Debug)]
enum CrossAssetDataKey {
    /// [`AssetConfig`] for a given asset.
    Config(AssetKey),
    /// List of all registered [`AssetKey`]s.
    AssetList,
    /// Per-user supply balance for an asset.
    UserSupply(AssetKey, Address),
    /// Per-user debt balance for an asset.
    UserDebt(AssetKey, Address),
    /// Protocol-wide total supply for an asset.
    TotalSupply(AssetKey),
    /// Protocol-wide total debt for an asset.
    TotalDebt(AssetKey),
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-asset borrow-power breakdown entry for `get_borrow_power_by_asset`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetBorrowPower {
    /// Asset key identifying the collateral asset.
    pub asset_key: AssetKey,
    /// Collateral value of this asset (normalised, 18-dp).
    pub collateral_value: i128,
    /// Borrow capacity contributed by this asset
    /// = collateral_value × collateral_factor_bps / 10_000.
    pub borrow_capacity: i128,
    /// Collateral factor in basis points for this asset.
    pub collateral_factor_bps: i128,
}

/// Configuration for a single asset registered in the protocol.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetConfig {
    /// Per-asset collateral factor in basis points (e.g. 7500 = 75 %).
    /// Must be in `0..=10_000`. A value of 0 means the asset can be
    /// supplied as collateral but contributes zero borrow capacity —
    /// useful for assets that should be recognised but never back debt.
    /// The full-fraction value 10_000 means 100 % LTV (matching pre-tier
    /// behaviour).
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
    /// Number of decimal places used by the oracle price feed for this asset.
    /// Must be in 0..=38. Typical values: 6 (USD stablecoins), 8 (BTC/ETH
    /// feeds), 18 (18-decimal ERC-20-style tokens).
    pub price_decimals: u32,
    /// Ledger timestamp when the asset price was last updated.
    pub last_update_ts: u64,
}

/// A user's supply/debt balances for a single asset.
#[contracttype]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssetPosition {
    /// Amount the user has supplied (raw token units).
    pub supplied: i128,
    /// Amount the user has borrowed (raw token units).
    pub borrowed: i128,
}

/// Aggregated position summary across all assets, expressed in the internal
/// 18-decimal fixed-point scale.
#[contracttype]
#[derive(Clone, Debug, Default)]
pub struct UserPositionSummary {
    /// Total collateral value (normalised, 18-dp).
    pub total_collateral_value: i128,
    /// Total debt value (normalised, 18-dp).
    pub total_debt_value: i128,
    /// Weighted borrowing capacity.
    ///
    /// `borrow_capacity = Σ_i (collateral_value_i × collateral_factor_bps_i / 10 000)`
    ///
    /// Each asset contributes according to its own
    /// [`AssetConfig::collateral_factor_bps`]; riskier assets back fewer
    /// borrowables per dollar of value.
    pub borrow_capacity: i128,
    /// 1 if the position is healthy, 0 if under-water.
    pub is_healthy: u32,
}

// ---------------------------------------------------------------------------
// Helpers
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

/// Subtract `amount` from a protocol-wide aggregate total, guarding against
/// both true `i128` overflow and the result going negative.
///
/// Note: `i128::checked_sub` only returns `None` when the mathematical
/// result would fall outside `i128`'s representable range (i.e. below
/// `i128::MIN`) — a negative result like `50 - 80 = -30` is a perfectly
/// valid `i128` value and would *not* be caught by `checked_sub` alone. A
/// negative aggregate total is a protocol-invariant violation (it means the
/// total drifted out of sync with per-user balances), so it must be treated
/// as an error here rather than silently stored or panicking downstream.
fn checked_sub_total(total: i128, amount: i128) -> Result<i128, CrossAssetError> {
    let result = total.checked_sub(amount).ok_or(CrossAssetError::Overflow)?;
    if result < 0 {
        return Err(CrossAssetError::Overflow);
    }
    Ok(result)
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
// Test harness support
// ---------------------------------------------------------------------------

/// Minimal no-op contract used in tests to establish a contract execution
/// context, which Soroban storage requires.
#[cfg(test)]
use soroban_sdk::{contract, contractimpl};

#[cfg(test)]
#[contract]
pub struct NoOpContract;

#[cfg(test)]
#[contractimpl]
impl NoOpContract {}

// ---------------------------------------------------------------------------
// Module initialization
// ---------------------------------------------------------------------------

/// Initialize the cross-asset module, setting the admin address for subsequent
/// operations that require authorization.
pub fn initialize(env: &Env, admin: Address) -> Result<(), CrossAssetError> {
    set_admin(env, &admin);
    Ok(())
}

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

/// Register a new asset with its initial configuration.
///
/// Fails with
/// - [`CrossAssetError::AssetAlreadyExists`] — asset key already registered.
/// - [`CrossAssetError::InvalidDecimals`] — `config.price_decimals > 38`.
/// - [`CrossAssetError::InvalidCollateralFactor`] — `config.collateral_factor_bps`
///   is outside `[MIN_COLLATERAL_FACTOR_BPS, MAX_COLLATERAL_FACTOR_BPS]`.
pub fn initialize_asset(
    env: &Env,
    asset: Option<Address>,
    config: AssetConfig,
) -> Result<(), CrossAssetError> {
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
/// Only the fields that are `Some(...)` are changed. Each supplied value is
/// range-checked identically to registration time.
///
/// # Access control
/// `caller` must be the stored protocol admin, else
/// [`CrossAssetError::Unauthorized`] is returned before any state is touched.
///
/// # Validation rules (all checked against the **post-update** config)
/// | Rule | Error |
/// |------|-------|
/// | `collateral_factor_bps` ∈ \[0, 10 000\] | [`InvalidCollateralFactor`] |
/// | `collateral_factor_bps` ≤ `liquidation_threshold` | [`LtvExceedsThreshold`] |
/// | `price_decimals` ≠ 0 | [`ZeroDecimals`] |
///
/// # Events
/// On success emits [`ConfigUpdatedEvent`] with the full post-update config.
///
/// [`InvalidCollateralFactor`]: CrossAssetError::InvalidCollateralFactor
/// [`LtvExceedsThreshold`]: CrossAssetError::LtvExceedsThreshold
/// [`ZeroDecimals`]: CrossAssetError::ZeroDecimals
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

    // Apply field-level updates first, then validate the resulting config.
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

    // Cross-field invariant: LTV must not exceed the liquidation threshold —
    // a position with LTV == threshold is liquidatable on creation; LTV > threshold
    // would be born underwater.
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

/// Store the latest oracle price for an asset (raw units, `price_decimals` scale).
///
/// # Access control
/// `caller` must be the stored protocol admin, else
/// [`CrossAssetError::Unauthorized`] is returned before any state is touched.
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

/// Return a user's supply/debt balances for a single asset (raw token units).
pub fn get_user_asset_position(env: &Env, user: &Address, asset: Option<Address>) -> AssetPosition {
    load_user_position(env, &asset_key(asset), user)
}

/// Compute the user's aggregated position across all registered assets.
///
/// All asset values are normalised to [`INTERNAL_DECIMALS`] (18) before
/// summation, so mixed oracle decimal scales do not corrupt the result.
///
/// Collateral value uses **floor** normalisation (conservative for the
/// protocol); debt value uses **ceiling** normalisation (also conservative for
/// the protocol).
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

        // Normalise price once per asset.
        let norm_price =
            normalize_price(cfg.price, cfg.price_decimals).ok_or(CrossAssetError::Overflow)?;
        let norm_price_ceil =
            normalize_price_ceil(cfg.price, cfg.price_decimals).ok_or(CrossAssetError::Overflow)?;

        if pos.supplied > 0 && cfg.can_collateralize {
            // collateral value: floor(supplied * normalised_price / 10^18)
            let val = (pos.supplied as i128)
                .checked_mul(norm_price)
                .ok_or(CrossAssetError::Overflow)?
                / pow10_checked(INTERNAL_DECIMALS).ok_or(CrossAssetError::Overflow)?;
            total_collateral = total_collateral
                .checked_add(val)
                .ok_or(CrossAssetError::Overflow)?;
            // borrow capacity: collateral_value * collateral_factor_bps / 10_000
            //
            // The per-asset `collateral_factor_bps` is bounded in [0, 10_000] at
            // registration / update time, so this multiplication cannot
            // accidentally amplify a value beyond 10x (the worst case is when
            // bps == 10_000, i.e. 100 % LTV, which is the pre-tier behaviour —
            // no regression for full-factor assets).
            let cap = val
                .checked_mul(cfg.collateral_factor_bps)
                .ok_or(CrossAssetError::Overflow)?
                / 10_000;
            borrow_capacity = borrow_capacity
                .checked_add(cap)
                .ok_or(CrossAssetError::Overflow)?;
        }

        if pos.borrowed > 0 {
            // debt value: ceil(borrowed * normalised_price_ceil / 10^18)
            let val_num = (pos.borrowed as i128)
                .checked_mul(norm_price_ceil)
                .ok_or(CrossAssetError::Overflow)?;
            let scale = pow10_checked(INTERNAL_DECIMALS).ok_or(CrossAssetError::Overflow)?;
            // ceiling division
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
///
/// For each registered asset where the user has supplied collateral and
/// `can_collateralize == true`, returns an `AssetBorrowPower` entry with the
/// collateral value and borrow capacity contributed by that asset.
///
/// Assets with zero supplied balance are omitted. Useful for front-ends
/// that need to display "how much each collateral is backing" or to detect
/// under-utilised capacity.
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

/// Deposit `amount` of an asset for the `user`.
///
/// Updates user supply and protocol total supply.
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

    let total = checked_sub_total(load_total_supply(env, &key), amount)?;
    save_total_supply(env, &key, total);

    Ok(pos)
}

/// Borrow `amount` of an asset for `user`.
///
/// Checks that the asset allows borrowing and that the user has sufficient
/// collateral after the borrow.
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

    // Health check: borrow_capacity must still cover total debt.
    let summary = get_user_position_summary(env, &user)?;
    if summary.is_healthy == 0 {
        // Roll back.
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

    let total = checked_sub_total(load_total_debt(env, &key), repay)?;
    save_total_debt(env, &key, total);

    Ok(pos)
}

// ---------------------------------------------------------------------------
// Regression tests: issue #1714 — aggregate total underflow
// ---------------------------------------------------------------------------
//
// `cross_asset_withdraw`/`cross_asset_repay` used to subtract from the
// protocol-wide `TotalSupply`/`TotalDebt` counters with the plain `-`
// operator. If those aggregates ever drift below an individual user's
// withdrawal/repay amount (e.g. due to desynced bookkeeping elsewhere), the
// subtraction would go negative and, depending on build overflow-check
// settings, could abort the transaction as an unrecoverable panic instead of
// a typed error. These tests force that desync directly (bypassing the
// public API, which cannot itself produce it under normal use) and assert a
// clean `CrossAssetError::Overflow` is returned instead.
#[cfg(test)]
mod total_underflow_regression_test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn with_contract<F, T>(env: &Env, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let contract_id = env.register(NoOpContract {}, ());
        env.as_contract(&contract_id, f)
    }

    /// `cross_asset_withdraw` must return `Overflow`, not panic, when the
    /// protocol-wide total is smaller than the user's own supplied balance
    /// (a desync that should never happen but must fail safely if it does).
    #[test]
    fn withdraw_returns_overflow_when_total_supply_desynced_below_amount() {
        let env = Env::default();
        with_contract(&env, || {
            let user = Address::generate(&env);
            let key = AssetKey::Native;

            // User appears to have 100 supplied, but the aggregate total was
            // (incorrectly) only ever bumped to 50 — an inconsistent state.
            save_user_supply(&env, &key, &user, 100);
            save_total_supply(&env, &key, 50);

            let result = cross_asset_withdraw(&env, user.clone(), None, 80);
            assert_eq!(result, Err(CrossAssetError::Overflow));

            // State must be left untouched by the failed call's total write —
            // the per-user balance was already saved before the total check,
            // matching pre-existing behaviour for this function.
            assert_eq!(load_total_supply(&env, &key), 50);
        });
    }

    /// `cross_asset_repay` must return `Overflow`, not panic or silently
    /// clamp to zero, when the protocol-wide total debt is smaller than the
    /// amount being repaid.
    #[test]
    fn repay_returns_overflow_when_total_debt_desynced_below_repay() {
        let env = Env::default();
        with_contract(&env, || {
            let user = Address::generate(&env);
            let key = AssetKey::Native;

            // User appears to owe 100, but total debt was only ever bumped
            // to 50 — an inconsistent state.
            save_user_debt(&env, &key, &user, 100);
            save_total_debt(&env, &key, 50);

            let result = cross_asset_repay(&env, user.clone(), None, 80);
            assert_eq!(result, Err(CrossAssetError::Overflow));
        });
    }

    /// Normal (synced) withdraw is unaffected by the fix: the total is
    /// decremented exactly as before.
    #[test]
    fn withdraw_normal_path_unaffected() {
        let env = Env::default();
        with_contract(&env, || {
            let user = Address::generate(&env);
            let key = AssetKey::Native;

            save_user_supply(&env, &key, &user, 100);
            save_total_supply(&env, &key, 100);

            let pos = cross_asset_withdraw(&env, user.clone(), None, 40).unwrap();
            assert_eq!(pos.supplied, 60);
            assert_eq!(load_total_supply(&env, &key), 60);
        });
    }

    /// Normal (synced) repay is unaffected by the fix, including the exact
    /// case that used to rely on `.max(0)`: total debt reaching exactly zero
    /// still succeeds without error.
    #[test]
    fn repay_normal_path_reaching_exact_zero_still_succeeds() {
        let env = Env::default();
        with_contract(&env, || {
            let user = Address::generate(&env);
            let key = AssetKey::Native;

            save_user_debt(&env, &key, &user, 100);
            save_total_debt(&env, &key, 80);

            // repay = min(80, 100) = 80; total = 80 - 80 = 0, no error.
            let pos = cross_asset_repay(&env, user.clone(), None, 80).unwrap();
            assert_eq!(pos.borrowed, 20);
            assert_eq!(load_total_debt(&env, &key), 0);
        });
    }
}
