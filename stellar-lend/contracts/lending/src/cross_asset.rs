use soroban_sdk::{Address, Env, Vec};

use crate::debt::{DebtPosition, DEFAULT_APR_BPS};
use crate::{
    check_emergency_status, check_pause_status, AssetParams, DataKey, LendingError, PriceRecord,
    ProtocolAction,
};

const PRICE_DIVISOR: i128 = 10_000_000;

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
/// Issues **1 instance-storage read** per call.
///
/// # Returns
/// `Some(AssetParams)` if the asset has been configured via `set_asset_params`,
/// `None` otherwise.
pub fn load_asset_params(env: &Env, asset: &Address) -> Option<AssetParams> {
    let key = DataKey::AssetParams(asset.clone());
    env.storage().instance().get(&key)
}

/// Fetch the most recent oracle price record for `asset`.
///
/// Issues **1 persistent-storage read** per call.
///
/// # Errors
/// Returns [`LendingError::PriceFeedNotFound`] if no price has been stored for
/// this asset.
pub fn get_price_for_asset(env: &Env, asset: &Address) -> Result<PriceRecord, LendingError> {
    env.storage()
        .persistent()
        .get(&DataKey::OraclePrice(asset.clone()))
        .ok_or(LendingError::PriceFeedNotFound)
}

