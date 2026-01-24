#![allow(unused)]
use soroban_sdk::{contracterror, Address, Env, IntoVal, Map, Symbol, Val, Vec};

use crate::deposit::{
    add_activity_log, emit_analytics_updated_event, emit_position_updated_event,
    emit_user_activity_tracked_event, update_protocol_analytics, update_user_analytics, Activity,
    DepositDataKey, Position, ProtocolAnalytics, UserAnalytics,
};

/// Errors that can occur during repay operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RepayError {
    /// Repay amount must be greater than zero
    InvalidAmount = 1,
    /// Asset address is invalid
    InvalidAsset = 2,
    /// Insufficient balance to repay
    InsufficientBalance = 3,
    /// Repay operations are currently paused
    RepayPaused = 4,
    /// No debt to repay
    NoDebt = 5,
    /// Overflow occurred during calculation
    Overflow = 6,
    /// Reentrancy detected
    Reentrancy = 7,
}

/// Annual interest rate in basis points (e.g., 500 = 5% per year)
/// This is a simple constant rate model - in production, this would be more sophisticated
const ANNUAL_INTEREST_RATE_BPS: i128 = 500; // 5% per year

/// Calculate interest accrued since last accrual time
/// Uses simple interest: interest = principal * rate * time
/// Rate is annual, so we need to convert time to years
fn calculate_accrued_interest(
    principal: i128,
    last_accrual_time: u64,
    current_time: u64,
) -> Result<i128, RepayError> {
    if principal == 0 {
        return Ok(0);
    }

    if current_time <= last_accrual_time {
        return Ok(0);
    }

    // Calculate time elapsed in seconds
    let time_elapsed = current_time
        .checked_sub(last_accrual_time)
        .ok_or(RepayError::Overflow)?;

    // Convert to years (assuming 365 days per year, 86400 seconds per day)
    // Use fixed-point arithmetic: multiply by 10000 to preserve precision
    let seconds_per_year: u64 = 365 * 86400; // 31,536,000 seconds

    // Calculate interest: principal * (rate / 10000) * (time_elapsed / seconds_per_year)
    // To avoid precision loss, we do: principal * rate * time_elapsed / (10000 * seconds_per_year)
    let rate_multiplier = ANNUAL_INTEREST_RATE_BPS as u64;
    let denominator = 10000_u64
        .checked_mul(seconds_per_year)
        .ok_or(RepayError::Overflow)?;

    // Calculate: principal * rate * time_elapsed / denominator
    let principal_u64 = principal as u64;
    let numerator = principal_u64
        .checked_mul(rate_multiplier)
        .and_then(|n| n.checked_mul(time_elapsed))
        .ok_or(RepayError::Overflow)?;

    let interest_u64 = numerator.checked_div(denominator).unwrap_or(0);
    Ok(interest_u64 as i128)
}

/// Accrue interest on a position
/// Updates the position's borrow_interest and last_accrual_time
fn accrue_interest(env: &Env, position: &mut Position) -> Result<(), RepayError> {
    let current_time = env.ledger().timestamp();

    if position.debt == 0 {
        position.borrow_interest = 0;
        position.last_accrual_time = current_time;
        return Ok(());
    }

    // Calculate new interest accrued
    let new_interest =
        calculate_accrued_interest(position.debt, position.last_accrual_time, current_time)?;

    // Add to existing interest
    position.borrow_interest = position
        .borrow_interest
        .checked_add(new_interest)
        .ok_or(RepayError::Overflow)?;

    // Update last accrual time
    position.last_accrual_time = current_time;

    Ok(())
}

