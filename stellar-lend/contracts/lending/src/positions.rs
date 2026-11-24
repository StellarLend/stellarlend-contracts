//! Position valuation, health-factor and borrow helpers.

use super::*;

pub(crate) fn check_and_clear_unhealthy_timestamp(env: &Env, user: &Address) {
    let first_unhealthy_key = DataKey::FirstUnhealthyTimestamp(user.clone());
    if env.storage().persistent().has(&first_unhealthy_key) {
        let col_key = DataKey::Collateral(user.clone());
        let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        let position = load_debt(env, user);
        let debt = position.principal;

        // Mirror `liquidate`'s entry guard: consult the *governed* threshold rather
        // than the hardcoded default, and treat an arithmetic failure as "not
        // provably healthy" (fail-closed) so an overflow can never clear the
        // grace-period timestamp. This is the same invariant as the post-liquidation
        // check in `liquidate`; both decide the fate of
        // `DataKey::FirstUnhealthyTimestamp` and must agree.
        let hf = if debt > 0 {
            let collateral_value = match value_collateral(env, collateral) {
                Ok(value) => value,
                Err(_) => return,
            };
            let debt_value = match value_debt(env, debt) {
                Ok(value) => value,
                Err(_) => return,
            };
            match math::checked_mul_div_floor(
                collateral_value,
                LendingContract::get_liquidation_threshold_bps(env.clone()),
                debt_value,
            ) {
                Ok(hf) => hf,
                Err(_) => return,
            }
        } else {
            i128::MAX
        };

        if hf >= HEALTH_FACTOR_SCALE {
            env.storage().persistent().remove(&first_unhealthy_key);
        }
    }
}

#[cfg(test)]
pub(crate) fn load_rate_snapshot(env: &Env) -> debt::RateSnapshot {
    debt::load_rate_snapshot(env)
}

// Shared solvency predicate for `borrow` and `withdraw`.
// 
// Passes when the position has no effective debt at the current ledger time
// and borrow rate, or when `collateral_value * threshold_bps >=
// HEALTH_FACTOR_SCALE * debt_value`, using the governed liquidation
// threshold. In cross-asset mode collateral and debt are first converted to
// oracle-valued units; otherwise raw units are used. Arithmetic overflow
// fails closed with an error.
pub(crate) fn assert_position_healthy(
    env: &Env,
    collateral: i128,
    position: &DebtPosition,
) -> Result<(), LendingError> {
    let threshold = LendingContract::get_liquidation_threshold_bps(env.clone());
    assert_position_within_bps(env, collateral, position, threshold)
}

// Same predicate as [`assert_position_healthy`] but against an explicit
// collateral weight (`bps`): the liquidation threshold for withdrawals, the
// governed max LTV for new single-asset borrows.
pub(crate) fn assert_position_within_bps(
    env: &Env,
    collateral: i128,
    position: &DebtPosition,
    threshold: i128,
) -> Result<(), LendingError> {
    let now = env.ledger().timestamp();
    let rate = current_borrow_rate(env);
    let debt = effective_debt(position, now, rate).map_err(|_| LendingError::Overflow)?;

    if debt > 0 {
        let collateral_value = value_collateral(env, collateral)?;
        let debt_value = value_debt(env, debt)?;
        let weighted_collateral = collateral_value
            .checked_mul(threshold)
            .ok_or(LendingError::Overflow)?;
        let required_collateral = HEALTH_FACTOR_SCALE
            .checked_mul(debt_value)
            .ok_or(LendingError::Overflow)?;

        if weighted_collateral < required_collateral {
            return Err(LendingError::InsufficientCollateral);
        }
    }
    Ok(())
}

