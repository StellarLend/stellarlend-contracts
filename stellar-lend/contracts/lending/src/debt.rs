use soroban_sdk::{contracttype, Address, Env};

use crate::rounding_strategy::{calculate_interest_with_rounding, RoundingError, RoundingMode};
use crate::DataKey;

pub const DEFAULT_APR_BPS: i128 = 500;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtPosition {
    pub principal: i128,
    pub last_update: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebtError {
    Overflow,
    InvalidAmount,
}

impl From<&'static str> for DebtError {
    fn from(_: &'static str) -> Self {
        DebtError::Overflow
    }
}

impl From<RoundingError> for DebtError {
    fn from(_: RoundingError) -> Self {
        DebtError::Overflow
    }
}

pub fn load_debt(env: &Env, user: &Address) -> DebtPosition {
    let key = DataKey::Debt(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(DebtPosition {
            principal: 0,
            last_update: env.ledger().timestamp(),
        })
}

pub fn save_debt(env: &Env, user: &Address, position: &DebtPosition) {
    let key = DataKey::Debt(user.clone());
    env.storage().persistent().set(&key, position);
}

pub fn elapsed_seconds(now: u64, last_update: u64) -> u64 {
    now.saturating_sub(last_update)
}

pub fn accrue_interest(principal: i128, elapsed: u64, rate_bps: i128) -> Result<i128, DebtError> {
    if principal == 0 || elapsed == 0 {
        return Ok(0);
    }

    let result =
        calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Bankers)?;

    if result.interest < 0 {
        return Err(DebtError::Overflow);
    }

    Ok(result.interest)
}

pub fn settle_accrual(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError> {
    let elapsed = elapsed_seconds(now, position.last_update);
    let interest = accrue_interest(position.principal, elapsed, rate_bps)?;
    let principal = position
        .principal
        .checked_add(interest)
        .ok_or(DebtError::Overflow)?;

    Ok(DebtPosition {
        principal,
        last_update: now,
    })
}

pub fn effective_debt(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<i128, DebtError> {
    let elapsed = elapsed_seconds(now, position.last_update);
    let interest = accrue_interest(position.principal, elapsed, rate_bps)?;
    position
        .principal
        .checked_add(interest)
        .ok_or(DebtError::Overflow)
}

pub fn borrow_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError> {
    if amount <= 0 {
        return Err(DebtError::InvalidAmount);
    }

    let mut settled = settle_accrual(&position, now, rate_bps)?;
    settled.principal = settled
        .principal
        .checked_add(amount)
        .ok_or(DebtError::Overflow)?;
    settled.last_update = now;
    Ok(settled)
}

pub fn repay_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError> {
    if amount <= 0 {
        return Err(DebtError::InvalidAmount);
    }

    let mut settled = settle_accrual(&position, now, rate_bps)?;
    settled.principal = if amount >= settled.principal {
        0
    } else {
        settled.principal - amount
    };
    settled.last_update = now;
    Ok(settled)
}
