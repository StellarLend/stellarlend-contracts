#![allow(unused)]
use crate::events::{emit_liquidation, LiquidationEvent};
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::{contracterror, token, Address, Env, IntoVal, Map, Symbol, Val, Vec, I256};

use crate::deposit::{
    add_activity_log, emit_analytics_updated_event, emit_position_updated_event,
    emit_user_activity_tracked_event, AssetParams, DepositDataKey, Position, ProtocolAnalytics,
    UserAnalytics,
};
use crate::oracle::get_price;
use crate::risk_management::{
    is_emergency_paused, is_operation_paused, require_operation_not_paused, RiskManagementError,
};
use crate::risk_params::{
    can_be_liquidated, get_liquidation_incentive_amount, get_max_liquidatable_amount,
    get_risk_params,
};

/// Errors that can occur during liquidation operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LiquidationError {
    /// Liquidation amount must be greater than zero
    InvalidAmount = 1,
    /// Asset address is invalid
    InvalidAsset = 2,
    /// Position is not undercollateralized
    NotLiquidatable = 3,
    /// Liquidation operations are currently paused
    LiquidationPaused = 4,
    /// Liquidation amount exceeds maximum allowed (close factor)
    ExceedsCloseFactor = 5,
    /// Insufficient balance to liquidate
    InsufficientBalance = 6,
    /// Overflow occurred during calculation
    Overflow = 7,
    /// Reentrancy detected during liquidation
    Reentrancy = 8,
    /// Invalid collateral asset
    InvalidCollateralAsset = 9,
    /// Invalid debt asset
    InvalidDebtAsset = 10,
    /// Price not available for asset
    PriceNotAvailable = 11,
}

/// Helper to get asset decimals from the token contract or default to 7 for XLM.
fn get_asset_decimals(env: &Env, asset: &Option<Address>) -> u32 {
    match asset {
        Some(addr) => TokenClient::new(env, addr).decimals(),
        None => 7, // Native XLM has 7 decimals
    }
}

/// Helper function to get the native asset contract address from storage
fn get_native_asset_address(env: &Env) -> Result<Address, LiquidationError> {
    env.storage()
        .persistent()
        .get::<DepositDataKey, Address>(&DepositDataKey::NativeAssetAddress)
        .ok_or(LiquidationError::InvalidAsset)
}

/// Fetch prices for both debt and collateral assets
fn get_liquidation_prices(
    env: &Env,
    debt_asset: &Option<Address>,
    collateral_asset: &Option<Address>,
) -> Result<(i128, i128), LiquidationError> {
    let d_price = if let Some(ref asset) = debt_asset {
        get_asset_price(env, asset)
    } else {
        get_asset_price(env, &get_native_asset_address(env)?)
    };

    let c_price = if let Some(ref asset) = collateral_asset {
        get_asset_price(env, asset)
    } else {
        get_asset_price(env, &get_native_asset_address(env)?)
    };

    if d_price <= 0 || c_price <= 0 {
        return Err(LiquidationError::PriceNotAvailable);
    }

    Ok((d_price, c_price))
}

/// Helper to fetch price from oracle.
fn get_asset_price(env: &Env, asset: &Address) -> i128 {
    get_price(env, asset).unwrap_or(0)
}

/// Helper to calculate current debt including interest since last accrual.
fn calculate_accrued_debt(env: &Env, position: &Position) -> Result<i128, LiquidationError> {
    let current_time = env.ledger().timestamp();
    let principal = position.debt;
    let stored_interest = position.borrow_interest;

    if principal == 0 {
        return Ok(stored_interest);
    }
    if current_time <= position.last_accrual_time {
        return Ok(principal
            .checked_add(stored_interest)
            .ok_or(LiquidationError::Overflow)?);
    }

    let rate_bps =
        crate::interest_rate::calculate_borrow_rate(env).map_err(|_| LiquidationError::Overflow)?;

    let delta_interest = crate::interest_rate::calculate_accrued_interest(
        principal,
        position.last_accrual_time,
        current_time,
        rate_bps,
    )
    .map_err(|_| LiquidationError::Overflow)?;

    principal
        .checked_add(stored_interest)
        .and_then(|v| v.checked_add(delta_interest))
        .ok_or(LiquidationError::Overflow)
}

