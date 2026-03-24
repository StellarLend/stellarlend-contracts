//! # Borrow Implementation (Simplified Lending)
//!
//! Core borrow logic for the simplified lending contract. Handles collateral
//! validation, debt tracking, interest calculation, and pause controls.
//!
//! [Issue #391] Optimized gas usage by migrating protocol settings to Instance storage.

use crate::pause::{self, blocks_high_risk_ops, PauseType};
use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env, I256};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum BorrowError {
    InsufficientCollateral = 1,
    DebtCeilingReached = 2,
    ProtocolPaused = 3,
    InvalidAmount = 4,
    Overflow = 5,
    Unauthorized = 6,
    AssetNotSupported = 7,
    BelowMinimumBorrow = 8,
    RepayAmountTooHigh = 9,
    PositionNotLiquidatable = 10,
    InvalidLiquidationAmount = 11,
}

#[contracttype]
#[derive(Clone)]
pub enum BorrowDataKey {
    ProtocolAdmin,
    BorrowUserDebt(Address),
    BorrowUserCollateral(Address),
    BorrowTotalDebt,
    BorrowDebtCeiling,
    BorrowMinAmount,
    OracleAddress,
    LiquidationThresholdBps,
    LiquidationCloseFactorBps,
    LiquidationIncentiveBps,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DebtPosition {
    pub borrowed_amount: i128,
    pub interest_accrued: i128,
    pub last_update: u64,
    pub asset: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct BorrowCollateral {
    pub amount: i128,
    pub asset: Address,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct BorrowEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub collateral: i128,
    pub timestamp: u64,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct RepayEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub timestamp: u64,
}

const COLLATERAL_RATIO_MIN: i128 = 15000; // 150%
const INTEREST_RATE_PER_YEAR: i128 = 500; // 5%
const SECONDS_PER_YEAR: u64 = 31536000;

/// Borrow assets against deposited collateral.
/// Optimized to minimize CPU instructions via storage locality.
pub fn borrow(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
    collateral_asset: Address,
    collateral_amount: i128,
) -> Result<(), BorrowError> {
    user.require_auth();

    if pause::is_paused(env, PauseType::Borrow) || blocks_high_risk_ops(env) {
        return Err(BorrowError::ProtocolPaused);
    }

    if amount <= 0 || collateral_amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    // Instance storage read (Cheap)
    let min_borrow = get_min_borrow_amount(env);
    if amount < min_borrow {
        return Err(BorrowError::BelowMinimumBorrow);
    }

    validate_collateral_ratio(collateral_amount, amount)?;

    let total_debt = get_total_debt(env);
    let debt_ceiling = get_debt_ceiling(env);
    let new_total = total_debt
        .checked_add(amount)
        .ok_or(BorrowError::Overflow)?;

    if new_total > debt_ceiling {
        return Err(BorrowError::DebtCeilingReached);
    }

    let mut debt_position = get_debt_position(env, &user);
    let accrued_interest = calculate_interest(env, &debt_position);

    debt_position.borrowed_amount = debt_position
        .borrowed_amount
        .checked_add(amount)
        .ok_or(BorrowError::Overflow)?;
    debt_position.interest_accrued = debt_position
        .interest_accrued
        .checked_add(accrued_interest)
        .ok_or(BorrowError::Overflow)?;
    debt_position.last_update = env.ledger().timestamp();
    debt_position.asset = asset.clone();

    let mut collateral_position = get_collateral_position(env, &user);
    collateral_position.amount = collateral_position
        .amount
        .checked_add(collateral_amount)
        .ok_or(BorrowError::Overflow)?;
    collateral_position.asset = collateral_asset.clone();

    save_debt_position(env, &user, &debt_position);
    save_collateral_position(env, &user, &collateral_position);
    set_total_debt(env, new_total);

    emit_borrow_event(env, user, asset, amount, collateral_amount);
    Ok(())
}

fn get_min_borrow_amount(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::BorrowMinAmount)
        .unwrap_or(1000)
}

fn get_debt_ceiling(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::BorrowDebtCeiling)
        .unwrap_or(i128::MAX)
}

pub(crate) fn get_total_debt(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::BorrowTotalDebt)
        .unwrap_or(0)
}

