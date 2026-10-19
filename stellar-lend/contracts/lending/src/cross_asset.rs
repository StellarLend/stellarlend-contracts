use soroban_sdk::{Address, Env, Map, Vec};
use stellar_lend_common::pow10_checked;

use crate::debt::{DebtPosition, DEFAULT_APR_BPS};
use crate::{
    check_emergency_status, check_pause_status, AssetParams, AssetParamsV2, DataKey, LendingError,
    PriceRecord, ProtocolAction, DEFAULT_ASSET_DECIMALS, DEFAULT_ORACLE_MAX_AGE_SECS,
};

/// Fixed-point divisor defining the oracle price scale, and the conversion of a
/// **7-decimal** asset's raw amount into the protocol's internal USD value scale.
///
/// The Lending oracle feeds every asset price on a uniform **7-decimal** scale
/// (e.g. `10_000_000` = \$1.00). An asset's raw token amount carries its own
/// `decimals` fractional digits, so the USD value of `amount` tokens priced at
/// `price` is:
///
/// ```text
/// value = amount × price / 10^decimals      (7-decimal USD scale)
/// ```
///
/// `PRICE_DIVISOR` == `10^7` is that conversion for a 7-decimal asset: it is the
/// historical, hard-coded divisor, and it equals `10^decimals` for
/// [`DEFAULT_ASSET_DECIMALS`]. Assets with a different `decimals` are normalised
/// per-leg by [`value_in_base`] / [`value_in_base_ceil`] using the asset's
/// configured decimals, so amounts of heterogeneous assets sum correctly.
///
/// # Rounding
///
/// Collateral values round **down** (floor) and debt values round **up**
/// (ceiling), both protocol-favouring. See [`value_in_base`] for details.
///
/// See [`docs/cross_asset.md`] for a worked example.
pub const PRICE_DIVISOR: i128 = 10_000_000;

/// Sentinel health factor returned when a user has zero outstanding debt.
///
/// Value: `100_000_000` (10 000× the [`HEALTH_FACTOR_SCALE`] baseline of 10 000).
/// Callers should treat any value ≥ this constant as "position is fully healthy"
/// and skip liquidation checks.
pub const HEALTH_FACTOR_NO_DEBT: i128 = 100_000_000;

/// Baseline scale for health-factor comparisons.
///
/// A health factor ≥ `HEALTH_FACTOR_SCALE` (i.e., ≥ 1.0 in human terms) means the
/// position is sufficiently collateralised and cannot be liquidated.
pub const HEALTH_FACTOR_SCALE: i128 = 10_000;

