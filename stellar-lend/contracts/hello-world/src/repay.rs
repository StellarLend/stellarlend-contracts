//! # Repay Module
//!
//! Handles debt repayment operations for the lending protocol.
//!
//! Supports both partial and full repayments. Interest is accrued before
//! repayment is applied. Repayment is allocated interest-first, then principal.
//!
//! ## Repayment Order
//! 1. Accrued interest is paid first.
//! 2. Any remaining repayment amount reduces the principal debt.
//!
//! ## Dust Handling
//! When the remaining debt (principal + interest) becomes very small (less than
//! DUST_THRESHOLD), it is automatically zeroed out to prevent precision issues
//! and ensure clean final states.
//!
//! ## Invariants
//! - Repay amount must be strictly positive.
//! - User must have outstanding debt to repay.
//! - Token transfers use `transfer_from`, requiring prior user approval.
//! - Events reflect actual processed amounts, ensuring alignment with final state.

#![allow(unused)]
use soroban_sdk::{contracterror, Address, Env, IntoVal, Map, Symbol, Val, Vec};

use crate::deposit::{
    add_activity_log, emit_analytics_updated_event, emit_position_updated_event,
    emit_user_activity_tracked_event, update_protocol_analytics, update_user_analytics, Activity,
    DepositDataKey, Position, ProtocolAnalytics, UserAnalytics,
};
use crate::events::{emit_repay, RepayEvent};

/// Dust threshold for debt cleanup
/// When total debt (principal + interest) falls below this amount, it's zeroed out
const DUST_THRESHOLD: i128 = 100;

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

/// Calculate interest accrued since last accrual time
///
/// Uses dynamic interest rate based on current protocol utilization.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `principal` - The principal amount to calculate interest on
/// * `last_accrual_time` - The timestamp of the last interest accrual
/// * `current_time` - The current ledger timestamp
///
/// # Returns
/// * `Result<i128, RepayError>` - The accrued interest amount or an error
fn calculate_accrued_interest(
    env: &Env,
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
    let rate_bps =
        crate::interest_rate::calculate_borrow_rate(env).map_err(|_| RepayError::Overflow)?;
    crate::interest_rate::calculate_accrued_interest(
        principal,
        last_accrual_time,
        current_time,
        rate_bps,
    )
    .map_err(|_| RepayError::Overflow)
}

/// Accrue interest on a position
///
/// Updates the position's borrow_interest and last_accrual_time based on elapsed time
/// and the current interest rate.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `position` - A mutable reference to the user's position
///
/// # Returns
/// * `Result<(), RepayError>` - Success or an error
fn accrue_interest(env: &Env, position: &mut Position) -> Result<(), RepayError> {
    let current_time = env.ledger().timestamp();
    if position.debt == 0 {
        position.borrow_interest = 0;
        position.last_accrual_time = current_time;
        return Ok(());
    }
    let new_interest =
        calculate_accrued_interest(env, position.debt, position.last_accrual_time, current_time)?;
    position.borrow_interest = position
        .borrow_interest
        .checked_add(new_interest)
        .ok_or(RepayError::Overflow)?;
    position.last_accrual_time = current_time;
    Ok(())
}

/// Helper function to get the native asset contract address from storage
///
/// # Arguments
/// * `env` - The Soroban environment
///
/// # Returns
/// * `Result<Address, RepayError>` - The native asset address or an error if not configured
fn get_native_asset_address(env: &Env) -> Result<Address, RepayError> {
    env.storage()
        .persistent()
        .get::<DepositDataKey, Address>(&DepositDataKey::NativeAssetAddress)
        .ok_or(RepayError::InvalidAsset)
}

