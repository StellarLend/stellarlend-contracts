//! # Borrow Implementation (Simplified Lending)
//!
//! Core borrow logic for the simplified lending contract. Handles collateral
//! validation, debt tracking, interest calculation, and pause controls.
//!
//! [Issue #391] Optimized gas usage by migrating protocol settings to Instance storage.

use crate::pause::{self, blocks_high_risk_ops, PauseType};
use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env, FromVal, IntoVal, Val, I256};

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
    NotLiquidatable = 10,
    ExceedsCloseFactor = 11,
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
    CloseFactorBps,
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

#[contractevent]
#[derive(Clone, Debug)]
pub struct LiquidationEvent {
    pub liquidator: Address,
    pub borrower: Address,
    pub debt_asset: Address,
    pub collateral_asset: Address,
    pub debt_amount: i128,
    pub collateral_seized: i128,
    pub incentive_amount: i128,
    pub timestamp: u64,
}

const COLLATERAL_RATIO_MIN: i128 = 15000; // 150%
const INTEREST_RATE_PER_YEAR: i128 = 500; // 5%
const SECONDS_PER_YEAR: u64 = 31536000;
const PRICE_SCALE: i128 = 100_000_000;

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

fn get_total_debt(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::BorrowTotalDebt)
        .unwrap_or(0)
}

fn set_total_debt(env: &Env, amount: i128) {
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

pub fn get_liquidation_threshold_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::LiquidationThresholdBps)
        .unwrap_or(8000)
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

pub fn get_close_factor_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::CloseFactorBps)
        .unwrap_or(5000) // Default 50%
}

pub fn set_close_factor_bps(env: &Env, admin: &Address, bps: i128) -> Result<(), BorrowError> {
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
        .set(&BorrowDataKey::CloseFactorBps, &bps);
    Ok(())
}

pub fn get_liquidation_incentive_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&BorrowDataKey::LiquidationIncentiveBps)
        .unwrap_or(1000) // Default 10%
}

pub fn set_liquidation_incentive_bps(env: &Env, admin: &Address, bps: i128) -> Result<(), BorrowError> {
    let current = get_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *admin != current {
        return Err(BorrowError::Unauthorized);
    }
    admin.require_auth();
    if bps < 0 || bps > 5000 {
        return Err(BorrowError::InvalidAmount);
    }
    env.storage()
        .instance()
        .set(&BorrowDataKey::LiquidationIncentiveBps, &bps);
    Ok(())
}

// USER DATA: Persistent Storage

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