/// Repay debt function
///
/// Allows users to repay their borrowed assets, reducing debt and accrued interest.
/// Supports both partial and full repayments.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address of the user repaying debt
/// * `asset` - The address of the asset contract to repay (None for native XLM)
/// * `amount` - The amount to repay
///
/// # Returns
/// Returns a tuple (remaining_debt, interest_paid, principal_paid)
///
/// # Errors
/// * `RepayError::InvalidAmount` - If amount is zero or negative
/// * `RepayError::InvalidAsset` - If asset address is invalid
/// * `RepayError::InsufficientBalance` - If user doesn't have enough balance
/// * `RepayError::RepayPaused` - If repayments are paused
/// * `RepayError::NoDebt` - If user has no debt to repay
/// * `RepayError::Overflow` - If calculation overflow occurs
///
/// # Security
/// * Validates repay amount > 0
/// * Checks pause switches
/// * Validates sufficient token balance
/// * Accrues interest before repayment
/// * Handles partial and full repayments
/// * Transfers tokens from user to contract
/// * Updates debt balances
/// * Emits events for tracking
/// * Updates analytics
pub fn repay_debt(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(i128, i128, i128), RepayError> {
    // Validate amount
    if amount <= 0 {
        return Err(RepayError::InvalidAmount);
    }

    // Check if repayments are paused
    let pause_switches_key = DepositDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Map<Symbol, bool>>(&pause_switches_key)
    {
        if let Some(paused) = pause_map.get(Symbol::new(env, "pause_repay")) {
            if paused {
                return Err(RepayError::RepayPaused);
            }
        }
    }

    // Get current timestamp
    let timestamp = env.ledger().timestamp();

    // Validate asset if provided
    if let Some(ref asset_addr) = asset {
        // Validate asset address - ensure it's not the contract itself
        if asset_addr == &env.current_contract_address() {
            return Err(RepayError::InvalidAsset);
        }
    }

    // Get user position
    let position_key = DepositDataKey::Position(user.clone());
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(RepayError::NoDebt)?;

    // Check if user has debt
    if position.debt == 0 && position.borrow_interest == 0 {
        return Err(RepayError::NoDebt);
    }

    // Accrue interest before repayment
    accrue_interest(env, &mut position)?;

    // Calculate total debt (principal + interest)
    let total_debt = position
        .debt
        .checked_add(position.borrow_interest)
        .ok_or(RepayError::Overflow)?;

    // Determine how much to repay
    let repay_amount = if amount >= total_debt {
        // Full repayment
        total_debt
    } else {
        // Partial repayment
        amount
    };

    // Handle asset transfer - user pays the contract
    if let Some(ref asset_addr) = asset {
        // Check user balance
        let token_client = soroban_sdk::token::Client::new(env, asset_addr);
        let user_balance = token_client.balance(&user);
        if user_balance < repay_amount {
            return Err(RepayError::InsufficientBalance);
        }

        // Transfer tokens from user to contract
        // The user must have approved the contract to spend their tokens
        token_client.transfer_from(
            &env.current_contract_address(), // spender (this contract)
            &user,                           // from (user)
            &env.current_contract_address(), // to (this contract)
            &repay_amount,
        );
    } else {
        // Native XLM repayment - in Soroban, native assets are handled differently
        // For now, we'll track it but actual XLM handling depends on Soroban's native asset support
        // This is a placeholder for native asset handling
    }

    // Calculate interest and principal portions
    // Interest is paid first, then principal
    let interest_paid = if repay_amount <= position.borrow_interest {
        repay_amount
    } else {
        position.borrow_interest
    };

    let principal_paid = repay_amount
        .checked_sub(interest_paid)
        .ok_or(RepayError::Overflow)?;

    // Update position
    position.borrow_interest = position
        .borrow_interest
        .checked_sub(interest_paid)
        .unwrap_or(0); // Should not underflow, but handle gracefully

    position.debt = position.debt.checked_sub(principal_paid).unwrap_or(0); // Should not underflow, but handle gracefully

    position.last_accrual_time = timestamp;

    // Save updated position
    env.storage().persistent().set(&position_key, &position);

    // Update user analytics
    update_user_analytics_repay(env, &user, repay_amount, timestamp)?;

    // Update protocol analytics
    update_protocol_analytics_repay(env, repay_amount)?;

    // Add to activity log
    add_activity_log(
        env,
        &user,
        Symbol::new(env, "repay"),
        repay_amount,
        asset.clone(),
        timestamp,
    )
    .map_err(|e| match e {
        crate::deposit::DepositError::Overflow => RepayError::Overflow,
        _ => RepayError::Overflow,
    })?;

    // Emit repay event
    emit_repay_event(
        env,
        &user,
        asset,
        repay_amount,
        interest_paid,
        principal_paid,
        timestamp,
    );

    // Emit position updated event
    emit_position_updated_event(env, &user, &position);

    // Emit analytics updated event
    emit_analytics_updated_event(env, &user, "repay", repay_amount, timestamp);

    // Emit user activity tracked event
    emit_user_activity_tracked_event(
        env,
        &user,
        Symbol::new(env, "repay"),
        repay_amount,
        timestamp,
    );

    // Return remaining debt, interest paid, and principal paid
    let remaining_debt = position
        .debt
        .checked_add(position.borrow_interest)
        .unwrap_or(0);
    Ok((remaining_debt, interest_paid, principal_paid))
}

/// Update user analytics after repayment
fn update_user_analytics_repay(
    env: &Env,
    user: &Address,
    amount: i128,
    timestamp: u64,
) -> Result<(), RepayError> {
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

    analytics.total_repayments = analytics
        .total_repayments
        .checked_add(amount)
        .ok_or(RepayError::Overflow)?;

    // Update debt value (subtract repayment)
    analytics.debt_value = analytics.debt_value.checked_sub(amount).unwrap_or(0); // Don't error on underflow, just set to 0

    // Recalculate collateralization ratio
    if analytics.debt_value > 0 && analytics.collateral_value > 0 {
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

/// Update protocol analytics after repayment
fn update_protocol_analytics_repay(env: &Env, amount: i128) -> Result<(), RepayError> {
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

    // Note: total_borrows doesn't decrease on repayment in this simple model
    // In a more sophisticated model, you might track active borrows separately
    // For now, we just update the analytics structure

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Emit repay event
fn emit_repay_event(
    env: &Env,
    user: &Address,
    asset: Option<Address>,
    amount: i128,
    interest_paid: i128,
    principal_paid: i128,
    timestamp: u64,
) {
    let topics = (Symbol::new(env, "repay"), user.clone());
    let mut data: Vec<Val> = Vec::new(env);
    data.push_back(Symbol::new(env, "user").into_val(env));
    data.push_back(user.clone().into_val(env));
    data.push_back(Symbol::new(env, "amount").into_val(env));
    data.push_back(amount.into_val(env));
    data.push_back(Symbol::new(env, "interest_paid").into_val(env));
    data.push_back(interest_paid.into_val(env));
    data.push_back(Symbol::new(env, "principal_paid").into_val(env));
    data.push_back(principal_paid.into_val(env));
    if let Some(asset_addr) = asset {
        data.push_back(Symbol::new(env, "asset").into_val(env));
        data.push_back(asset_addr.into_val(env));
    }
    data.push_back(Symbol::new(env, "timestamp").into_val(env));
    data.push_back(timestamp.into_val(env));

    env.events().publish(topics, data);
}