// Shared borrowing pipeline for [`LendingContract::borrow`] and
// [`LendingContract::borrow_against_collateral`].
//
// The helper performs the complete *validation and calculation* phase of a
// borrow and returns the post-borrow debt position together with the
// **principal delta** — the amount by which the stored principal grew.
//
// The principal delta is *not* simply the requested amount: it also contains
// the interest that was settled during this call
// (`settled_principal + amount - previous_principal`). The protocol
// `TotalDebt` and the isolation-mode debt tracker are both updated with this
// delta so that they stay aligned with the value written to the position.
//
// # Effect ordering
//
// The helper deliberately performs **no** write to `DataKey::Debt` or
// `DataKey::TotalDebt`, does not extend the debt TTL and emits no event.
// Callers apply those effects with [`commit_borrow`] only after every check
// has passed — including the isolation-mode ceiling check that
// `borrow_against_collateral` performs *after* this helper returns. A
// rejected borrow therefore leaves no partial debt, isolation accounting,
// TTL or event effect behind.
//
// # Errors
// - `InvalidAmount` — `amount <= 0`.
// - `BelowMinimumBorrow` — `amount` is below the configured minimum.
// - `PriceUnavailable` / `StaleOracleTimestamp` — the valuation price is
//   missing or stale.
// - `InsufficientCollateral` — the post-borrow position would be
//   undercollateralised.
// - `DebtCeilingExceeded` — protocol `TotalDebt` would exceed the ceiling.
// - `Overflow` — arithmetic overflow while computing the new position.
pub(crate) fn borrow_internal(
    env: &Env,
    user: &Address,
    amount: i128,
) -> Result<(DebtPosition, i128), LendingError> {
    // 1. Amount and minimum-borrow rules.
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let min_borrow = LendingContract::get_min_borrow(env.clone());
    if amount < min_borrow {
        return Err(LendingError::BelowMinimumBorrow);
    }

    // 2. Oracle freshness. A borrow must never be priced against a missing or
    //    stale oracle record.
    require_fresh_valuation_prices(env)?;

    // 3. Settle accrued interest (and accrue the insurance share) before the
    //    new principal is added, so that the delta reflects the true debt
    //    growth of the position.
    let now = env.ledger().timestamp();
    let rate = current_borrow_rate(env);
    let position = load_debt(env, user);
    let prev_principal = position.principal;
    let settled_position = settle_and_accrue_insurance(env, &position, now, rate)?;
    let updated = borrow_amount(settled_position, now, amount, rate).map_err(|e| match e {
        debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
        debt::DebtError::RepayAmountTooHigh => LendingError::RepayAmountTooHigh,
        debt::DebtError::Overflow => LendingError::Overflow,
        debt::DebtError::IndexInvariantViolated => LendingError::Overflow,
    })?;

    // 4. Principal delta = settled interest + newly borrowed amount.
    let delta = updated
        .principal
        .checked_sub(prev_principal)
        .ok_or(LendingError::Overflow)?;

    // 5. Solvency (collateral coverage) and the protocol-wide debt ceiling.
    let new_total_debt = total_debt_after(env, delta)?;
    assert_borrow_solvent(env, user, &updated, new_total_debt)?;

    Ok((updated, delta))
}

// Apply the persistent effects of a validated borrow.
//
// Writes the new protocol `TotalDebt`, stores the borrower's debt position,
// extends the TTL of the debt entry and emits the standard `BorrowEvent`.
//
// Callers must have completed **all** validation (including the
// isolation-mode ceiling check performed by `borrow_against_collateral`)
// before calling this helper; it performs no checks of its own.
pub(crate) fn commit_borrow(
    env: &Env,
    user: &Address,
    amount: i128,
    updated: &DebtPosition,
    delta: i128,
) -> Result<(), LendingError> {
    // Track protocol-level total debt.
    let new_total_debt = total_debt_after(env, delta)?;
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_debt);
    crate::ttl::bump_global(&env, &DataKey::TotalDebt);

    save_debt(env, user, updated);
    register_borrower(env, user);
    // Extend TTL to prevent archival of debt entry
    extend_debt_ttl(env, user);

    // Emit borrow event
    emit_borrow(env, user, amount, updated.principal);

    Ok(())
}