/// Load the collateral balance for a single `(user, asset)` pair.
///
/// Issues **1 persistent-storage read** per call.
///
/// # Returns
/// The stored collateral amount, or `0` if no entry exists.
pub fn load_collateral_asset(env: &Env, user: &Address, asset: &Address) -> i128 {
    let key = DataKey::CollateralAsset(user.clone(), asset.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Persist the collateral balance for a single `(user, asset)` pair.
///
/// Issues **1 persistent-storage write** per call.
pub fn save_collateral_asset(env: &Env, user: &Address, asset: &Address, amount: i128) {
    let key = DataKey::CollateralAsset(user.clone(), asset.clone());
    env.storage().persistent().set(&key, &amount);
}

/// Load the debt position for a single `(user, asset)` pair.
///
/// Issues **1 persistent-storage read** per call.
///
/// # Returns
/// The stored [`DebtPosition`], or a zero-principal position timestamped at
/// the current ledger if no entry exists.
pub fn load_debt_asset(env: &Env, user: &Address, asset: &Address) -> DebtPosition {
    let key = DataKey::DebtAsset(user.clone(), asset.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(DebtPosition {
            principal: 0,
            borrow_index_snapshot: 0,
            last_update: env.ledger().timestamp(),
        })
}

/// Persist the debt position for a single `(user, asset)` pair.
///
/// Issues **1 persistent-storage write** per call.
pub fn save_debt_asset(env: &Env, user: &Address, asset: &Address, position: &DebtPosition) {
    let key = DataKey::DebtAsset(user.clone(), asset.clone());
    env.storage().persistent().set(&key, position);
}

/// Load the risk parameters configured for `asset`.
///
/// Reads the legacy [`AssetParams`] entry first (1 instance read); when that is
/// absent it falls back to the [`AssetParamsV2`] entry written by
/// `set_asset_params_v2` (1 further instance read), exposing the shared risk
/// fields through the same [`AssetParams`] view.
///
/// # Returns
/// `Some(AssetParams)` if the asset has been configured via either
/// `set_asset_params` or `set_asset_params_v2`, `None` otherwise.
pub fn load_asset_params(env: &Env, asset: &Address) -> Option<AssetParams> {
    let legacy_key = DataKey::AssetParams(asset.clone());
    if let Some(params) = env.storage().instance().get(&legacy_key) {
        return Some(params);
    }
    let v2_key = DataKey::AssetParamsV2(asset.clone());
    env.storage()
        .instance()
        .get::<DataKey, AssetParamsV2>(&v2_key)
        .map(|v2| AssetParams {
            ltv_bps: v2.ltv_bps,
            liquidation_threshold_bps: v2.liquidation_threshold_bps,
            debt_ceiling: v2.debt_ceiling,
            borrow_cap: v2.borrow_cap,
            supply_cap: v2.supply_cap,
        })
}

/// Load the token decimals configured for `asset`.
///
/// Issues **1 instance-storage read** per call.
///
/// # Read-time fallback
/// Assets configured with the legacy [`AssetParams`] entry (which has no
/// `decimals` field) are treated as [`DEFAULT_ASSET_DECIMALS`] (7), preserving
/// their historical valuation behaviour exactly. Assets configured via
/// `set_asset_params_v2` return their configured value.
pub fn load_asset_decimals(env: &Env, asset: &Address) -> u32 {
    let key = DataKey::AssetParamsV2(asset.clone());
    env.storage()
        .instance()
        .get::<DataKey, AssetParamsV2>(&key)
        .map(|v2| v2.decimals)
        .unwrap_or(DEFAULT_ASSET_DECIMALS)
}

/// Convert `amount` raw token units of an asset with `decimals` fractional
/// digits into the protocol's internal USD value scale (7 decimals, i.e.
/// [`PRICE_DIVISOR`] units per \$1.00).
///
/// # Formula
///
/// ```text
/// value = amount × price / 10^decimals      (floor)
/// ```
///
/// `price` is the oracle price on the protocol's uniform 7-decimal scale
/// (`10_000_000` = \$1.00). For `decimals == 7` this reduces to the historical
/// `amount × price / PRICE_DIVISOR`.
///
/// # Rounding
/// Rounds **down** (floor), which is conservative for collateral values — it
/// can only understate, never overstate, borrowing power. Debt values must use
/// [`value_in_base_ceil`] instead so debt is never understated.
///
/// # Errors
/// - [`LendingError::PriceFeedNotFound`] if `price <= 0` (non-positive prices are
///   never valid; mirrors [`get_price_for_asset`]).
/// - [`LendingError::InvalidAmount`] if `amount < 0`.
/// - [`LendingError::Overflow`] if `10^decimals` overflows `i128` (possible only
///   for `decimals > 38`), if `amount × price` overflows, or if the division
///   overflows. Never wraps.
pub fn value_in_base(amount: i128, price: i128, decimals: u32) -> Result<i128, LendingError> {
    value_in_base_rounded(amount, price, decimals, false)
}

/// Same as [`value_in_base`] but rounds **up** (ceiling), which is conservative
/// for debt values.
///
/// # Errors
/// Same as [`value_in_base`].
pub fn value_in_base_ceil(amount: i128, price: i128, decimals: u32) -> Result<i128, LendingError> {
    value_in_base_rounded(amount, price, decimals, true)
}

fn value_in_base_rounded(
    amount: i128,
    price: i128,
    decimals: u32,
    round_up: bool,
) -> Result<i128, LendingError> {
    if price <= 0 {
        return Err(LendingError::PriceFeedNotFound);
    }
    if amount < 0 {
        return Err(LendingError::InvalidAmount);
    }
    let denom = pow10_checked(decimals).ok_or(LendingError::Overflow)?;
    let product = amount.checked_mul(price).ok_or(LendingError::Overflow)?;
    let value = product.checked_div(denom).ok_or(LendingError::Overflow)?;
    if round_up && product % denom != 0 {
        return value.checked_add(1).ok_or(LendingError::Overflow);
    }
    Ok(value)
}

/// Fetch the most recent oracle price record for `asset`.
///
/// Issues **1 persistent-storage read** per call.
///
/// The freshness policy is fail-closed: a missing or non-positive price is
/// never replaced with an older recorded price or a zero value.
///
/// # Errors
/// Returns [`LendingError::PriceFeedNotFound`] if no price has been stored for
/// this asset, or if the stored price is non-positive.
///
/// Returns [`LendingError::StaleOracleTimestamp`] if the price is older than
/// `DEFAULT_ORACLE_MAX_AGE_SECS` or carries a future timestamp.
pub fn get_price_for_asset(env: &Env, asset: &Address) -> Result<PriceRecord, LendingError> {
    let record: PriceRecord = env
        .storage()
        .persistent()
        .get(&DataKey::OraclePrice(asset.clone()))
        .ok_or(LendingError::PriceFeedNotFound)?;
    if record.price <= 0 {
        // A non-positive price is not a valid feed; treat it as missing so a
        // zero-priced debt leg cannot bypass health-factor checks.
        return Err(LendingError::PriceFeedNotFound);
    }
    let now = env.ledger().timestamp();
    if record.timestamp > now || now.saturating_sub(record.timestamp) > DEFAULT_ORACLE_MAX_AGE_SECS
    {
        return Err(LendingError::StaleOracleTimestamp);
    }
    // A non-positive price is never valid; treat as missing feed.
    if record.price <= 0 {
        return Err(LendingError::PriceFeedNotFound);
    }
    Ok(record)
}

pub(crate) fn add_to_user_collateral_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserCollateralAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if !list.contains(asset) {
        list.push_back(asset.clone());
        env.storage().persistent().set(&key, &list);
    }
}

pub(crate) fn remove_from_user_collateral_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserCollateralAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if let Some(pos) = list.first_index_of(asset) {
        list.remove(pos);
        env.storage().persistent().set(&key, &list);
    }
}

pub(crate) fn add_to_user_debt_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserDebtAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if !list.contains(asset) {
        list.push_back(asset.clone());
        env.storage().persistent().set(&key, &list);
    }
}