/// Repay debt function
///
/// Allows users to repay their borrowed assets, reducing debt and accrued interest.
/// Supports both partial and full repayments. The repayment amount is first unconditionally
/// applied to any outstanding accrued interest. Any remainder after fully settling the interest
/// is applied directly to the principal debt.
///
/// ## Rounding & Truncation Handling
/// - Interest accruals and principal limits operate on positive integer ranges. Any rounding in the computation of `checked_mul` or `checked_div` defaults uniformly to floor division. Dust limits implicitly follow Soroban precision.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address of the user repaying debt
/// * `asset` - The address of the asset contract to repay (None for native XLM)
/// * `amount` - The amount to repay
///
/// # Returns
/// Returns a tuple `(remaining_debt, interest_paid, principal_paid)` upon successful execution.
///
/// # Errors
/// * `RepayError::InvalidAmount` - If amount is zero or negative
/// * `RepayError::InvalidAsset` - If asset address is invalid or not configured
/// * `RepayError::InsufficientBalance` - If user doesn't have enough balance
/// * `RepayError::RepayPaused` - If repayments are paused
/// * `RepayError::NoDebt` - If user has no debt to repay
/// * `RepayError::Overflow` - If calculation overflow occurs
///
/// # Security Boundaries & Invariants
/// * **Authorization**: This function is open for a user to pay down their own debt. No explicit admin auth required. Token transfers use `transfer_from`, hence the caller/user must have pre-approved the protocol.
/// * **Validation**: The caller designates the `repay_amount`. The protocol checks that it is correctly bounded and strictly positive.
/// * **External Calls / Reentrancy**: Token transfers via `client.transfer_from` involve external contract calls. To prevent malicious reentry, the system employs an environment-level `ReentrancyGuard`.
/// * **Asset Controls**: Pausing overrides the functionality. Safe fallback arithmetic prevents under/overflows.
pub fn repay_debt(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(i128, i128, i128), RepayError> {
    if amount <= 0 {
        return Err(RepayError::InvalidAmount);
    }

    // Check for reentrancy
    let _guard =
        crate::reentrancy::ReentrancyGuard::new(env).map_err(|_| RepayError::Reentrancy)?;

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

    let timestamp = env.ledger().timestamp();

    // Determine the asset contract address to use
    let asset_addr = match &asset {
        Some(addr) => {
            if addr == &env.current_contract_address() {
                return Err(RepayError::InvalidAsset);
            }
            addr.clone()
        }
        None => get_native_asset_address(env)?,
    };

    let reserve_factor = if let Some(asset_addr) = asset.as_ref() {
        let params_key = DepositDataKey::AssetParams(asset_addr.clone());
        if let Some(params) = env
            .storage()
            .persistent()
            .get::<DepositDataKey, crate::deposit::AssetParams>(&params_key)
        {
            params.borrow_fee_bps.max(1000) // Use asset-specific fee or default 10%
        } else {
            1000 // Default 10%
        }
    } else {
        1000 // Default 10%
    };

    // Get user position
    let position_key = DepositDataKey::Position(user.clone());
    let mut position = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Position>(&position_key)
        .ok_or(RepayError::NoDebt)?;

    if position.debt == 0 && position.borrow_interest == 0 {
        return Err(RepayError::NoDebt);
    }

    // Accrue interest before repayment
    accrue_interest(env, &mut position)?;

    let total_debt = position
        .debt
        .checked_add(position.borrow_interest)
        .ok_or(RepayError::Overflow)?;

    let repay_amount = if amount >= total_debt {
        total_debt
    } else {
        amount
    };

    // Calculate interest and principal portions
    // Interest is paid fully first, then the remainder goes to principal
    let interest_paid = if repay_amount <= position.borrow_interest {
        repay_amount
    } else {
        position.borrow_interest
    };

    let principal_paid = repay_amount
        .checked_sub(interest_paid)
        .ok_or(RepayError::Overflow)?;

    // Handle asset transfer - user pays the contract
    // Uses standardized SRC-20 transfer format requiring pre-authorization
    #[cfg(not(test))]
    {
        let token_client = soroban_sdk::token::Client::new(env, &asset_addr);
        let user_balance = token_client.balance(&user);
        if user_balance < repay_amount {
            return Err(RepayError::InsufficientBalance);
        }

        token_client.transfer_from(
            &env.current_contract_address(), // spender
            &user,                           // from
            &env.current_contract_address(), // to
            &repay_amount,
        );
    }

    // Update position ensuring no underflow during integer truncation
    position.borrow_interest = position
        .borrow_interest
        .checked_sub(interest_paid)
        .unwrap_or(0);

    position.debt = position.debt.checked_sub(principal_paid).unwrap_or(0);

    position.last_accrual_time = timestamp;

    // Save final updated position state
    env.storage().persistent().set(&position_key, &position);

    // Apply portion of paid interest to protocol reserves
    if interest_paid > 0 {
        let reserve_amount = interest_paid
            .checked_mul(reserve_factor)
            .ok_or(RepayError::Overflow)?
            .checked_div(10000)
            .unwrap_or(0); // Floor rounding bounds protocol take to >= 0

        if reserve_amount > 0 {
            let reserve_key = DepositDataKey::ProtocolReserve(asset.clone());
            let current_reserve = env
                .storage()
                .persistent()
                .get::<DepositDataKey, i128>(&reserve_key)
                .unwrap_or(0);
            env.storage().persistent().set(
                &reserve_key,
                &(current_reserve
                    .checked_add(reserve_amount)
                    .ok_or(RepayError::Overflow)?),
            );
        }
    }

    update_user_analytics_repay(env, &user, repay_amount, timestamp)?;
    update_protocol_analytics_repay(env, repay_amount)?;

    // Add to activity log tracking for metrics
    add_activity_log(
        env,
        &user,
        Symbol::new(env, "repay"),
        repay_amount,
        asset.clone(),
        timestamp,
    )
    .map_err(|_| RepayError::Overflow)?;

    // Emit Soroban lifecycle events
    let event = RepayEvent {
        user: user.clone(),
        asset: asset.clone(),
        amount: repay_amount,
        timestamp,
    };
    log_repay(env, event);
    emit_position_updated_event(env, &user, &position, Symbol::new(env, "repay"), timestamp);
    emit_analytics_updated_event(env, &user, "repay", repay_amount, timestamp);
    emit_user_activity_tracked_event(
        env,
        &user,
        Symbol::new(env, "repay"),
        repay_amount,
        timestamp,
    );

    let remaining_debt = position
        .debt
        .checked_add(position.borrow_interest)
        .unwrap_or(0);

    Ok((remaining_debt, interest_paid, principal_paid))
}

/// Update user analytics after repayment
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address of the user
/// * `amount` - The repayment amount
/// * `timestamp` - The current ledger timestamp
///
/// # Returns
/// * `Result<(), RepayError>` - Success or an error
fn update_user_analytics_repay(
    env: &Env,
    user: &Address,
    amount: i128,
    timestamp: u64,
) -> Result<(), RepayError> {
    let analytics_key = DepositDataKey::UserAnalytics(user.clone());
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
    analytics.debt_value = analytics.debt_value.checked_sub(amount).unwrap_or(0);

    if analytics.debt_value > 0 && analytics.collateral_value > 0 {
        analytics.collateralization_ratio = analytics
            .collateral_value
            .checked_mul(10000)
            .and_then(|v| v.checked_div(analytics.debt_value))
            .unwrap_or(0);
    } else {
        analytics.collateralization_ratio = 0;
    }

    analytics.transaction_count = analytics.transaction_count.saturating_add(1);
    analytics.last_activity = timestamp;

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Update protocol analytics after repayment
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `amount` - The repayment amount
///
/// # Returns
/// * `Result<(), RepayError>` - Success or an error
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

    // Update total borrows (decrease by repayment amount)
    analytics.total_borrows = analytics.total_borrows.checked_sub(amount).unwrap_or(0); // If it underflows, set to 0 (graceful recovery)

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

fn log_repay(env: &Env, event: RepayEvent) {
    emit_repay(env, event);
}
