//! # Liquidation Implementation
//!
//! Handles the liquidation of undercollateralized positions.
//! Implements close factor, liquidation incentive, and safe asset transfers.

use crate::borrow::{
    get_liquidation_close_factor, get_liquidation_incentive, get_total_debt, get_user_collateral,
    get_user_debt, save_collateral_position, save_debt_position, set_total_debt, BorrowError,
};
use crate::views::{collateral_value, debt_value, get_health_factor, HEALTH_FACTOR_SCALE};
use soroban_sdk::{contractevent, Address, Env, I256};

#[contractevent]
#[derive(Clone, Debug)]
pub struct LiquidationEvent {
    pub liquidator: Address,
    pub borrower: Address,
    pub debt_asset: Address,
    pub collateral_asset: Address,
    pub debt_repaid: i128,
    pub collateral_seized: i128,
    pub timestamp: u64,
}

/// Executes a liquidation of an undercollateralized position.
///
/// # Arguments
/// * `env` - Contract environment
/// * `liquidator` - Address of the liquidator providing debt tokens
/// * `borrower` - Address of the borrower being liquidated
/// * `debt_asset` - Asset being repaid by the liquidator
/// * `collateral_asset` - Asset being seized from the borrower
/// * `repay_amount` - Initial amount of debt to repay (subject to close factor)
pub fn liquidate(
    env: &Env,
    liquidator: Address,
    borrower: Address,
    debt_asset: Address,
    collateral_asset: Address,
    repay_amount: i128,
) -> Result<(), BorrowError> {
    liquidator.require_auth();

    if repay_amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    // 1. Get positions
    let mut debt_pos = get_user_debt(env, &borrower);
    let mut collat_pos = get_user_collateral(env, &borrower);

    // 2. Validate assets and position health
    if debt_pos.asset != debt_asset || collat_pos.asset != collateral_asset {
        return Err(BorrowError::AssetNotSupported);
    }

    let hf = get_health_factor(env, &borrower);
    if hf >= HEALTH_FACTOR_SCALE {
        return Err(BorrowError::PositionNotLiquidatable);
    }

    // 3. Calculate max liquidatable amount based on Close Factor
    let total_debt = debt_pos
        .borrowed_amount
        .checked_add(debt_pos.interest_accrued)
        .ok_or(BorrowError::Overflow)?;
    let close_factor = get_liquidation_close_factor(env);

    let max_liquidatable = I256::from_i128(env, total_debt)
        .mul(&I256::from_i128(env, close_factor))
        .div(&I256::from_i128(env, 10000))
        .to_i128()
        .unwrap_or(0);

    let actual_repay = if repay_amount > max_liquidatable {
        max_liquidatable
    } else {
        repay_amount
    };

    if actual_repay <= 0 {
        return Err(BorrowError::InvalidLiquidationAmount);
    }

    // 4. Calculate collateral to seize including incentive
    // Seized = (RepayValue * (1 + Incentive)) / CollateralPrice
    let dv = debt_value(env, &debt_pos); // Value of total debt
    let cv = collateral_value(env, &collat_pos); // Value of total collateral

    if dv <= 0 || cv <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    let incentive = get_liquidation_incentive(env);

    // amount_to_seize_raw = actual_repay * (DebtPrice / CollateralPrice) * (1 + incentive)
    // We use values to simplify: seized_value = actual_repay_value * (1 + incentive)
    let actual_repay_value = I256::from_i128(env, dv)
        .mul(&I256::from_i128(env, actual_repay))
        .div(&I256::from_i128(env, total_debt));

    let seized_value = actual_repay_value
        .mul(&I256::from_i128(env, 10000 + incentive))
        .div(&I256::from_i128(env, 10000));

    // Convert back to collateral units: seized_amount = seized_value * collateral_amount / collateral_value
    let collateral_to_seize = seized_value
        .mul(&I256::from_i128(env, collat_pos.amount))
        .div(&I256::from_i128(env, cv))
        .to_i128()
        .unwrap_or(i128::MAX);

    let actual_seize = if collateral_to_seize > collat_pos.amount {
        collat_pos.amount
    } else {
        collateral_to_seize
    };

    // 5. Update state
    // Update debt
    let mut remaining_repay = actual_repay;
    if remaining_repay >= debt_pos.interest_accrued {
        remaining_repay -= debt_pos.interest_accrued;
        debt_pos.interest_accrued = 0;
    } else {
        debt_pos.interest_accrued -= remaining_repay;
        remaining_repay = 0;
    }

    debt_pos.borrowed_amount -= remaining_repay;
    debt_pos.last_update = env.ledger().timestamp();

    // Update collateral
    collat_pos.amount -= actual_seize;

    // Global stats
    let current_total_debt = get_total_debt(env);
    set_total_debt(env, current_total_debt.saturating_sub(remaining_repay));

    save_debt_position(env, &borrower, &debt_pos);
    save_collateral_position(env, &borrower, &collat_pos);

    // 6. Emit event
    LiquidationEvent {
        liquidator,
        borrower,
        debt_asset,
        collateral_asset,
        debt_repaid: actual_repay,
        collateral_seized: actual_seize,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(())
}