fn save_debt_position(env: &Env, user: &Address, position: &DebtPosition) {
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

fn save_collateral_position(env: &Env, user: &Address, position: &BorrowCollateral) {
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

pub fn liquidate_position(
    env: &Env,
    liquidator: Address,
    borrower: Address,
    debt_asset: Address,
    collateral_asset: Address,
    amount: i128,
) -> Result<(i128, i128), BorrowError> {
    if liquidator == borrower {
        return Err(BorrowError::Unauthorized);
    }

    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    if pause::is_paused(env, PauseType::Liquidation) || blocks_high_risk_ops(env) {
        return Err(BorrowError::ProtocolPaused);
    }

    // 1. Load context and accrue interest
    let mut debt_pos = get_debt_position(env, &borrower);
    if debt_pos.borrowed_amount == 0 && debt_pos.interest_accrued == 0 {
        return Err(BorrowError::NotLiquidatable);
    }
    if debt_pos.asset != debt_asset {
        return Err(BorrowError::AssetNotSupported);
    }

    let mut collateral_pos = get_collateral_position(env, &borrower);
    if collateral_pos.amount == 0 || collateral_pos.asset != collateral_asset {
        return Err(BorrowError::NotLiquidatable);
    }

    let accrued = calculate_interest(env, &debt_pos);
    debt_pos.interest_accrued = debt_pos.interest_accrued.checked_add(accrued).ok_or(BorrowError::Overflow)?;
    debt_pos.last_update = env.ledger().timestamp();

    let total_debt = debt_pos.borrowed_amount.checked_add(debt_pos.interest_accrued).ok_or(BorrowError::Overflow)?;

    // 2. Validate health factor
    let oracle = get_oracle(env).ok_or(BorrowError::Unauthorized)?;
    let price_debt = get_asset_price(env, &oracle, &debt_asset);
    let price_collateral = get_asset_price(env, &oracle, &collateral_asset);

    if price_debt <= 0 || price_collateral <= 0 {
        return Err(BorrowError::NotLiquidatable);
    }

    let dv = calculate_value(env, total_debt, price_debt);
    let cv = calculate_value(env, collateral_pos.amount, price_collateral);
    
    let threshold = get_liquidation_threshold_bps(env);
    let weighted_collateral = I256::from_i128(env, cv)
        .mul(&I256::from_i128(env, threshold))
        .div(&I256::from_i128(env, 10000));
    
    if weighted_collateral >= I256::from_i128(env, dv) {
        return Err(BorrowError::NotLiquidatable);
    }

    // 3. Enforce Close Factor
    let close_factor = get_close_factor_bps(env);
    let max_repayable = I256::from_i128(env, total_debt)
        .mul(&I256::from_i128(env, close_factor))
        .div(&I256::from_i128(env, 10000))
        .to_i128().ok_or(BorrowError::Overflow)?;
    
    if amount > max_repayable {
        return Err(BorrowError::ExceedsCloseFactor);
    }

    let actual_repay = amount;

    // 4. Calculate seized collateral with incentive
    let incentive_bps = get_liquidation_incentive_bps(env);
    
    let base_seizure = I256::from_i128(env, actual_repay)
        .mul(&I256::from_i128(env, price_debt))
        .div(&I256::from_i128(env, price_collateral));
    
    let total_seizure_256 = base_seizure
        .mul(&I256::from_i128(env, 10000 + incentive_bps))
        .div(&I256::from_i128(env, 10000));
    
    let collateral_to_seize = total_seizure_256.to_i128().ok_or(BorrowError::Overflow)?;
    let final_seizure = if collateral_to_seize > collateral_pos.amount {
        collateral_pos.amount
    } else {
        collateral_to_seize
    };

    let incentive_amount = final_seizure.saturating_sub(
        base_seizure.to_i128().unwrap_or(0)
    );

    // 5. Update state
    let mut remaining_repay = actual_repay;
    if remaining_repay >= debt_pos.interest_accrued {
        remaining_repay -= debt_pos.interest_accrued;
        debt_pos.interest_accrued = 0;
    } else {
        debt_pos.interest_accrued -= remaining_repay;
        remaining_repay = 0;
    }
    debt_pos.borrowed_amount = debt_pos.borrowed_amount.checked_sub(remaining_repay).unwrap_or(0);
    save_debt_position(env, &borrower, &debt_pos);

    collateral_pos.amount = collateral_pos.amount.checked_sub(final_seizure).unwrap_or(0);
    save_collateral_position(env, &borrower, &collateral_pos);

    let total_debt_global = get_total_debt(env);
    set_total_debt(env, total_debt_global.saturating_sub(actual_repay));

    // 6. Events
    LiquidationEvent {
        liquidator,
        borrower,
        debt_asset,
        collateral_asset,
        debt_amount: actual_repay,
        collateral_seized: final_seizure,
        incentive_amount,
        timestamp: env.ledger().timestamp(),
    }.publish(env);

    Ok((actual_repay, final_seizure))
}

fn get_asset_price(env: &Env, oracle: &Address, asset: &Address) -> i128 {
    use soroban_sdk::{Val, Symbol, FromVal};
    let price_val: Val = env.invoke_contract(
        oracle,
        &Symbol::new(env, "price"),
        (asset.clone(),).into_val(env),
    );
    i128::from_val(env, &price_val)
}

fn calculate_value(env: &Env, amount: i128, price: i128) -> i128 {
    let amount_256 = I256::from_i128(env, amount);
    let price_256 = I256::from_i128(env, price);
    let scale_256 = I256::from_i128(env, PRICE_SCALE);
    amount_256.mul(&price_256).div(&scale_256).to_i128().unwrap_or(0)
}