pub(crate) fn remove_from_user_debt_list(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::UserDebtAssets(user.clone());
    let mut list: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env));
    if let Some(pos) = list.first_index_of(asset) {
        list.remove(pos);
        env.storage().persistent().set(&key, &list);
    }
}

/// Return the ordered list of collateral asset addresses for `user`.
///
/// Issues **1 persistent-storage read** per call.
fn get_user_collateral_assets(env: &Env, user: &Address) -> Vec<Address> {
    let key = DataKey::UserCollateralAssets(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env))
}

/// Return the ordered list of debt asset addresses for `user`.
///
/// Issues **1 persistent-storage read** per call.
fn get_user_debt_assets(env: &Env, user: &Address) -> Vec<Address> {
    let key = DataKey::UserDebtAssets(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(Vec::new(env))
}

pub(crate) fn extend_collateral_asset_ttl(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::CollateralAsset(user.clone(), asset.clone());
    let extend_to = env.storage().max_ttl().min(crate::PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

pub(crate) fn extend_debt_asset_ttl(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::DebtAsset(user.clone(), asset.clone());
    let extend_to = env.storage().max_ttl().min(crate::PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

/// Compute the aggregate health factor across all collateral and debt assets.
///
/// # Read-budget contract
///
/// For a user with **N** collateral assets and **M** debt assets, this function
/// issues the following storage reads:
///
/// | Operation | Reads |
/// |-----------|-------|
/// | Collateral-asset list (`UserCollateralAssets`) | 1 |
/// | Debt-asset list (`UserDebtAssets`) | 1 |
/// | Per collateral asset: `AssetParams`/`AssetParamsV2` + decimals + `OraclePrice` + `CollateralAsset` | up to 4 × N |
/// | Per debt asset: `OraclePrice` (if not cached) + `DebtAsset` | 2 × M (worst case) |
/// | **Total** | **2 + 4N + 2M - C** (where C is the number of overlapping assets) |
///
/// The `cross_asset_health_perf_test` module asserts this budget for
/// representative values of N and M and must be kept in sync with this comment
/// whenever the implementation changes.
///
/// # Price Cache
///
/// This function uses a local `Map<Address, PriceRecord>` to cache prices fetched
/// during the cross-asset evaluation. If an asset appears in both the collateral and
/// debt lists, its price is fetched from persistent storage only once, saving a read.
///
/// # Redundant-read note
///
/// `compute_aggregate_health_factor` fetches the two asset lists independently
/// of `get_cross_position_value` and `get_cross_debt_value`.  When all three
/// are called together (via `get_cross_position_summary`) the lists are read
/// **three times** instead of once.  This is linear O(N+M), not quadratic, but
/// carries a 3× constant.  A future single-pass optimisation could merge the
/// loops and reduce the constant to ≈1× — tracked as a separate issue.
///
/// # Formula
///
/// Each leg is normalised to the internal 7-decimal USD scale via
/// [`value_in_base`] (collateral, floor) and [`value_in_base_ceil`] (debt,
/// ceiling) using the asset's configured decimals:
///
/// ```text
/// collateral_value_i  = floor(amount_i × price_i / 10^decimals_i)
/// weighted_collateral = Σ  collateral_value_i × liquidation_threshold_bps_i
/// debt_value_j        = ceil(effective_debt_j × price_j / 10^decimals_j)
/// total_debt_value    = Σ  debt_value_j
/// health_factor       = weighted_collateral / total_debt_value
/// ```
///
/// The per-leg normalisation is a pure ratio-preserving scale when every asset
/// shares the same decimals, so 7-decimal positions keep their previous value.
///
/// # Returns
/// - `Ok(HEALTH_FACTOR_NO_DEBT)` when the user has no debt.
/// - `Ok(health_factor)` — scaled integer; values ≥ `HEALTH_FACTOR_SCALE` are healthy.
/// - `Err(LendingError)` on missing asset params, missing price feed, or overflow.
///
/// # See also
/// - [`cross_asset.md`] — full aggregation pipeline and a worked example.
/// - [`CROSS_ASSET_HEALTH_PERF.md`] — read-budget rationale and edge-case notes.
pub fn compute_aggregate_health_factor(env: &Env, user: &Address) -> Result<i128, LendingError> {
    // Read 1: collateral-asset list
    let collateral_assets = get_user_collateral_assets(env, user);
    // Read 2: debt-asset list
    let debt_assets = get_user_debt_assets(env, user);

    if debt_assets.is_empty() {
        return Ok(HEALTH_FACTOR_NO_DEBT);
    }

    let mut weighted_collateral: i128 = 0;
    let mut total_debt_value: i128 = 0;

    // Cache to prevent duplicate price fetches for assets on both sides.
    let mut price_cache: Map<Address, PriceRecord> = Map::new(env);

    // Reads 3 .. 2 + 4N: per collateral asset — params + decimals (instance),
    // price, balance.
    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        // Instance reads: risk params + token decimals (1 per asset each)
        let params = load_asset_params(env, &asset).ok_or(LendingError::AssetNotConfigured)?;
        let decimals = load_asset_decimals(env, &asset);
        // Persistent read: OraclePrice (1 per asset, cached locally)
        let price_record = match price_cache.get(asset.clone()) {
            Some(cached) => cached,
            None => {
                let fetched = get_price_for_asset(env, &asset)?;
                price_cache.set(asset.clone(), fetched.clone());
                fetched
            }
        };
        // Persistent read: CollateralAsset balance (1 per asset)
        let amount = load_collateral_asset(env, user, &asset);
        if amount == 0 {
            continue;
        }
        // Normalise to the internal 7-decimal USD scale; collateral rounds down.
        let value = value_in_base(amount, price_record.price, decimals)?;
        let weighted = value
            .checked_mul(params.liquidation_threshold_bps)
            .ok_or(LendingError::Overflow)?;
        weighted_collateral = weighted_collateral
            .checked_add(weighted)
            .ok_or(LendingError::Overflow)?;
    }

    // Reads 3 + 4N .. 2 + 4N + 2M: per debt asset — price, debt position
    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        // Persistent read: OraclePrice (1 per asset, cached locally)
        let price_record = match price_cache.get(asset.clone()) {
            Some(cached) => cached,
            None => {
                let fetched = get_price_for_asset(env, &asset)?;
                price_cache.set(asset.clone(), fetched.clone());
                fetched
            }
        };
        // Persistent read: DebtAsset position (1 per asset)
        let position = load_debt_asset(env, user, &asset);
        let debt =
            crate::debt::effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
                .map_err(|_| LendingError::Overflow)?;
        if debt == 0 {
            continue;
        }
        // Normalise to the internal 7-decimal USD scale; debt rounds up.
        let decimals = load_asset_decimals(env, &asset);
        let value = value_in_base_ceil(debt, price_record.price, decimals)?;
        total_debt_value = total_debt_value
            .checked_add(value)
            .ok_or(LendingError::Overflow)?;
    }

    if total_debt_value == 0 {
        return Ok(HEALTH_FACTOR_NO_DEBT);
    }

    let health_factor = weighted_collateral
        .checked_div(total_debt_value)
        .ok_or(LendingError::Overflow)?;

    Ok(health_factor)
}

/// Return the total USD value of a user's cross-asset collateral positions.
///
/// # Read-budget contract
///
/// Issues `1 + 3N` storage reads for a user with **N** collateral
/// assets:
/// - 1 read for the collateral-asset list.
/// - N instance reads for token decimals (`AssetParamsV2`).
/// - N reads for oracle prices.
/// - N reads for collateral balances.
///
/// # Returns
/// Total collateral value in the internal 7-decimal USD scale, summing each leg
/// through [`value_in_base`] (which normalises by the asset's decimals and
/// rounds down), or `Err(LendingError)` on missing price feed or overflow.
pub fn get_cross_position_value(env: &Env, user: &Address) -> Result<i128, LendingError> {
    let collateral_assets = get_user_collateral_assets(env, user);
    let mut total_collateral = 0i128;

    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        let decimals = load_asset_decimals(env, &asset);
        let price_record = get_price_for_asset(env, &asset)?;
        let amount = load_collateral_asset(env, user, &asset);
        if amount == 0 {
            continue;
        }
        // Collateral rounds down (protocol-favouring).
        let value = value_in_base(amount, price_record.price, decimals)?;
        total_collateral = total_collateral
            .checked_add(value)
            .ok_or(LendingError::Overflow)?;
    }

    Ok(total_collateral)
}

/// Return the total USD value of a user's cross-asset debt positions.
///
/// # Read-budget contract
///
/// Issues `1 + 3M` storage reads for a user with **M** debt assets:
/// - 1 read for the debt-asset list.
/// - M instance reads for token decimals (`AssetParamsV2`).
/// - M reads for oracle prices.
/// - M reads for debt positions.
///
/// # Returns
/// Total debt value in the internal 7-decimal USD scale, summing each leg
/// through [`value_in_base_ceil`] (which normalises by the asset's decimals and
/// rounds up, so debt is never understated), or `Err(LendingError)` on missing
/// price feed or overflow.
pub fn get_cross_debt_value(env: &Env, user: &Address) -> Result<i128, LendingError> {
    let debt_assets = get_user_debt_assets(env, user);
    let mut total_debt_value = 0i128;

    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        let decimals = load_asset_decimals(env, &asset);
        let price_record = get_price_for_asset(env, &asset)?;
        let position = load_debt_asset(env, user, &asset);
        let debt =
            crate::debt::effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
                .map_err(|_| LendingError::Overflow)?;
        if debt == 0 {
            continue;
        }
        // Debt rounds up (protocol-favouring).
        let value = value_in_base_ceil(debt, price_record.price, decimals)?;
        total_debt_value = total_debt_value
            .checked_add(value)
            .ok_or(LendingError::Overflow)?;
    }

    Ok(total_debt_value)
}

/// Validate that `asset` has been configured and return its [`AssetParams`].
///
/// Issues **1 instance-storage read**.
///
/// # Errors
/// Returns [`LendingError::AssetNotConfigured`] if no params entry exists.
pub fn validate_asset_params_configured(
    env: &Env,
    asset: &Address,
) -> Result<AssetParams, LendingError> {
    load_asset_params(env, asset).ok_or(LendingError::AssetNotConfigured)
}

/// Persist risk parameters for `asset`.
///
/// Issues **1 instance-storage write**.
pub fn set_asset_params_internal(env: &Env, asset: &Address, params: &AssetParams) {
    let key = DataKey::AssetParams(asset.clone());
    env.storage().instance().set(&key, params);
}

/// Persist extended risk parameters (including token `decimals`) for `asset`.
///
/// Issues **1 instance-storage write**.
pub fn set_asset_params_v2_internal(env: &Env, asset: &Address, params: &AssetParamsV2) {
    let key = DataKey::AssetParamsV2(asset.clone());
    env.storage().instance().set(&key, params);
}

/// Deposit `amount` of `asset` as collateral for `user`.
///
/// Validates protocol pause state, checks that `asset` is configured, requires
/// the user's authorisation, updates the collateral balance, registers the
/// asset in the user's collateral list, and extends the entry's TTL.
///
/// # Errors
/// - [`LendingError::InvalidAmount`] if `amount ≤ 0`.
/// - [`LendingError::AssetNotConfigured`] if `asset` has no params entry.
pub fn deposit_collateral_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Deposit);
    check_emergency_status(env, ProtocolAction::Deposit);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    user.require_auth();

    let current = load_collateral_asset(env, user, asset);
    let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
    save_collateral_asset(env, user, asset, new_balance);
    add_to_user_collateral_list(env, user, asset);
    extend_collateral_asset_ttl(env, user, asset);

    Ok(new_balance)
}

