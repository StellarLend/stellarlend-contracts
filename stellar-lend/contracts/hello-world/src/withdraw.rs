#![allow(unused)]
use soroban_sdk::{contracterror, Address, Env, IntoVal, Map, Symbol, Val, Vec};

use crate::deposit::{
    add_activity_log, emit_analytics_updated_event, emit_position_updated_event,
    emit_user_activity_tracked_event, update_protocol_analytics, update_user_analytics, Activity,
    AssetParams, DepositDataKey, Position, ProtocolAnalytics, UserAnalytics,
};

/// Errors that can occur during withdraw operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum WithdrawError {
    /// Withdraw amount must be greater than zero
    InvalidAmount = 1,
    /// Asset address is invalid
    InvalidAsset = 2,
    /// Insufficient collateral balance
    InsufficientCollateral = 3,
    /// Withdraw operations are currently paused
    WithdrawPaused = 4,
    /// Withdrawal would violate minimum collateral ratio
    InsufficientCollateralRatio = 5,
    /// Overflow occurred during calculation
    Overflow = 6,
    /// Reentrancy detected
    Reentrancy = 7,
    /// Position would become undercollateralized
    Undercollateralized = 8,
}

/// Minimum collateral ratio (in basis points, e.g., 15000 = 150%)
/// This is the minimum ratio required: collateral_value / debt_value >= 1.5
const MIN_COLLATERAL_RATIO_BPS: i128 = 15000; // 150%

/// Calculate collateral ratio
/// Returns (collateral_value * collateral_factor) / (debt + interest)
/// Returns None if debt is zero (infinite ratio)
fn calculate_collateral_ratio(
    collateral: i128,
    debt: i128,
    interest: i128,
    collateral_factor: i128,
) -> Option<i128> {
    let total_debt = debt.checked_add(interest)?;
    if total_debt == 0 {
        return None; // No debt means infinite ratio
    }

    // collateral_value = collateral * collateral_factor / 10000 (basis points)
    let collateral_value = collateral
        .checked_mul(collateral_factor)?
        .checked_div(10000)?;

    // ratio = (collateral_value * 10000) / total_debt (in basis points)
    collateral_value.checked_mul(10000)?.checked_div(total_debt)
}

/// Check if withdrawal would violate minimum collateral ratio
fn validate_collateral_ratio_after_withdraw(
    env: &Env,
    user: &Address,
    withdraw_amount: i128,
    asset: Option<&Address>,
) -> Result<(), WithdrawError> {
    // Get user position
    let position_key = DepositDataKey::Position(user.clone());
    let position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(WithdrawError::InsufficientCollateral)?;

    // If no debt, withdrawal is always allowed (as long as sufficient collateral)
    if position.debt == 0 && position.borrow_interest == 0 {
        return Ok(());
    }

    // Get current collateral balance
    let collateral_key = DepositDataKey::CollateralBalance(user.clone());
    let current_collateral = env
        .storage()
        .persistent()
        .get::<DepositDataKey, i128>(&collateral_key)
        .unwrap_or(0);

    // Calculate new collateral after withdrawal
    let new_collateral = current_collateral
        .checked_sub(withdraw_amount)
        .ok_or(WithdrawError::InsufficientCollateral)?;

    // Get asset parameters for collateral factor
    // Default collateral factor if asset params not found
    let collateral_factor = if let Some(asset_addr) = asset {
        let asset_params_key = DepositDataKey::AssetParams(asset_addr.clone());
        if let Some(params) = env
            .storage()
            .persistent()
            .get::<DepositDataKey, AssetParams>(&asset_params_key)
        {
            params.collateral_factor
        } else {
            10000 // Default 100% if not configured
        }
    } else {
        10000 // Default 100% for native XLM
    };

    // Calculate total debt (debt + accrued interest)
    let total_debt = position
        .debt
        .checked_add(position.borrow_interest)
        .ok_or(WithdrawError::Overflow)?;

    // Calculate new collateral ratio
    if let Some(new_ratio) = calculate_collateral_ratio(
        new_collateral,
        position.debt,
        position.borrow_interest,
        collateral_factor,
    ) {
        if new_ratio < MIN_COLLATERAL_RATIO_BPS {
            return Err(WithdrawError::InsufficientCollateralRatio);
        }
    } else {
        // If ratio calculation returns None, it means no debt, which is already handled above
        // This shouldn't happen, but handle it gracefully
        return Ok(());
    }

    Ok(())
}

