//! Flash-loan guard and reservation helpers.

use super::*;

// Returns an error if a flash loan is currently in progress.
//
// All state-mutating user operations (`deposit`, `withdraw`, `borrow`,
// `borrow_against_collateral`, `repay`, `repay_against_collateral`,
// `liquidate`) call this at entry to prevent reentrancy via the
// `on_flash_loan` callback.
//
// The `FlashActive` flag is set by [`flash_loan`] before the external
// callback and cleared after it returns.  On revert, Soroban's atomic
// transaction rollback restores the flag to `false`.
pub(crate) fn require_no_active_flash_loan(env: &Env) -> Result<(), LendingError> {
    let active: bool = env
        .storage()
        .instance()
        .get(&DataKey::FlashActive)
        .unwrap_or(false);
    if active {
        return Err(LendingError::FlashLoanReentrancy);
    }
    Ok(())
}

/// Return the amount of `asset` currently reserved for active flash loans.
/// Stored in Temporary storage and defaults to `0` when absent.
pub fn get_flash_loan_reserved(env: &Env, asset: &Address) -> i128 {
    env.storage()
        .temporary()
        .get(&DataKey::ReservedForFlashLoan(asset.clone()))
        .unwrap_or(0)
}

/// Reserve `amount` of `asset` for an active flash loan in Temporary storage.
pub fn reserve_flash_loan(env: &Env, asset: &Address, amount: i128) -> Result<i128, LendingError> {
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let key = DataKey::ReservedForFlashLoan(asset.clone());
    let current: i128 = env.storage().temporary().get(&key).unwrap_or(0);
    let updated = current.checked_add(amount).ok_or(LendingError::Overflow)?;
    env.storage().temporary().set(&key, &updated);
    Ok(updated)
}

/// Release `amount` of `asset` previously reserved for a flash loan.
/// Removes the Temporary storage entry when the remaining reservation reaches `0`.
pub fn release_flash_loan_reservation(
    env: &Env,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }
    let key = DataKey::ReservedForFlashLoan(asset.clone());
    let current: i128 = env.storage().temporary().get(&key).unwrap_or(0);
    if amount > current {
        return Err(LendingError::InvalidAmount);
    }
    let updated = current.checked_sub(amount).ok_or(LendingError::Overflow)?;
    if updated == 0 {
        env.storage().temporary().remove(&key);
    } else {
        env.storage().temporary().set(&key, &updated);
    }
    Ok(updated)
}