/// Withdraw `amount` of collateral `asset` for `user`.
///
/// Checks pause state, validates params, requires authorisation, reduces the
/// collateral balance, removes the asset from the list when balance reaches
/// zero, and verifies the resulting health factor remains ≥ `HEALTH_FACTOR_SCALE`.
/// Rolls back the state change if the health-factor check fails.
///
/// # Errors
/// - [`LendingError::InvalidAmount`] if `amount ≤ 0` or exceeds current balance.
/// - [`LendingError::AssetNotConfigured`] if `asset` has no params entry.
/// - [`LendingError::HealthFactorTooLow`] if withdrawal would under-collateralise the position.
pub fn withdraw_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Withdraw);
    check_emergency_status(env, ProtocolAction::Withdraw);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    user.require_auth();

    let current = load_collateral_asset(env, user, asset);
    if amount > current {
        return Err(LendingError::InvalidAmount);
    }

    let new_balance = current.checked_sub(amount).ok_or(LendingError::Overflow)?;
    save_collateral_asset(env, user, asset, new_balance);

    if new_balance == 0 {
        remove_from_user_collateral_list(env, user, asset);
    }

    let hf = match compute_aggregate_health_factor(env, user) {
        Ok(hf) => hf,
        Err(err) => {
            // Roll back the provisional collateral write if the health check
            // itself fails (for example, due to a stale or missing oracle
            // price). Without this, a failed withdrawal would still reduce the
            // user's collateral balance.
            save_collateral_asset(env, user, asset, current);
            if current > 0 {
                add_to_user_collateral_list(env, user, asset);
            }
            return Err(err);
        }
    };
    if hf < HEALTH_FACTOR_SCALE {
        save_collateral_asset(env, user, asset, current);
        if current > 0 {
            add_to_user_collateral_list(env, user, asset);
        }
        return Err(LendingError::HealthFactorTooLow);
    }

    extend_collateral_asset_ttl(env, user, asset);

    Ok(new_balance)
}