/// Withdraw collateral from the protocol
///
/// Allows users to withdraw their deposited collateral, subject to:
/// - Sufficient collateral balance
/// - Minimum collateral ratio requirements
/// - Pause switch checks
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address of the user withdrawing collateral
/// * `asset` - The address of the asset contract to withdraw (None for native XLM)
/// * `amount` - The amount to withdraw
///
/// # Returns
/// Returns the updated collateral balance for the user
///
/// # Errors
/// * `WithdrawError::InvalidAmount` - If amount is zero or negative
/// * `WithdrawError::InvalidAsset` - If asset address is invalid
/// * `WithdrawError::InsufficientCollateral` - If user doesn't have enough collateral
/// * `WithdrawError::WithdrawPaused` - If withdrawals are paused
/// * `WithdrawError::InsufficientCollateralRatio` - If withdrawal would violate minimum ratio
/// * `WithdrawError::Overflow` - If calculation overflow occurs
///
/// # Security
/// * Validates withdraw amount > 0
/// * Checks pause switches
/// * Validates sufficient collateral balance
/// * Enforces minimum collateral ratio
/// * Transfers tokens from contract to user
/// * Updates collateral balances
/// * Emits events for tracking
/// * Updates analytics
pub fn withdraw_collateral(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, WithdrawError> {
    // Validate amount
    if amount <= 0 {
        return Err(WithdrawError::InvalidAmount);
    }

    // Check if withdrawals are paused
    let pause_switches_key = DepositDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Map<Symbol, bool>>(&pause_switches_key)
    {
        if let Some(paused) = pause_map.get(Symbol::new(env, "pause_withdraw")) {
            if paused {
                return Err(WithdrawError::WithdrawPaused);
            }
        }
    }

    // Get current timestamp
    let timestamp = env.ledger().timestamp();

    // Validate asset if provided
    if let Some(ref asset_addr) = asset {
        // Validate asset address - ensure it's not the contract itself
        if asset_addr == &env.current_contract_address() {
            return Err(WithdrawError::InvalidAsset);
        }
    }

    // Get current collateral balance
    let collateral_key = DepositDataKey::CollateralBalance(user.clone());
    let current_collateral = env
        .storage()
        .persistent()
        .get::<DepositDataKey, i128>(&collateral_key)
        .unwrap_or(0);

    // Check sufficient collateral
    if current_collateral < amount {
        return Err(WithdrawError::InsufficientCollateral);
    }

    // Validate collateral ratio after withdrawal
    validate_collateral_ratio_after_withdraw(env, &user, amount, asset.as_ref())?;

    // Calculate new collateral balance
    let new_collateral = current_collateral
        .checked_sub(amount)
        .ok_or(WithdrawError::Overflow)?;

    // Update storage
    env.storage()
        .persistent()
        .set(&collateral_key, &new_collateral);

    // Get or update user position
    let position_key = DepositDataKey::Position(user.clone());
    #[allow(clippy::unnecessary_lazy_evaluations)]
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .unwrap_or_else(|| Position {
            collateral: 0,
            debt: 0,
            borrow_interest: 0,
            last_accrual_time: timestamp,
        });

    // Update position
    position.collateral = new_collateral;
    position.last_accrual_time = timestamp;
    env.storage().persistent().set(&position_key, &position);

    // Handle asset transfer
    if let Some(ref asset_addr) = asset {
        // Transfer tokens from contract to user
        let token_client = soroban_sdk::token::Client::new(env, asset_addr);
        token_client.transfer(
            &env.current_contract_address(), // from (this contract)
            &user,                           // to (user)
            &amount,
        );
    } else {
        // Native XLM withdrawal - in Soroban, native assets are handled differently
        // For now, we'll track it but actual XLM handling depends on Soroban's native asset support
        // This is a placeholder for native asset handling
    }

    // Update user analytics
    update_user_analytics_withdraw(env, &user, amount, timestamp)?;

    // Update protocol analytics
    update_protocol_analytics_withdraw(env, amount)?;

    // Add to activity log
    add_activity_log(
        env,
        &user,
        Symbol::new(env, "withdraw"),
        amount,
        asset.clone(),
        timestamp,
    )
    .map_err(|e| match e {
        crate::deposit::DepositError::Overflow => WithdrawError::Overflow,
        _ => WithdrawError::Overflow,
    })?;

    // Emit withdraw event
    emit_withdraw_event(env, &user, asset, amount, timestamp);

    // Emit position updated event
    emit_position_updated_event(env, &user, &position);

    // Emit analytics updated event
    emit_analytics_updated_event(env, &user, "withdraw", amount, timestamp);

    // Emit user activity tracked event
    emit_user_activity_tracked_event(env, &user, Symbol::new(env, "withdraw"), amount, timestamp);

    Ok(new_collateral)
}

/// Update user analytics after withdrawal
fn update_user_analytics_withdraw(
    env: &Env,
    user: &Address,
    amount: i128,
    timestamp: u64,
) -> Result<(), WithdrawError> {
    let analytics_key = DepositDataKey::UserAnalytics(user.clone());
    #[allow(clippy::unnecessary_lazy_evaluations)]
    let mut analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, UserAnalytics>(&analytics_key)
        .unwrap_or_else(|| UserAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_withdrawals: 0,
            total_repayments: 0,
            collateral_value: 0,
            debt_value: 0,
            collateralization_ratio: 0,
            activity_score: 0,
            transaction_count: 0,
            first_interaction: timestamp,
            last_activity: timestamp,
            risk_level: 0,
            loyalty_tier: 0,
        });

    analytics.total_withdrawals = analytics
        .total_withdrawals
        .checked_add(amount)
        .ok_or(WithdrawError::Overflow)?;

    // Update collateral value (subtract withdrawal)
    analytics.collateral_value = analytics.collateral_value.checked_sub(amount).unwrap_or(0); // Don't error on underflow, just set to 0

    // Recalculate collateralization ratio
    if analytics.debt_value > 0 {
        analytics.collateralization_ratio = analytics
            .collateral_value
            .checked_mul(10000)
            .and_then(|v| v.checked_div(analytics.debt_value))
            .unwrap_or(0);
    } else {
        analytics.collateralization_ratio = 0; // No debt means no ratio
    }

    analytics.transaction_count = analytics.transaction_count.saturating_add(1);
    analytics.last_activity = timestamp;

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Update protocol analytics after withdrawal
fn update_protocol_analytics_withdraw(env: &Env, amount: i128) -> Result<(), WithdrawError> {
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

    // Update total value locked (subtract withdrawal)
    analytics.total_value_locked = analytics
        .total_value_locked
        .checked_sub(amount)
        .unwrap_or(0); // Don't error on underflow, just set to 0

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Emit withdraw event
fn emit_withdraw_event(
    env: &Env,
    user: &Address,
    asset: Option<Address>,
    amount: i128,
    timestamp: u64,
) {
    let topics = (Symbol::new(env, "withdraw"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount").into_val(env));
    data.push_back(amount.into_val(env));
    if let Some(asset_addr) = asset {
        data.push_back(Symbol::new(env, "asset").into_val(env));
        data.push_back(asset_addr.into_val(env));
    }
    data.push_back(Symbol::new(env, "timestamp").into_val(env));
    data.push_back(timestamp.into_val(env));

    env.events().publish(topics, data);
}