pub(crate) fn set_total_debt(env: &Env, amount: i128) {
    env.storage()
        .instance()
        .set(&BorrowDataKey::BorrowTotalDebt, &amount);
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage()
        .instance()
        .set(&BorrowDataKey::ProtocolAdmin, admin);
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&BorrowDataKey::ProtocolAdmin)
}

pub fn get_oracle(env: &Env) -> Option<Address> {
    env.storage().instance().get(&BorrowDataKey::OracleAddress)
}

pub fn set_oracle(env: &Env, admin: &Address, oracle: Address) -> Result<(), BorrowError> {
    let current = get_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *admin != current {
        return Err(BorrowError::Unauthorized);
    }
    admin.require_auth();
    env.storage()
        .instance()
        .set(&BorrowDataKey::OracleAddress, &oracle);
    Ok(())
}

pub fn set_liquidation_threshold_bps(
    env: &Env,
    admin: &Address,
    bps: i128,
) -> Result<(), BorrowError> {
    let current = get_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *admin != current {
        return Err(BorrowError::Unauthorized);
    }
    admin.require_auth();
    if bps <= 0 || bps > 10000 {
        return Err(BorrowError::InvalidAmount);
    }
    env.storage()
        .instance()
        .set(&BorrowDataKey::LiquidationThresholdBps, &bps);
    Ok(())
}

pub fn get_liquidation_threshold_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::LiquidationThresholdBps)
        .unwrap_or(8000)
}

pub fn get_liquidation_close_factor(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::LiquidationCloseFactorBps)
        .unwrap_or(5000) // Default 50%
}

pub fn set_liquidation_close_factor(
    env: &Env,
    admin: &Address,
    bps: i128,
) -> Result<(), BorrowError> {
    let current = get_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *admin != current {
        return Err(BorrowError::Unauthorized);
    }
    admin.require_auth();
    if bps <= 0 || bps > 10000 {
        return Err(BorrowError::InvalidAmount);
    }
    env.storage()
        .instance()
        .set(&BorrowDataKey::LiquidationCloseFactorBps, &bps);
    Ok(())
}

pub fn get_liquidation_incentive(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::LiquidationIncentiveBps)
        .unwrap_or(1000) // Default 10%
}