/// Borrow `amount` of `asset` for `user`.
///
/// Checks pause state, validates params, enforces the minimum-borrow floor,
/// **fail-closes on partial oracle staleness** (every collateral and debt leg
/// already on the position, plus the asset being borrowed, must have a fresh
/// price — see [`ensure_position_prices_fresh`]), accrues interest on any
/// existing debt position, creates or updates the debt entry, verifies the
/// health factor post-borrow, enforces the per-asset and protocol debt
/// ceilings, and extends the debt entry's TTL.
///
/// # Partial-staleness policy
///
/// A multi-asset position is valued by aggregating every collateral and debt
/// leg. If *any* of those legs carries a stale oracle price while others are
/// fresh, the true health of the position is unknown. This function therefore
/// **fails closed** with [`LendingError::StaleOracleTimestamp`] whenever any
/// relevant leg is stale — not only when the borrowed asset itself is stale.
///
/// Repay is intentionally *not* gated by this check (see
/// `PARTIAL_STALENESS_POLICY.md`): reducing risk must remain allowed.
///
/// # Errors
/// - [`LendingError::InvalidAmount`] if `amount ≤ 0`.
/// - [`LendingError::AssetNotConfigured`] if `asset` has no params entry.
/// - [`LendingError::BelowMinimumBorrow`] if `amount < min_borrow`.
/// - [`LendingError::StaleOracleTimestamp`] if any collateral, existing debt,
///   or the borrowed asset has a price older than `DEFAULT_ORACLE_MAX_AGE_SECS`.
/// - [`LendingError::PriceFeedNotFound`] if a required oracle price is missing.
/// - [`LendingError::HealthFactorTooLow`] if borrow would under-collateralise the position.
/// - [`LendingError::DebtCeilingExceeded`] if borrow would exceed the per-asset ceiling.
/// - [`LendingError::BorrowCapExceeded`] if borrow would exceed the per-asset borrow cap.
/// - [`LendingError::Overflow`] on arithmetic overflow.
pub fn borrow_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Borrow);
    check_emergency_status(env, ProtocolAction::Borrow);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    let params = validate_asset_params_configured(env, asset)?;

    let min_borrow = crate::LendingContract::get_min_borrow(env.clone());
    if amount < min_borrow {
        return Err(LendingError::BelowMinimumBorrow);
    }

    user.require_auth();

    // Fail closed on partial oracle staleness: reject the borrow if *any*
    // collateral or debt asset already on the user's cross-asset position has
    // a stale price, or if the asset being borrowed itself has a stale price.
    // This hardens the borrow path so a single stale leg cannot enable an
    // under-collateralised borrow, independent of the health-factor computation.
    // Repay is intentionally not gated here (reducing risk must always
    // succeed). See PARTIAL_STALENESS_POLICY.md.
    ensure_position_prices_fresh(env, user, asset)?;

    let now = env.ledger().timestamp();

    let rate = crate::current_borrow_rate(env);
    let position = load_debt_asset(env, user, asset);
    let prev_principal = position.principal;
    let settled_position = crate::settle_and_accrue_insurance(env, &position, now, rate)?;
    let updated = crate::debt::borrow_amount(settled_position, now, amount, rate)
        .map_err(|_| LendingError::Overflow)?;
    save_debt_asset(env, user, asset, &updated);
    add_to_user_debt_list(env, user, asset);

    let hf = match compute_aggregate_health_factor(env, user) {
        Ok(hf) => hf,
        Err(err) => {
            // Roll back the provisional debt write if the health check itself
            // fails. The borrow is rejected, so the user's debt position must
            // be restored to its original state.
            save_debt_asset(env, user, asset, &position);
            if prev_principal == 0 {
                remove_from_user_debt_list(env, user, asset);
            }
            return Err(err);
        }
    };

    if hf < HEALTH_FACTOR_SCALE {
        save_debt_asset(
            env,
            user,
            asset,
            &DebtPosition {
                principal: prev_principal,
                borrow_index_snapshot: 0,
                last_update: now,
            },
        );
        if prev_principal == 0 {
            env.storage()
                .persistent()
                .remove(&DataKey::DebtAsset(user.clone(), asset.clone()));
            remove_from_user_debt_list(env, user, asset);
        } else {
            save_debt_asset(env, user, asset, &position);
        }
        return Err(LendingError::HealthFactorTooLow);
    }

    let total_debt_for_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(asset.clone()))
        .unwrap_or(0);
    let delta = updated
        .principal
        .checked_sub(prev_principal)
        .ok_or(LendingError::Overflow)?;
    let new_total_debt = total_debt_for_asset
        .checked_add(delta)
        .ok_or(LendingError::Overflow)?;
    if new_total_debt > params.debt_ceiling {
        save_debt_asset(
            env,
            user,
            asset,
            &DebtPosition {
                principal: prev_principal,
                borrow_index_snapshot: 0,
                last_update: now,
            },
        );
        if prev_principal == 0 {
            remove_from_user_debt_list(env, user, asset);
        }
        return Err(LendingError::DebtCeilingExceeded);
    }
    // Enforce optional per-asset borrow cap: 0 means uncapped.
    if params.borrow_cap != 0 && new_total_debt > params.borrow_cap {
        save_debt_asset(
            env,
            user,
            asset,
            &DebtPosition {
                principal: prev_principal,
                borrow_index_snapshot: 0,
                last_update: now,
            },
        );
        if prev_principal == 0 {
            remove_from_user_debt_list(env, user, asset);
        }
        return Err(LendingError::BorrowCapExceeded);
    }
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebtAsset(asset.clone()), &new_total_debt);

    let total_debt_protocol: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let new_total_protocol = total_debt_protocol
        .checked_add(delta)
        .ok_or(LendingError::Overflow)?;
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_protocol);

    extend_debt_asset_ttl(env, user, asset);

    Ok(updated.principal)
}