// Return the protocol-wide `TotalDebt` that results from adding `delta` to the
// currently recorded total.
pub(crate) fn total_debt_after(env: &Env, delta: i128) -> Result<i128, LendingError> {
    let total_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    total_debt.checked_add(delta).ok_or(LendingError::Overflow)
}

// Reject a prospective borrow that would leave the user undercollateralized or
// push protocol `TotalDebt` past the configured ceiling.
// 
// The governed maximum LTV (not the liquidation threshold) bounds new borrows,
// using effective debt after accrual, so a successful borrow can never be
// instantly liquidatable. In cross-asset mode, collateral and debt are first converted to
// oracle-valued units; otherwise legacy raw units are retained. Solvency is
// checked with the overflow-safe equivalent of health factor >= 1.0:
// `collateral_value * max_ltv_bps >= HEALTH_FACTOR_SCALE * debt_value`.
// 
// # Worked example
// With 100 collateral and a 75% maximum LTV, 75 debt is the boundary.
// A borrow to 76 debt fails because `100 * 7500 < 10_000 * 76`.
pub(crate) fn assert_borrow_solvent(
    env: &Env,
    user: &Address,
    updated_position: &DebtPosition,
    new_total_debt: i128,
) -> Result<(), LendingError> {
    let col_key = DataKey::Collateral(user.clone());
    let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);

    assert_position_within_bps(
        env,
        collateral,
        updated_position,
        crate::entrypoints::LendingImpl::max_ltv_bps_config(env),
    )?;

    if let Some(ceiling) = env
        .storage()
        .instance()
        .get::<DataKey, i128>(&DataKey::DebtCeiling)
    {
        if new_total_debt > ceiling {
            return Err(LendingError::DebtCeilingExceeded);
        }
    }

    Ok(())
}

pub(crate) fn current_borrow_rate(env: &Env) -> i128 {
    cached_borrow_rate(env)
}

pub(crate) fn write_utilization_sample(env: &Env, utilization_bps: i128) {
    let mut samples = utilization_history_oldest_first(env);

    if let Some(last) = samples.last() {
        let last: UtilizationSample = last;
        if last.ledger == env.ledger().sequence() {
            return;
        }
    }

    while samples.len() >= UTILIZATION_HISTORY_CAPACITY {
        samples.remove(0);
    }

    samples.push_back(UtilizationSample {
        ledger: env.ledger().sequence(),
        utilization_bps,
    });

    env.storage()
        .persistent()
        .set(&DataKey::UtilizationHistory, &samples);
    crate::ttl::bump_global(&env, &DataKey::UtilizationHistory);
}

// Load utilization samples in their storage order, oldest-first.
pub(crate) fn utilization_history_oldest_first(env: &Env) -> Vec<UtilizationSample> {
    env.storage()
        .persistent()
        .get(&DataKey::UtilizationHistory)
        .unwrap_or_else(|| Vec::new(env))
}

// Load utilization samples in view order, newest-first.
pub(crate) fn utilization_history_newest_first(env: &Env) -> Vec<UtilizationSample> {
    let samples = utilization_history_oldest_first(env);
    let mut newest_first = Vec::new(env);
    let mut index = samples.len();

    while index > 0 {
        index -= 1;
        let sample: UtilizationSample = samples.get(index).unwrap();
        newest_first.push_back(sample);
    }

    newest_first
}

pub(crate) fn uses_cross_asset_valuation(env: &Env) -> bool {
    match (
        env.storage()
            .instance()
            .get::<_, Address>(&DataKey::ValuationCollateralAsset),
        env.storage()
            .instance()
            .get::<_, Address>(&DataKey::ValuationDebtAsset),
    ) {
        (Some(collateral), Some(debt)) => collateral != debt,
        _ => false,
    }
}

pub(crate) fn value_collateral(env: &Env, amount: i128) -> Result<i128, LendingError> {
    value_amount(env, amount, true)
}

pub(crate) fn value_debt(env: &Env, amount: i128) -> Result<i128, LendingError> {
    value_amount(env, amount, false)
}

