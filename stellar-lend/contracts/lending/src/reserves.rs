//! Insurance fund, reserve-factor checkpoint and reentrancy-lock helpers.

use super::*;

pub(crate) fn settle_and_accrue_insurance(
    env: &Env,
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<DebtPosition, LendingError> {
    let current_index = touch_borrow_index(env, now, rate_bps);

    let (interest, settled) = if position.borrow_index_snapshot > 0 {
        let settled = debt::settle_position(position, current_index, now)
            .map_err(|_| LendingError::Overflow)?;
        let interest = settled
            .principal
            .checked_sub(position.principal)
            .ok_or(LendingError::Overflow)?;
        (interest, settled)
    } else {
        let elapsed = debt::elapsed_seconds(now, position.last_update);
        let interest = debt::accrue_interest(position.principal, elapsed, rate_bps)
            .map_err(|_| LendingError::Overflow)?;
        let mut settled =
            debt::settle_accrual(position, now, rate_bps).map_err(|_| LendingError::Overflow)?;
        settled.borrow_index_snapshot = current_index;
        (interest, settled)
    };

    credit_insurance_share(env, interest)?;

    Ok(settled)
}

// Credit protocol revenue from `interest`: the reserve-factor cut goes to
// `ProtocolReserves` first, then the insurance fund receives its configured
// share of the remainder, so the two cuts can never exceed gross interest.
//
// Shared by the single-asset settlement path ([`settle_and_accrue_insurance`])
// and the per-asset cross-asset settlement path so both routes accrue
// protocol revenue identically.
pub(crate) fn credit_insurance_share(env: &Env, interest: i128) -> Result<(), LendingError> {
    if interest <= 0 {
        return Ok(());
    }
    let reserve_factor_bps: i128 = env
        .storage()
        .instance()
        .get(&DataKey::ReserveFactorBps)
        .unwrap_or(0);
    let reserve_factor_bps =
        u32::try_from(reserve_factor_bps).map_err(|_| LendingError::Overflow)?;
    let (_, reserve_cut) =
        math::split_interest_by_reserve_factor(interest, reserve_factor_bps)
            .map_err(|_| LendingError::Overflow)?;
    if reserve_cut > 0 {
        let reserves: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::ProtocolReserves)
            .unwrap_or(0);
        let new_reserves = reserves
            .checked_add(reserve_cut)
            .ok_or(LendingError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::ProtocolReserves, &new_reserves);
    }

    let share_bps: i128 = env
        .storage()
        .instance()
        .get(&DataKey::InsuranceShareBps)
        .unwrap_or(0);
    if share_bps > 0 {
        // The reserve cut is taken first; insurance receives its configured
        // share of the remaining interest, so the two protocol cuts cannot
        // exceed gross interest.
        let insurance_base = interest
            .checked_sub(reserve_cut)
            .ok_or(LendingError::Overflow)?;
        let share = insurance_base
            .checked_mul(share_bps)
            .ok_or(LendingError::Overflow)?
            .checked_div(BPS_DENOM)
            .ok_or(LendingError::Overflow)?;
        if share > 0 {
            let current_fund: i128 = env
                .storage()
                .instance()
                .get(&DataKey::InsuranceFund)
                .unwrap_or(0);
            let new_fund = current_fund
                .checked_add(share)
                .ok_or(LendingError::Overflow)?;
            env.storage()
                .instance()
                .set(&DataKey::InsuranceFund, &new_fund);
        }
    }
    Ok(())
}

pub(crate) fn checkpoint_reserve_factor(env: &Env) -> Result<(), LendingError> {
    let now = env.ledger().timestamp();
    let rate = cached_borrow_rate(env);
    touch_borrow_index(env, now, rate);

    let borrowers: Vec<Address> = env
        .storage()
        .instance()
        .get(&DataKey::BorrowerList)
        .unwrap_or_else(|| soroban_sdk::vec![env]);
    let mut accrued_total = 0i128;

    for i in 0..borrowers.len() {
        let borrower = borrowers.get(i).ok_or(LendingError::Overflow)?;
        let position = load_debt(env, &borrower);
        if position.principal <= 0 {
            continue;
        }
        let settled = settle_and_accrue_insurance(env, &position, now, rate)?;
        let accrued = settled
            .principal
            .checked_sub(position.principal)
            .ok_or(LendingError::Overflow)?;
        if accrued < 0 {
            return Err(LendingError::Overflow);
        }
        accrued_total = accrued_total
            .checked_add(accrued)
            .ok_or(LendingError::Overflow)?;
        save_debt(env, &borrower, &settled);
        extend_debt_ttl(env, &borrower);
    }

    if accrued_total > 0 {
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let new_total_debt = total_debt
            .checked_add(accrued_total)
            .ok_or(LendingError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        crate::ttl::bump_global(&env, &DataKey::TotalDebt);
    }
    Ok(())
}

pub(crate) fn draw_insurance(env: &Env, amount: i128) -> Result<i128, LendingError> {
    if amount <= 0 {
        return Ok(0);
    }
    let current: i128 = env
        .storage()
        .instance()
        .get(&DataKey::InsuranceFund)
        .unwrap_or(0);
    let drawn = amount.min(current);
    let new_balance = current.checked_sub(drawn).ok_or(LendingError::Overflow)?;
    env.storage()
        .instance()
        .set(&DataKey::InsuranceFund, &new_balance);
    Ok(drawn)
}

#[allow(dead_code)]
pub(crate) fn acquire_reentrancy_lock(env: &Env) {
    let reentrancy_lock_key = Symbol::new(env, "reent_l");
    let locked: bool = env
        .storage()
        .temporary()
        .get(&reentrancy_lock_key)
        .unwrap_or(false);
    if locked {
        panic_with_error!(&env, LendingError::Unauthorized);
    }
    env.storage().temporary().set(&reentrancy_lock_key, &true);
}

#[allow(dead_code)]
pub(crate) fn release_reentrancy_lock(env: &Env) {
    let reentrancy_lock_key = Symbol::new(env, "reent_l");
    env.storage().temporary().remove(&reentrancy_lock_key);
}

#[allow(dead_code)]
pub(crate) fn with_reentrancy_lock<T>(env: &Env, f: impl FnOnce() -> T) -> T {
    acquire_reentrancy_lock(env);
    let result = f();
    release_reentrancy_lock(env);
    result
}