pub fn set_liquidation_incentive(env: &Env, admin: &Address, bps: i128) -> Result<(), BorrowError> {
    let current = get_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *admin != current {
        return Err(BorrowError::Unauthorized);
    }
    admin.require_auth();
    if bps < 0 || bps > 5000 {
        // Cap incentive at 50%
        return Err(BorrowError::InvalidAmount);
    }
    env.storage()
        .instance()
        .set(&BorrowDataKey::LiquidationIncentiveBps, &bps);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// USER DATA: Persistent Storage (Remains for data scaling)
// ═══════════════════════════════════════════════════════════════════

fn get_debt_position(env: &Env, user: &Address) -> DebtPosition {
    env.storage()
        .persistent()
        .get(&BorrowDataKey::BorrowUserDebt(user.clone()))
        .unwrap_or(DebtPosition {
            borrowed_amount: 0,
            interest_accrued: 0,
            last_update: env.ledger().timestamp(),
            asset: user.clone(),
        })
}

pub(crate) fn save_debt_position(env: &Env, user: &Address, position: &DebtPosition) {
    env.storage()
        .persistent()
        .set(&BorrowDataKey::BorrowUserDebt(user.clone()), position);
}

fn get_collateral_position(env: &Env, user: &Address) -> BorrowCollateral {
    env.storage()
        .persistent()
        .get(&BorrowDataKey::BorrowUserCollateral(user.clone()))
        .unwrap_or(BorrowCollateral {
            amount: 0,
            asset: user.clone(),
        })
}

pub(crate) fn save_collateral_position(env: &Env, user: &Address, position: &BorrowCollateral) {
    env.storage()
        .persistent()
        .set(&BorrowDataKey::BorrowUserCollateral(user.clone()), position);
}

// Remaining logic (calculate_interest, etc) remains unchanged but benefits from optimized callers.
pub(crate) fn calculate_interest(env: &Env, position: &DebtPosition) -> i128 {
    if position.borrowed_amount == 0 {
        return 0;
    }
    let time_elapsed = env
        .ledger()
        .timestamp()
        .saturating_sub(position.last_update);
    let borrowed_256 = I256::from_i128(env, position.borrowed_amount);
    let rate_256 = I256::from_i128(env, INTEREST_RATE_PER_YEAR);
    let time_256 = I256::from_i128(env, time_elapsed as i128);

    let interest_256 = borrowed_256
        .mul(&rate_256)
        .mul(&time_256)
        .div(&I256::from_i128(env, 10000))
        .div(&I256::from_i128(env, SECONDS_PER_YEAR as i128));

    interest_256.to_i128().unwrap_or(i128::MAX)
}

pub fn initialize_borrow_settings(
    env: &Env,
    debt_ceiling: i128,
    min_borrow_amount: i128,
) -> Result<(), BorrowError> {
    env.storage()
        .instance()
        .set(&BorrowDataKey::BorrowDebtCeiling, &debt_ceiling);
    env.storage()
        .instance()
        .set(&BorrowDataKey::BorrowMinAmount, &min_borrow_amount);
    Ok(())
}

fn emit_borrow_event(env: &Env, user: Address, asset: Address, amount: i128, collateral: i128) {
    BorrowEvent {
        user,
        asset,
        amount,
        collateral,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);
}

pub(crate) fn validate_collateral_ratio(collateral: i128, borrow: i128) -> Result<(), BorrowError> {
    let min_collateral = borrow
        .checked_mul(COLLATERAL_RATIO_MIN)
        .ok_or(BorrowError::Overflow)?
        .checked_div(10000)
        .ok_or(BorrowError::InvalidAmount)?;
    if collateral < min_collateral {
        return Err(BorrowError::InsufficientCollateral);
    }
    Ok(())
}

pub fn get_user_debt(env: &Env, user: &Address) -> DebtPosition {
    let mut position = get_debt_position(env, user);
    let accrued = calculate_interest(env, &position);
    position.interest_accrued = position.interest_accrued.saturating_add(accrued);
    position
}

pub fn get_user_collateral(env: &Env, user: &Address) -> BorrowCollateral {
    get_collateral_position(env, user)
}

pub fn deposit(env: &Env, user: Address, asset: Address, amount: i128) -> Result<(), BorrowError> {
    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }
    let mut collateral_position = get_collateral_position(env, &user);
    if collateral_position.amount == 0 {
        collateral_position.asset = asset.clone();
    } else if collateral_position.asset != asset {
        return Err(BorrowError::AssetNotSupported);
    }
    collateral_position.amount = collateral_position
        .amount
        .checked_add(amount)
        .ok_or(BorrowError::Overflow)?;
    save_collateral_position(env, &user, &collateral_position);
    crate::deposit::DepositEvent {
        user,
        asset,
        amount,
        new_balance: collateral_position.amount,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);
    Ok(())
}

pub fn repay(env: &Env, user: Address, asset: Address, amount: i128) -> Result<(), BorrowError> {
    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }
    let mut debt_position = get_debt_position(env, &user);
    if debt_position.borrowed_amount == 0 && debt_position.interest_accrued == 0 {
        return Err(BorrowError::InvalidAmount);
    }
    if debt_position.asset != asset {
        return Err(BorrowError::AssetNotSupported);
    }
    let accrued_interest = calculate_interest(env, &debt_position);
    debt_position.interest_accrued = debt_position
        .interest_accrued
        .checked_add(accrued_interest)
        .ok_or(BorrowError::Overflow)?;
    debt_position.last_update = env.ledger().timestamp();
    let mut remaining_repayment = amount;
    if remaining_repayment >= debt_position.interest_accrued {
        remaining_repayment -= debt_position.interest_accrued;
        debt_position.interest_accrued = 0;
    } else {
        debt_position.interest_accrued -= remaining_repayment;
        remaining_repayment = 0;
    }
    if remaining_repayment > 0 {
        if remaining_repayment > debt_position.borrowed_amount {
            return Err(BorrowError::RepayAmountTooHigh);
        }
        debt_position.borrowed_amount -= remaining_repayment;
        let total_debt = get_total_debt(env);
        let new_total = total_debt.saturating_sub(remaining_repayment);
        set_total_debt(env, new_total);
    }
    save_debt_position(env, &user, &debt_position);
    RepayEvent {
        user,
        asset,
        amount,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);
    Ok(())
}