pub(crate) fn value_amount(env: &Env, amount: i128, round_down: bool) -> Result<i128, LendingError> {
    if !uses_cross_asset_valuation(env) {
        return Ok(amount);
    }
    if amount < 0 {
        return Err(LendingError::InvalidAmount);
    }

    let key = if round_down {
        DataKey::ValuationCollateralAsset
    } else {
        DataKey::ValuationDebtAsset
    };
    let asset: Address = env
        .storage()
        .instance()
        .get(&key)
        .ok_or(LendingError::PriceUnavailable)?;
    let record = cross_asset::get_price_for_asset(env, &asset)?;
    let result = if round_down {
        math::checked_mul_div_floor(amount, record.price, ORACLE_PRICE_SCALE)
    } else {
        math::checked_mul_div_ceil(amount, record.price, ORACLE_PRICE_SCALE)
    };
    result.map_err(|_| LendingError::Overflow)
}

pub(crate) fn read_position_summary(env: &Env, user: &Address) -> (PositionSummary, bool) {
    let collateral: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::Collateral(user.clone()))
        .unwrap_or(0);
    if collateral != 0 {
        extend_collateral_ttl(env, user);
    }
    let position = load_debt(env, user);
    if position.principal != 0 {
        extend_debt_ttl(env, user);
    }

    let debt = effective_debt(&position, env.ledger().timestamp(), current_borrow_rate(env))
        .unwrap_or(position.principal)
        .max(0);
    let (health_factor, price_stale) = position_health_factor(env, collateral, debt);
    (
        PositionSummary {
            collateral,
            debt,
            health_factor,
        },
        price_stale,
    )
}

pub(crate) fn position_health_factor(env: &Env, collateral: i128, debt: i128) -> (i128, bool) {
    if debt <= 0 {
        return (HEALTH_FACTOR_NO_DEBT, false);
    }
    let threshold = LendingContract::get_liquidation_threshold_bps(env.clone());
    if !uses_cross_asset_valuation(env) {
        let value = math::checked_mul_div_floor(collateral, threshold, debt).unwrap_or(i128::MAX);
        return (value, false);
    }

    let collateral_asset = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::ValuationCollateralAsset);
    let debt_asset = env
        .storage()
        .instance()
        .get::<_, Address>(&DataKey::ValuationDebtAsset);
    let (Some(collateral_asset), Some(debt_asset)) = (collateral_asset, debt_asset) else {
        return (0, true);
    };
    let collateral_record = env
        .storage()
        .persistent()
        .get::<_, PriceRecord>(&DataKey::OraclePrice(collateral_asset.clone()));
    let debt_record = env
        .storage()
        .persistent()
        .get::<_, PriceRecord>(&DataKey::OraclePrice(debt_asset.clone()));
    let (Some(collateral_record), Some(debt_record)) = (collateral_record, debt_record) else {
        return (0, true);
    };

    let now = env.ledger().timestamp();
    let stale = collateral_record.price <= 0
        || debt_record.price <= 0
        || collateral_record.timestamp > now
        || debt_record.timestamp > now
        || now.saturating_sub(collateral_record.timestamp) > max_age_for(env, &collateral_asset)
        || now.saturating_sub(debt_record.timestamp) > max_age_for(env, &debt_asset);
    if collateral_record.price <= 0 || debt_record.price <= 0 {
        return (0, true);
    }

    let collateral_value = match math::checked_mul_div_floor(
        collateral,
        collateral_record.price,
        ORACLE_PRICE_SCALE,
    ) {
        Ok(value) => value,
        Err(_) => return (0, true),
    };
    let debt_value = match math::checked_mul_div_ceil(debt, debt_record.price, ORACLE_PRICE_SCALE) {
        Ok(value) if value > 0 => value,
        _ => return (0, true),
    };
    let health_factor = match math::checked_mul_div_floor(collateral_value, threshold, debt_value) {
        Ok(value) => value,
        Err(_) => return (0, true),
    };
    (health_factor, stale)
}
