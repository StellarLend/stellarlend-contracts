//! # Withdraw collateral
//!
//! Withdrawals reduce a user’s posted collateral while preserving **minimum collateral ratio**
//! against outstanding debt (same 150% rule as [`crate::borrow::validate_collateral_ratio`]).
//!
//! ## Pause & emergency alignment
//!
//! Withdraw respects:
//! - Legacy `WithdrawDataKey::Paused` (storage compatibility),
//! - Granular [`crate::pause::PauseType::Withdraw`] and global [`crate::pause::PauseType::All`]
//!   via [`crate::pause::is_paused`],
//! - Emergency lifecycle: **shutdown** blocks unwind; **recovery** allows `withdraw` and `repay`
//!   so users can exit (see [`crate::pause::blocks_high_risk_ops`], [`crate::pause::is_recovery`]).
//!
//! This matches the public `LendingContract::withdraw` policy: callers get a single consistent
//! check path through [`withdraw`] without duplicating pause logic in the facade.
//!
//! ## Security
//! - **Authorization**: [`withdraw`] requires `user.require_auth()`.
//! - **State**: Collateral and totals are updated before publishing events (no external token hook
//!   in this simplified flow; CEI still applies to storage writes).
//! - **Arithmetic**: Uses `checked_*` and shared borrow validation to avoid overflow and drift.

use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env};

use crate::borrow::{validate_collateral_ratio, BorrowDataKey, BorrowError, DebtPosition};
use crate::deposit::{DepositCollateral, DepositDataKey};
use crate::pause::{self, PauseType};

/// Errors that can occur during withdraw operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum WithdrawError {
    InvalidAmount = 1,
    WithdrawPaused = 2,
    Overflow = 3,
    InsufficientCollateral = 4,
    InsufficientCollateralRatio = 5,
    Unauthorized = 6,
}

/// Storage keys for withdraw-related data
#[contracttype]
#[derive(Clone)]
pub enum WithdrawDataKey {
    Paused,
    MinWithdrawAmount,
}

/// Withdraw event data
#[contractevent]
#[derive(Clone, Debug)]
pub struct WithdrawEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub remaining_balance: i128,
    pub timestamp: u64,
}

/// Enforce the same pause / emergency rules as `LendingContract::withdraw` used to apply at the
/// facade layer: granular withdraw pause, global `All`, legacy flag, and shutdown (but not
/// recovery) blocking.
fn ensure_withdraw_allowed(env: &Env) -> Result<(), WithdrawError> {
    if legacy_withdraw_paused(env) {
        return Err(WithdrawError::WithdrawPaused);
    }
    if pause::is_paused(env, PauseType::Withdraw) {
        return Err(WithdrawError::WithdrawPaused);
    }
    // Recovery: allow withdraw/repay unwind; Shutdown: block until governance moves state.
    if !pause::is_recovery(env) && pause::blocks_high_risk_ops(env) {
        return Err(WithdrawError::WithdrawPaused);
    }
    Ok(())
}

fn legacy_withdraw_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&WithdrawDataKey::Paused)
        .unwrap_or(false)
}

fn map_borrow_to_withdraw(e: BorrowError) -> WithdrawError {
    match e {
        BorrowError::InsufficientCollateral => WithdrawError::InsufficientCollateralRatio,
        BorrowError::Overflow => WithdrawError::Overflow,
        BorrowError::InvalidAmount => WithdrawError::InvalidAmount,
        _ => WithdrawError::InsufficientCollateralRatio,
    }
}

