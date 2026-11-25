//! Isolation-mode debt accounting helpers.

use super::*;

// -----------------------------------------------------------------------
// Isolation-mode internal helpers
// -----------------------------------------------------------------------

// Returns `true` when `asset` has `isolated = true` in its stored config.
// Returns `false` when unconfigured or `isolated = false`.
pub(crate) fn is_asset_isolated(env: &Env, asset: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::AssetIsolation(asset.clone()))
        .map(|c: IsolationConfig| c.isolated)
        .unwrap_or(false)
}

// Return whether a debt reduction should release an isolation bucket.
// 
// The tracker is retained when governance disables isolation so historical
// debt remains auditable. Repayments and liquidations must still release that
// retained capacity. A positive tracker is therefore sufficient to identify
// this legacy/disabled case, while an empty unconfigured asset remains
// unaffected by ordinary debt operations.
pub(crate) fn should_release_isolation_debt(env: &Env, asset: &Address) -> bool {
    is_asset_isolated(env, asset)
        || LendingContract::get_isolation_debt(env.clone(), asset.clone()) > 0
}

// Check whether a borrow of `amount` against `collateral_asset` would breach
// the isolation debt ceiling.  Returns `Ok(())` when the asset is not
// isolated or when the ceiling would not be exceeded.
pub(crate) fn check_isolation_ceiling_internal(
    env: &Env,
    collateral_asset: &Address,
    amount: i128,
) -> Result<(), LendingError> {
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let config: Option<IsolationConfig> = env
        .storage()
        .persistent()
        .get(&DataKey::AssetIsolation(collateral_asset.clone()));

    let cfg = match config {
        Some(c) if c.isolated => c,
        // Not isolated — nothing to enforce.
        _ => return Ok(()),
    };

    let current_isolation_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::IsolationDebt(collateral_asset.clone()))
        .unwrap_or(0);

    let new_isolation_debt = current_isolation_debt
        .checked_add(amount)
        .ok_or(LendingError::Overflow)?;

    if new_isolation_debt > cfg.isolation_debt_ceiling {
        return Err(LendingError::IsolationCeilingExceeded);
    }

    Ok(())
}

// Increment the running isolation-debt counter for `collateral_asset` by `delta`.
// 
// Called after a successful `borrow_against_collateral` to keep the ceiling
// tracker in sync with actual outstanding debt. Overflow is an invariant
// violation rather than an opportunity to wrap the counter.
pub(crate) fn increment_isolation_debt(
    env: &Env,
    collateral_asset: &Address,
    delta: i128,
) -> Result<(), LendingError> {
    let key = DataKey::IsolationDebt(collateral_asset.clone());
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    let updated = current
        .checked_add(delta)
        .ok_or(LendingError::IsolationDebtInvariant)?;
    env.storage().persistent().set(&key, &updated);
    ttl::bump_global(env, &key);
    Ok(())
}

// Decrement the running isolation-debt counter for `collateral_asset` by `amount`.
// 
// Called after a successful `repay_against_collateral`.  Rejects a release
// larger than the bucket instead of saturating to zero and hiding corruption.
pub(crate) fn decrement_isolation_debt(
    env: &Env,
    collateral_asset: &Address,
    amount: i128,
) -> Result<(), LendingError> {
    let key = DataKey::IsolationDebt(collateral_asset.clone());
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    if amount < 0 || amount > current {
        return Err(LendingError::IsolationDebtInvariant);
    }
    let updated = current
        .checked_sub(amount)
        .ok_or(LendingError::IsolationDebtInvariant)?;
    env.storage().persistent().set(&key, &updated);
    ttl::bump_global(env, &key);
    Ok(())
}