fn add_to_user_collateral_list(env: &Env, user: &Address, asset: &Address) {
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

fn remove_from_user_collateral_list(env: &Env, user: &Address, asset: &Address) {
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

fn add_to_user_debt_list(env: &Env, user: &Address, asset: &Address) {
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

fn remove_from_user_debt_list(env: &Env, user: &Address, asset: &Address) {
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

fn extend_collateral_asset_ttl(env: &Env, user: &Address, asset: &Address) {
    let key = DataKey::CollateralAsset(user.clone(), asset.clone());
    let extend_to = env.storage().max_ttl().min(crate::PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

fn extend_debt_asset_ttl(env: &Env, user: &Address, asset: &Address) {
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
/// issues the following persistent-storage reads:
///
/// | Operation | Reads |
/// |-----------|-------|
/// | Collateral-asset list (`UserCollateralAssets`) | 1 |
/// | Debt-asset list (`UserDebtAssets`) | 1 |
/// | Per collateral asset: `AssetParams` (instance) + `OraclePrice` + `CollateralAsset` | 3 × N |
/// | Per debt asset: `OraclePrice` + `DebtAsset` | 2 × M |
/// | **Total** | **2 + 3N + 2M** |
///
/// The `cross_asset_health_perf_test` module asserts this budget for
/// representative values of N and M and must be kept in sync with this comment
/// whenever the implementation changes.
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
/// ```text
/// weighted_collateral = Σ  amount_i × price_i × liquidation_threshold_bps_i
/// total_debt_value    = Σ  effective_debt_j × price_j
/// health_factor       = weighted_collateral / total_debt_value
/// ```
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

    // Reads 3 .. 2 + 3N: per collateral asset — params (instance), price, balance
    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        // Instance read: AssetParams (1 per asset)
        let params = load_asset_params(env, &asset).ok_or(LendingError::AssetNotConfigured)?;
        // Persistent read: OraclePrice (1 per asset)
        let price_record = get_price_for_asset(env, &asset)?;
        // Persistent read: CollateralAsset balance (1 per asset)
        let amount = load_collateral_asset(env, user, &asset);
        if amount == 0 {
            continue;
        }
        let value = amount
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?;
        let weighted = value
            .checked_mul(params.liquidation_threshold_bps)
            .ok_or(LendingError::Overflow)?;
        weighted_collateral = weighted_collateral
            .checked_add(weighted)
            .ok_or(LendingError::Overflow)?;
    }

    // Reads 3 + 3N .. 2 + 3N + 2M: per debt asset — price, debt position
    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        // Persistent read: OraclePrice (1 per asset)
        let price_record = get_price_for_asset(env, &asset)?;
        // Persistent read: DebtAsset position (1 per asset)
        let position = load_debt_asset(env, user, &asset);
        let debt =
            crate::debt::effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
                .map_err(|_| LendingError::Overflow)?;
        if debt == 0 {
            continue;
        }
        let value = debt
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?;
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
/// Issues `1 + 2N` persistent-storage reads for a user with **N** collateral
/// assets:
/// - 1 read for the collateral-asset list.
/// - N reads for oracle prices.
/// - N reads for collateral balances.
///
/// # Returns
/// Total collateral value in protocol units (price × amount ÷ `PRICE_DIVISOR`),
/// or `Err(LendingError)` on missing price feed or overflow.
pub fn get_cross_position_value(env: &Env, user: &Address) -> Result<i128, LendingError> {
    let collateral_assets = get_user_collateral_assets(env, user);
    let mut total_collateral = 0i128;

    for i in 0..collateral_assets.len() {
        let asset = collateral_assets.get(i).unwrap();
        let price_record = get_price_for_asset(env, &asset)?;
        let amount = load_collateral_asset(env, user, &asset);
        if amount == 0 {
            continue;
        }
        let value = amount
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?
            .checked_div(PRICE_DIVISOR)
            .ok_or(LendingError::Overflow)?;
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
/// Issues `1 + 2M` persistent-storage reads for a user with **M** debt assets:
/// - 1 read for the debt-asset list.
/// - M reads for oracle prices.
/// - M reads for debt positions.
///
/// # Returns
/// Total debt value in protocol units, or `Err(LendingError)` on missing price
/// feed or overflow.
pub fn get_cross_debt_value(env: &Env, user: &Address) -> Result<i128, LendingError> {
    let debt_assets = get_user_debt_assets(env, user);
    let mut total_debt_value = 0i128;

    for i in 0..debt_assets.len() {
        let asset = debt_assets.get(i).unwrap();
        let price_record = get_price_for_asset(env, &asset)?;
        let position = load_debt_asset(env, user, &asset);
        let debt =
            crate::debt::effective_debt(&position, env.ledger().timestamp(), DEFAULT_APR_BPS)
                .map_err(|_| LendingError::Overflow)?;
        if debt == 0 {
            continue;
        }
        let value = debt
            .checked_mul(price_record.price)
            .ok_or(LendingError::Overflow)?
            .checked_div(PRICE_DIVISOR)
            .ok_or(LendingError::Overflow)?;
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

    let hf = compute_aggregate_health_factor(env, user)?;
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
/// accrues interest on any existing debt position, creates or updates the debt
/// entry, verifies the health factor post-borrow, enforces the per-asset and
/// protocol debt ceilings, and extends the debt entry's TTL.
///
/// # Errors
/// - [`LendingError::InvalidAmount`] if `amount ≤ 0`.
/// - [`LendingError::AssetNotConfigured`] if `asset` has no params entry.
/// - [`LendingError::BelowMinimumBorrow`] if `amount < min_borrow`.
/// - [`LendingError::HealthFactorTooLow`] if borrow would under-collateralise the position.
/// - [`LendingError::DebtCeilingExceeded`] if borrow would exceed the per-asset ceiling.
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

    let now = env.ledger().timestamp();

    let rate = crate::current_borrow_rate(env);
    let position = load_debt_asset(env, user, asset);
    let prev_principal = position.principal;
    let settled_position = crate::settle_and_accrue_insurance(env, &position, now, rate)?;
    let updated = crate::debt::borrow_amount(settled_position, now, amount, rate)
        .map_err(|_| LendingError::Overflow)?;
    save_debt_asset(env, user, asset, &updated);
    add_to_user_debt_list(env, user, asset);

    let hf = compute_aggregate_health_factor(env, user)?;

    if hf < HEALTH_FACTOR_SCALE {
        save_debt_asset(
            env,
            user,
            asset,
            &DebtPosition {
                principal: prev_principal,
                last_update: now,
            },
        );
        if prev_principal == 0 {
            remove_from_user_debt_list(env, user, asset);
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
                last_update: now,
            },
        );
        if prev_principal == 0 {
            remove_from_user_debt_list(env, user, asset);
        }
        return Err(LendingError::DebtCeilingExceeded);
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
    let updated = crate::debt::repay_amount(settled_position, now, amount, rate)
        .map_err(|_| LendingError::Overflow)?;
    save_debt_asset(env, user, asset, &updated);
    if updated.principal == 0 {
        remove_from_user_debt_list(env, user, asset);
    }

    let repaid = prev_principal.checked_sub(updated.principal).unwrap_or(0);

    let total_debt_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(asset.clone()))
        .unwrap_or(0);
    let new_total_debt_asset = total_debt_asset.saturating_sub(repaid);
    env.storage().persistent().set(
        &DataKey::TotalDebtAsset(asset.clone()),
        &new_total_debt_asset,
    );

    let total_debt_protocol: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let new_total_protocol = total_debt_protocol.saturating_sub(repaid);
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_protocol);

    extend_debt_asset_ttl(env, user, asset);

    Ok(updated.principal)
}