/// Withdraw collateral from the protocol.
///
/// # Arguments
/// * `user` — Position owner; must authorize the call.
/// * `asset` — Collateral asset account.
/// * `amount` — Amount to withdraw (≥ min configured, ≤ balance).
///
/// # Errors
/// * [`WithdrawError::WithdrawPaused`] — Legacy pause, `Withdraw` / `All` pause, or emergency
///   shutdown (not recovery).
/// * [`WithdrawError::InvalidAmount`] — Non-positive or below minimum withdraw.
/// * [`WithdrawError::InsufficientCollateral`] — Request exceeds balance.
/// * [`WithdrawError::InsufficientCollateralRatio`] — Would leave debt under-collateralized.
/// * [`WithdrawError::Overflow`] — Checked arithmetic overflow.
///
/// # Security
/// User auth is required. Pause checks run before balance/ratio math. Ratio enforcement delegates
/// to [`validate_collateral_ratio`] so borrow and withdraw stay aligned on the 150% rule.
pub fn withdraw(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<i128, WithdrawError> {
    user.require_auth();

    ensure_withdraw_allowed(env)?;

    if amount <= 0 {
        return Err(WithdrawError::InvalidAmount);
    }

    let min_withdraw = get_min_withdraw_amount(env);
    if amount < min_withdraw {
        return Err(WithdrawError::InvalidAmount);
    }

    let position = get_collateral_position(env, &user, &asset);

    if position.amount < amount {
        return Err(WithdrawError::InsufficientCollateral);
    }

    let new_amount = position
        .amount
        .checked_sub(amount)
        .ok_or(WithdrawError::Overflow)?;

    validate_collateral_ratio_after_withdraw(env, &user, new_amount)?;

    let updated_position = DepositCollateral {
        amount: new_amount,
        asset: asset.clone(),
        last_deposit_time: position.last_deposit_time,
    };

    save_collateral_position(env, &user, &updated_position);

    let total_deposits = get_total_deposits(env);
    let new_total = total_deposits.checked_sub(amount).unwrap_or(0);
    set_total_deposits(env, new_total);

    WithdrawEvent {
        user,
        asset,
        amount,
        remaining_balance: new_amount,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(new_amount)
}

/// Validate collateral ratio remains above minimum after withdrawal (aligned with borrow checks).
fn validate_collateral_ratio_after_withdraw(
    env: &Env,
    user: &Address,
    remaining_collateral: i128,
) -> Result<(), WithdrawError> {
    let debt_position: Option<DebtPosition> = env
        .storage()
        .persistent()
        .get(&BorrowDataKey::BorrowUserDebt(user.clone()));

    if let Some(debt) = debt_position {
        let total_debt = debt
            .borrowed_amount
            .checked_add(debt.interest_accrued)
            .ok_or(WithdrawError::Overflow)?;

        if total_debt > 0 {
            validate_collateral_ratio(remaining_collateral, total_debt)
                .map_err(map_borrow_to_withdraw)?;
        }
    }

    Ok(())
}

/// Initialize withdraw settings
pub fn initialize_withdraw_settings(
    env: &Env,
    min_withdraw_amount: i128,
) -> Result<(), WithdrawError> {
    env.storage()
        .persistent()
        .set(&WithdrawDataKey::MinWithdrawAmount, &min_withdraw_amount);
    env.storage()
        .persistent()
        .set(&WithdrawDataKey::Paused, &false);
    Ok(())
}

/// Set the legacy withdraw pause flag (kept for storage-layer compatibility).
///
/// Prefer using the unified `set_pause(PauseType::Withdraw, …)` entry point
/// on the contract, which routes through the granular pause system and emits
/// a `pause_event`. This inner helper only writes the legacy key; it is
/// intentionally kept so that the persistent-storage layout remains stable.
#[allow(dead_code)]
pub fn set_withdraw_paused(env: &Env, paused: bool) -> Result<(), WithdrawError> {
    env.storage()
        .persistent()
        .set(&WithdrawDataKey::Paused, &paused);
    Ok(())
}

fn get_collateral_position(env: &Env, user: &Address, asset: &Address) -> DepositCollateral {
    env.storage()
        .persistent()
        .get(&DepositDataKey::UserCollateral(user.clone()))
        .unwrap_or(DepositCollateral {
            amount: 0,
            asset: asset.clone(),
            last_deposit_time: env.ledger().timestamp(),
        })
}

fn save_collateral_position(env: &Env, user: &Address, position: &DepositCollateral) {
    env.storage()
        .persistent()
        .set(&DepositDataKey::UserCollateral(user.clone()), position);
}

fn get_total_deposits(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DepositDataKey::TotalAmount)
        .unwrap_or(0)
}

fn set_total_deposits(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&DepositDataKey::TotalAmount, &amount);
}

fn get_min_withdraw_amount(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&WithdrawDataKey::MinWithdrawAmount)
        .unwrap_or(0)
}