/// Reject (fail closed) a borrow when *any* collateral or debt asset on the
/// user's cross-asset position — or the asset being borrowed — carries a stale
/// oracle price.
///
/// # Rationale
///
/// Multi-asset health is an aggregate of every leg. A single stale collateral
/// or debt price makes the true health unknown; allowing the borrow would let
/// a partial-staleness gap enable under-collateralised debt. This helper scans
/// every existing position leg plus `borrow_asset` (which may not yet be on
/// the debt list on first borrow of that asset) via [`get_price_for_asset`],
/// which returns [`LendingError::StaleOracleTimestamp`] for an aged price
/// (`now > record.timestamp + DEFAULT_ORACLE_MAX_AGE_SECS`).
///
/// # Fail-open counterpart
///
/// Repay is intentionally *not* gated by this helper — reducing a position's
/// risk must always be permitted. See `PARTIAL_STALENESS_POLICY.md`.
///
/// # Errors
/// - [`LendingError::StaleOracleTimestamp`] if any scanned asset's price is stale.
/// - [`LendingError::PriceFeedNotFound`] if any scanned asset has no price record.
pub(crate) fn ensure_position_prices_fresh(
    env: &Env,
    user: &Address,
    borrow_asset: &Address,
) -> Result<(), LendingError> {
    let collateral_assets = get_user_collateral_assets(env, user);
    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        get_price_for_asset(env, &asset)?;
    }

    let debt_assets = get_user_debt_assets(env, user);
    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        get_price_for_asset(env, &asset)?;
    }

    // First borrow of this asset: it is not yet on the debt list, but it
    // contributes to post-borrow health and must be fresh.
    get_price_for_asset(env, borrow_asset)?;

    Ok(())
}