/// # Liquidation: Debt Repayment and Collateral Seizure
///
/// This function allows a liquidator to repay a portion of a borrower's undercollateralized debt
/// in exchange for a discounted portion of their collateral.
///
/// # Logic and Economics
/// 1. Verifies position health (must be below liquidation threshold).
/// 2. Enforces close factor (maximum repayment per transaction).
/// 3. Calculates incentive-adjusted collateral to seize using I256 precision.
/// 4. Updates borrower state and global analytics.
/// 5. Transfers debt from liquidator and collateral to liquidator.
///
/// # Equations
/// - `max_repayable = total_debt * close_factor`
/// - `collateral_seized = (repaid_debt * debt_price * (1 + incentive) * 10^col_decimals) / (collateral_price * 10^debt_decimals)`
///
/// # Errors
/// * `InvalidAmount`: Debt amount <= 0.
/// * `LiquidationPaused`: Protocol or specific operation is paused.
/// * `NotLiquidatable`: Borrower position is healthy or non-existent.
/// * `PriceNotAvailable`: Oracle prices missing or invalid.
/// * `Overflow`: Mathematical overflow during precision scaling.
///
/// # Security
/// * Uses Checks-Effects-Interactions (CEI) to prevent reentrancy during cross-contract token transfers.
/// * Implements strict capping to ensure seized collateral never exceeds available borrower balance.
pub fn liquidate(
    env: &Env,
    liquidator: Address,
    borrower: Address,
    debt_asset: Option<Address>,
    collateral_asset: Option<Address>,
    debt_amount: i128,
) -> Result<(i128, i128, i128), LiquidationError> {
    // 1. Initial validation
    if debt_amount <= 0 {
        return Err(LiquidationError::InvalidAmount);
    }

    // Explicit authorization check for liquidator
    liquidator.require_auth();

    // 2. Authorization and Pause Checks
    if is_emergency_paused(env) {
        return Err(LiquidationError::LiquidationPaused);
    }

    require_operation_not_paused(env, Symbol::new(env, "pause_liquidate"))
        .map_err(|_| LiquidationError::LiquidationPaused)?;

    // 3. Load Borrower State
    let position_key = DepositDataKey::Position(borrower.clone());
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(LiquidationError::NotLiquidatable)?;

    // 4. Load Collateral State
    let collateral_key = DepositDataKey::CollateralBalance(borrower.clone());
    let borrower_collateral = env
        .storage()
        .persistent()
        .get::<DepositDataKey, i128>(&collateral_key)
        .unwrap_or(0);

    // 5. Fetch Prices and Decimals (Interactions - allowed here as they don't modify state)
    let (debt_price, collateral_price) =
        get_liquidation_prices(env, &debt_asset, &collateral_asset)?;
    let debt_decimals = get_asset_decimals(env, &debt_asset);
    let collateral_decimals = get_asset_decimals(env, &collateral_asset);

    // 6. ENFORCE HEALTH AND CLOSE FACTOR
    // Accrue debt up to current timestamp for accurate health assessment
    let current_total_debt = calculate_accrued_debt(env, &position)?;

    if !can_be_liquidated(env, borrower_collateral, current_total_debt).unwrap_or(false) {
        return Err(LiquidationError::NotLiquidatable);
    }

    let max_liquidatable = get_max_liquidatable_amount(env, current_total_debt)
        .map_err(|_| LiquidationError::Overflow)?;
    let actual_debt_liquidated = debt_amount.min(max_liquidatable).min(current_total_debt);

    if actual_debt_liquidated <= 0 {
        return Err(LiquidationError::InvalidAmount);
    }

    // 7. CALCULATE SEIZURE WITH PRECISION MATH
    // math: amount * price_debt * (10000 + incentive) * 10^col_decimals / (price_col * 10000 * 10^debt_decimals)

    let incentive_bps = get_risk_params(env)
        .map(|p| p.liquidation_incentive)
        .unwrap_or(1000);
    let bonus_multiplier = 10000i128
        .checked_add(incentive_bps)
        .ok_or(LiquidationError::Overflow)?;

    let amount_256 = I256::from_i128(env, actual_debt_liquidated);
    let debt_price_256 = I256::from_i128(env, debt_price);
    let bonus_multiplier_256 = I256::from_i128(env, bonus_multiplier);
    let collateral_price_256 = I256::from_i128(env, collateral_price);
    let bps_scale_256 = I256::from_i128(env, 10000);

    // Compute decimal scaling factor using powers of 10
    let debt_scale_val = 10i128.pow(debt_decimals);
    let col_scale_val = 10i128.pow(collateral_decimals);
    let debt_scale_256 = I256::from_i128(env, debt_scale_val);
    let col_scale_256 = I256::from_i128(env, col_scale_val);

    // Numerator: liquidated * price_debt * (10000 + incentive) * 10^col_decimals
    let numerator_256 = amount_256
        .mul(&debt_price_256)
        .mul(&bonus_multiplier_256)
        .mul(&col_scale_256);

    // Denominator: price_col * 10000 * 10^debt_decimals
    let denominator_256 = collateral_price_256
        .mul(&bps_scale_256)
        .mul(&debt_scale_256);

    let seized_256 = numerator_256.div(&denominator_256);
    let mut collateral_seized = seized_256.to_i128().ok_or(LiquidationError::Overflow)?;

    // Cap seizure at available collateral
    collateral_seized = collateral_seized.min(borrower_collateral);

    let incentive_amount =
        get_liquidation_incentive_amount(env, actual_debt_liquidated).unwrap_or(0);

    // 8. UPDATE STORAGE (EFFECTS)

    // Resolve Interest and Debt (mirroring repay_debt logic)
    // Interest is paid first, then principal.
    let total_interest_to_repay = position
        .borrow_interest
        .checked_add(
            current_total_debt
                .checked_sub(position.debt + position.borrow_interest)
                .unwrap_or(0),
        )
        .ok_or(LiquidationError::Overflow)?;

    if actual_debt_liquidated <= total_interest_to_repay {
        position.borrow_interest = total_interest_to_repay - actual_debt_liquidated;
    } else {
        let remaining_to_principal = actual_debt_liquidated - total_interest_to_repay;
        position.borrow_interest = 0;
        position.debt = position
            .debt
            .checked_sub(remaining_to_principal)
            .unwrap_or(0);
    }

    position.collateral = borrower_collateral
        .checked_sub(collateral_seized)
        .unwrap_or(0);
    position.last_accrual_time = env.ledger().timestamp();

    env.storage().persistent().set(&position_key, &position);
    env.storage()
        .persistent()
        .set(&collateral_key, &position.collateral);

    update_protocol_analytics(env, actual_debt_liquidated, collateral_seized)
        .map_err(|_| LiquidationError::Overflow)?;

    // 9. EXTERNAL INTERACTIONS (TRANSFERS)
    // Transfers are performed LAST to follow CEI pattern

    let debt_addr = match &debt_asset {
        Some(ref addr) => addr.clone(),
        None => get_native_asset_address(env)?,
    };
    let debt_client = TokenClient::new(env, &debt_addr);
    debt_client.transfer_from(
        &env.current_contract_address(),
        &liquidator,
        &env.current_contract_address(),
        &actual_debt_liquidated,
    );

    let col_addr = match &collateral_asset {
        Some(ref addr) => addr.clone(),
        None => get_native_asset_address(env)?,
    };
    let col_client = TokenClient::new(env, &col_addr);
    col_client.transfer(
        &env.current_contract_address(),
        &liquidator,
        &collateral_seized,
    );

    // 10. EMIT EVENTS
    emit_liquidation(
        env,
        LiquidationEvent {
            liquidator: liquidator.clone(),
            borrower: borrower.clone(),
            debt_asset: debt_asset.clone(),
            collateral_asset,
            debt_liquidated: actual_debt_liquidated,
            collateral_seized,
            incentive_amount,
            debt_price,
            collateral_price,
            timestamp: position.last_accrual_time,
        },
    );

    emit_position_updated_event(
        env,
        &borrower,
        &position,
        Symbol::new(env, "liquidate"),
        position.last_accrual_time,
    );
    add_activity_log(
        env,
        &borrower,
        Symbol::new(env, "liquidate"),
        actual_debt_liquidated,
        debt_asset.clone(),
        position.last_accrual_time,
    )
    .ok();

    Ok((actual_debt_liquidated, collateral_seized, incentive_amount))
}

/// Update protocol analytics after liquidation
fn update_protocol_analytics(
    env: &Env,
    debt_liquidated: i128,
    collateral_seized: i128,
) -> Result<(), LiquidationError> {
    let analytics_key = DepositDataKey::ProtocolAnalytics;
    let mut analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, ProtocolAnalytics>(&analytics_key)
        .unwrap_or(ProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    analytics.total_borrows = analytics
        .total_borrows
        .checked_sub(debt_liquidated)
        .unwrap_or(0);
    analytics.total_value_locked = analytics
        .total_value_locked
        .checked_sub(collateral_seized)
        .unwrap_or(0);

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}