/// Require fresh prices for every leg of a cross-asset position when
/// performing a *liquidation*.  Unlike [`ensure_position_prices_fresh`], this
/// variant does **not** accept an extra "new-asset" argument — the liquidator
/// chooses specific `debt_asset` / `collateral_asset` legs that already exist
/// on the position, so no new leg is introduced.  Both chosen assets must
/// appear in the price scan because the seized-collateral conversion uses both
/// prices.
///
/// The policy is fail-closed for the same reason as borrow: the aggregate
/// health factor is derived from every leg.  A single stale leg makes the true
/// health unknown and the liquidation unsafe.
///
/// # Errors
/// - [`LendingError::StaleOracleTimestamp`] if any scanned asset's price is stale.
/// - [`LendingError::PriceFeedNotFound`] if any scanned asset has no price record.
pub(crate) fn ensure_position_prices_fresh_for_liquidation(
    env: &Env,
    user: &Address,
) -> Result<(), LendingError> {
    let collateral_assets = get_user_collateral_assets(env, user);
    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        get_price_for_asset(env, &asset)?;
    }
    let debt_assets = get_user_debt_assets(env, user);
    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        get_price_for_asset(env, &asset)?;
    }
    Ok(())
}

/// Repay `amount` of debt `asset` for `user`.
///
/// Checks pause state, validates params, requires authorisation, accrues
/// interest on the existing position, applies the repayment, removes the asset
/// from the user's debt list when the position reaches zero, and updates both
/// per-asset and protocol-level total-debt accumulators.
///
/// # Errors
/// - [`LendingError::InvalidAmount`] if `amount ≤ 0`.
/// - [`LendingError::AssetNotConfigured`] if `asset` has no params entry.
/// - [`LendingError::Overflow`] on arithmetic overflow.
pub fn repay_asset_internal(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    check_pause_status(env, ProtocolAction::Repay);
    check_emergency_status(env, ProtocolAction::Repay);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    user.require_auth();

    let now = env.ledger().timestamp();
    let rate = crate::current_borrow_rate(env);
    let position = load_debt_asset(env, user, asset);
    let prev_principal = position.principal;
    let settled_position = crate::settle_and_accrue_insurance(env, &position, now, rate)?;
    // Cross-asset repay silently clamps to the outstanding balance so callers
    // can safely pass an amount larger than the debt (see REPAY_SEMANTICS.md).
    // When the position is already zero, return early — nothing to repay.
    let clamped_amount = amount.min(settled_position.principal);
    if clamped_amount <= 0 {
        return Ok(settled_position.principal);
    }
    let updated = crate::debt::repay_amount(settled_position, now, clamped_amount, rate)
        .map_err(|_| LendingError::Overflow)?;
    save_debt_asset(env, user, asset, &updated);
    if updated.principal == 0 {
        remove_from_user_debt_list(env, user, asset);
    }

    let principal_delta = updated
        .principal
        .checked_sub(prev_principal)
        .ok_or(LendingError::Overflow)?;

    let total_debt_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(asset.clone()))
        .unwrap_or(0);
    let new_total_debt_asset = total_debt_asset
        .checked_add(principal_delta)
        .ok_or(LendingError::Overflow)?;
    if new_total_debt_asset < 0 {
        return Err(LendingError::Overflow);
    }
    env.storage().persistent().set(
        &DataKey::TotalDebtAsset(asset.clone()),
        &new_total_debt_asset,
    );

    let total_debt_protocol: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let new_total_protocol = total_debt_protocol
        .checked_add(principal_delta)
        .ok_or(LendingError::Overflow)?;
    if new_total_protocol < 0 {
        return Err(LendingError::Overflow);
    }
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_protocol);

    extend_debt_asset_ttl(env, user, asset);

    Ok(updated.principal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger as _};

    /// Register a contract to host the storage frame.
    ///
    /// The SDK rejects persistent-storage access outside a contract frame, so
    /// every helper below runs inside `env.as_contract(&id, ..)` on the id this
    /// returns.
    fn setup() -> (Env, Address) {
        let env = Env::default();
        let id = env.register(crate::LendingContract, ());
        (env, id)
    }

    fn set_price(env: &Env, id: &Address, asset: &Address, price: i128, timestamp: u64) {
        env.as_contract(id, || {
            env.storage().persistent().set(
                &DataKey::OraclePrice(asset.clone()),
                &PriceRecord { price, timestamp },
            );
        });
    }

    fn price_of(env: &Env, id: &Address, asset: &Address) -> Result<PriceRecord, LendingError> {
        env.as_contract(id, || get_price_for_asset(env, asset))
    }

    #[test]
    fn get_price_for_asset_success() {
        let (env, id) = setup();
        let asset = Address::generate(&env);
        let timestamp = 1_000_000u64;
        env.ledger().set_timestamp(timestamp);
        set_price(&env, &id, &asset, 10_000_000, timestamp);

        assert_eq!(
            price_of(&env, &id, &asset)
                .expect("fresh price is readable")
                .price,
            10_000_000
        );
    }

    #[test]
    fn get_price_for_asset_missing_feed_fails() {
        let (env, id) = setup();
        let asset = Address::generate(&env);
        let contract_id = env.register(crate::LendingContract, ());

        assert!(matches!(
            price_of(&env, &id, &asset),
            Err(LendingError::PriceFeedNotFound)
        ));
    }

    #[test]
    fn get_price_for_asset_stale_fails() {
        let (env, id) = setup();
        let asset = Address::generate(&env);
        let timestamp = 1_000_000u64;
        env.ledger()
            .set_timestamp(timestamp + DEFAULT_ORACLE_MAX_AGE_SECS + 1);
        set_price(&env, &id, &asset, 10_000_000, timestamp);

        assert!(matches!(
            price_of(&env, &id, &asset),
            Err(LendingError::StaleOracleTimestamp)
        ));
    }

    #[test]
    fn get_price_for_asset_boundary_is_fresh() {
        let (env, id) = setup();
        let asset = Address::generate(&env);
        let timestamp = 1_000_000u64;
        env.ledger()
            .set_timestamp(timestamp + DEFAULT_ORACLE_MAX_AGE_SECS);
        set_price(&env, &id, &asset, 10_000_000, timestamp);

        assert!(price_of(&env, &id, &asset).is_ok());
    }

    #[test]
    fn get_price_for_asset_future_timestamp_fails() {
        let (env, id) = setup();
        let asset = Address::generate(&env);
        env.ledger().set_timestamp(1_000_000);
        set_price(&env, &id, &asset, 10_000_000, 1_000_001);

        assert!(matches!(
            price_of(&env, &id, &asset),
            Err(LendingError::StaleOracleTimestamp)
        ));
    }

    #[test]
    fn get_price_for_asset_non_positive_price_fails() {
        let (env, id) = setup();
        let asset = Address::generate(&env);
        env.ledger().set_timestamp(1_000_000);
        set_price(&env, &id, &asset, 0, 1_000_000);

        assert!(matches!(
            price_of(&env, &id, &asset),
            Err(LendingError::PriceFeedNotFound)
        ));
    }
}
