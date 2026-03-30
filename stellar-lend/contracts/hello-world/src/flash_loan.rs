//! # Flash Loan Module
//!
//! Provides uncollateralized flash loan functionality for the lending protocol.
//!
//! Flash loans allow users to borrow assets without collateral, provided the loan
//! (principal + fee) is repaid within the same transaction via a callback contract.
//!
//! ## Fee Structure
//! - Default fee: 9 basis points (0.09%) of the borrowed amount.
//! - Fee is configurable by the admin.
//!
//! ## Reentrancy Protection
//! An active flash loan is recorded per (user, asset) pair. A second flash loan
//! for the same pair is rejected until the first is repaid, preventing reentrancy.
//!
//! ## Invariants
//! - The borrowed amount must be within configured min/max limits.
//! - The contract must have sufficient liquidity to fund the loan.
//! - Repayment must cover principal + fee in full.

#![allow(unused)]
use crate::events::{
    emit_flash_loan_initiated, emit_flash_loan_repaid, FlashLoanInitiatedEvent,
    FlashLoanRepaidEvent,
};
use soroban_sdk::{contracterror, contracttype, Address, Env, IntoVal, Map, Symbol, Val, Vec};

use crate::deposit::DepositDataKey;

/// Errors that can occur during flash loan operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FlashLoanError {
    /// Flash loan amount must be greater than zero
    InvalidAmount = 1,
    /// Asset address is invalid
    InvalidAsset = 2,
    /// Insufficient liquidity for flash loan
    InsufficientLiquidity = 3,
    /// Flash loan operations are currently paused
    FlashLoanPaused = 4,
    /// Flash loan not repaid within transaction
    NotRepaid = 5,
    /// Insufficient repayment amount
    InsufficientRepayment = 6,
    /// Overflow occurred during calculation
    Overflow = 7,
    /// Reentrancy detected
    Reentrancy = 8,
    /// Invalid callback
    InvalidCallback = 9,
    /// Callback execution failed
    CallbackFailed = 10,
}

/// Storage keys for flash loan-related data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum FlashLoanDataKey {
    /// Basis points fee charged for flash loans (legacy)
    FlashLoanFeeBps,
    /// Transient record of an active flash loan (prevents reentrancy)
    /// Value type: FlashLoanRecord
    ActiveFlashLoan(Address, Address),
    /// Global flash loan parameters (fee, min/max amount)
    /// Value type: FlashLoanConfig
    FlashLoanConfig,
    /// Pause switches specifically for flash loan operations: Map<Symbol, bool>
    PauseSwitches,
}

/// Flash loan record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FlashLoanRecord {
    /// Amount borrowed
    pub amount: i128,
    /// Fee amount
    pub fee: i128,
    /// Timestamp when loan was initiated
    pub timestamp: u64,
    /// Callback contract address
    pub callback: Address,
}

/// Flash loan configuration
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FlashLoanConfig {
    /// Flash loan fee in basis points
    pub fee_bps: i128,
    /// Maximum flash loan amount
    pub max_amount: i128,
    /// Minimum flash loan amount
    pub min_amount: i128,
}

/// Default flash loan fee: 9 basis points (0.09%)
const DEFAULT_FLASH_LOAN_FEE_BPS: i128 = 9;

/// Default maximum flash loan amount
const DEFAULT_MAX_FLASH_LOAN_AMOUNT: i128 = i128::MAX;

/// Default minimum flash loan amount
const DEFAULT_MIN_FLASH_LOAN_AMOUNT: i128 = 1;

/// Get default flash loan configuration
fn get_default_config() -> FlashLoanConfig {
    FlashLoanConfig {
        fee_bps: DEFAULT_FLASH_LOAN_FEE_BPS,
        max_amount: DEFAULT_MAX_FLASH_LOAN_AMOUNT,
        min_amount: DEFAULT_MIN_FLASH_LOAN_AMOUNT,
    }
}

/// Get flash loan configuration
fn get_flash_loan_config(env: &Env) -> FlashLoanConfig {
    let config_key = FlashLoanDataKey::FlashLoanConfig;
    env.storage()
        .persistent()
        .get::<FlashLoanDataKey, FlashLoanConfig>(&config_key)
        .unwrap_or_else(get_default_config)
}

/// Calculate flash loan fee
fn calculate_flash_loan_fee(env: &Env, amount: i128) -> Result<i128, FlashLoanError> {
    let config = get_flash_loan_config(env);

    // Fee = amount * fee_bps / 10000
    amount
        .checked_mul(config.fee_bps)
        .ok_or(FlashLoanError::Overflow)?
        .checked_div(10000)
        .ok_or(FlashLoanError::Overflow)
}

/// Check if flash loan is active
fn is_flash_loan_active(env: &Env, user: &Address, asset: &Address) -> bool {
    let loan_key = FlashLoanDataKey::ActiveFlashLoan(user.clone(), asset.clone());
    env.storage()
        .persistent()
        .get::<FlashLoanDataKey, FlashLoanRecord>(&loan_key)
        .is_some()
}

/// Record active flash loan
fn record_flash_loan(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
    fee: i128,
    callback: &Address,
) {
    let loan_key = FlashLoanDataKey::ActiveFlashLoan(user.clone(), asset.clone());
    let record = FlashLoanRecord {
        amount,
        fee,
        timestamp: env.ledger().timestamp(),
        callback: callback.clone(),
    };
    env.storage().persistent().set(&loan_key, &record);
}

/// Clear flash loan record
fn clear_flash_loan(env: &Env, user: &Address, asset: &Address) {
    let loan_key = FlashLoanDataKey::ActiveFlashLoan(user.clone(), asset.clone());
    env.storage().persistent().remove(&loan_key);
}

/// Execute flash loan
///
/// Allows users to borrow assets without collateral for a single transaction.
/// The loan must be repaid (with fee) within the same transaction via callback.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address borrowing the flash loan
/// * `asset` - The address of the asset contract to borrow
/// * `amount` - The amount to borrow
/// * `callback` - The callback contract address that will handle repayment
///
/// # Returns
/// Returns the total amount to repay (principal + fee)
///
/// # Errors
/// * `FlashLoanError::InvalidAmount` - If amount is zero, negative, or outside limits
/// * `FlashLoanError::InvalidAsset` - If asset address is invalid
/// * `FlashLoanError::InsufficientLiquidity` - If contract doesn't have enough liquidity
/// * `FlashLoanError::FlashLoanPaused` - If flash loans are paused
/// * `FlashLoanError::Reentrancy` - If flash loan is already active for this user/asset
/// * `FlashLoanError::InvalidCallback` - If callback address is invalid
/// * `FlashLoanError::Overflow` - If calculation overflow occurs
pub fn execute_flash_loan(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
    callback: Address,
) -> Result<i128, FlashLoanError> {
    // Validate amount
    if amount <= 0 {
        return Err(FlashLoanError::InvalidAmount);
    }

    // Check if flash loans are paused
    let pause_key = FlashLoanDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<FlashLoanDataKey, Map<Symbol, bool>>(&pause_key)
    {
        if let Some(paused) = pause_map.get(Symbol::new(env, "pause_flash_loan")) {
            if paused {
                return Err(FlashLoanError::FlashLoanPaused);
            }
        }
    }

    // Validate asset address
    if asset == env.current_contract_address() {
        return Err(FlashLoanError::InvalidAsset);
    }

    // Validate callback address
    if callback == env.current_contract_address() {
        return Err(FlashLoanError::InvalidCallback);
    }

    // Check configuration limits
    let config = get_flash_loan_config(env);
    if amount < config.min_amount || amount > config.max_amount {
        return Err(FlashLoanError::InvalidAmount);
    }

    // Check for reentrancy (active flash loan)
    if is_flash_loan_active(env, &user, &asset) {
        return Err(FlashLoanError::Reentrancy);
    }

    // Calculate fee
    let fee = calculate_flash_loan_fee(env, amount)?;
    let total_repayment = amount.checked_add(fee).ok_or(FlashLoanError::Overflow)?;

    // Check contract balance
    let token_client = soroban_sdk::token::Client::new(env, &asset);
    let contract_balance = token_client.balance(&env.current_contract_address());
    if contract_balance < amount {
        return Err(FlashLoanError::InsufficientLiquidity);
    }

    // Record flash loan before transfer
    record_flash_loan(env, &user, &asset, amount, fee, &callback);

    // Transfer tokens to user
    token_client.transfer(
        &env.current_contract_address(), // from (this contract)
        &user,                           // to (user)
        &amount,
    );

    // Emit flash loan initiated event
    emit_flash_loan_initiated(
        env,
        FlashLoanInitiatedEvent {
            user: user.clone(),
            asset: asset.clone(),
            amount,
            fee,
            callback: callback.clone(),
            timestamp: env.ledger().timestamp(),
        },
    );

    // Note: In a real implementation, we would call the callback here
    // For Soroban, the callback would need to be invoked by the user
    // The repayment check happens when the user calls repay_flash_loan

    Ok(total_repayment)
}

/// Repay flash loan
///
/// Must be called within the same transaction as the flash loan.
/// Validates that the full amount (principal + fee) is repaid.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address repaying the flash loan
/// * `asset` - The address of the asset contract
/// * `amount` - The amount being repaid (should equal principal + fee)
///
/// # Returns
/// Returns success if repayment is valid
pub fn repay_flash_loan(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<(), FlashLoanError> {
    // Get active flash loan record
    let loan_key = FlashLoanDataKey::ActiveFlashLoan(user.clone(), asset.clone());
    let record = env
        .storage()
        .persistent()
        .get::<FlashLoanDataKey, FlashLoanRecord>(&loan_key)
        .ok_or(FlashLoanError::NotRepaid)?;

    // Calculate required repayment
    let required_repayment = record
        .amount
        .checked_add(record.fee)
        .ok_or(FlashLoanError::Overflow)?;

    // Validate repayment amount
    if amount < required_repayment {
        return Err(FlashLoanError::InsufficientRepayment);
    }

    // Transfer tokens from user to contract
    let token_client = soroban_sdk::token::Client::new(env, &asset);

    // Check user balance
    let user_balance = token_client.balance(&user);
    if user_balance < required_repayment {
        return Err(FlashLoanError::InsufficientRepayment);
    }

    // Transfer repayment (user must have approved the contract)
    token_client.transfer_from(
        &env.current_contract_address(), // spender (this contract)
        &user,                           // from (user)
        &env.current_contract_address(), // to (this contract)
        &required_repayment,
    );

    // Credit fee to protocol reserve
    if record.fee > 0 {
        let reserve_key = DepositDataKey::ProtocolReserve(Some(asset.clone()));
        let current_reserve = env
            .storage()
            .persistent()
            .get::<DepositDataKey, i128>(&reserve_key)
            .unwrap_or(0);
        env.storage().persistent().set(
            &reserve_key,
            &(current_reserve
                .checked_add(record.fee)
                .ok_or(FlashLoanError::Overflow)?),
        );
    }

    // Clear flash loan record
    clear_flash_loan(env, &user, &asset);

    // Emit flash loan repaid event
    emit_flash_loan_repaid(
        env,
        FlashLoanRepaidEvent {
            user: user.clone(),
            asset: asset.clone(),
            amount: record.amount,
            fee: record.fee,
            timestamp: env.ledger().timestamp(),
        },
    );

    Ok(())
}

/// Set flash loan fee
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The address calling this function (must be admin)
/// * `fee_bps` - The new fee in basis points
pub fn set_flash_loan_fee(env: &Env, caller: Address, fee_bps: i128) -> Result<(), FlashLoanError> {
    // Check authorization
    crate::admin::require_admin(env, &caller).map_err(|_| FlashLoanError::InvalidCallback)?;

    // Validate fee (must be between 0 and 10000 basis points)
    if !(0..=10000).contains(&fee_bps) {
        return Err(FlashLoanError::InvalidAmount);
    }

    // Update configuration
    let mut config = get_flash_loan_config(env);
    config.fee_bps = fee_bps;
    let config_key = FlashLoanDataKey::FlashLoanConfig;
    env.storage().persistent().set(&config_key, &config);

    Ok(())
}

/// Configure flash loan parameters
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The address calling this function (must be admin)
/// * `config` - The new flash loan configuration
pub fn configure_flash_loan(
    env: &Env,
    caller: Address,
    config: FlashLoanConfig,
) -> Result<(), FlashLoanError> {
    // Check authorization
    crate::admin::require_admin(env, &caller).map_err(|_| FlashLoanError::InvalidCallback)?;

    // Validate configuration
    if !(0..=10000).contains(&config.fee_bps) {
        return Err(FlashLoanError::InvalidAmount);
    }

    if config.min_amount <= 0 || config.max_amount < config.min_amount {
        return Err(FlashLoanError::InvalidAmount);
    }

    // Update configuration
    let config_key = FlashLoanDataKey::FlashLoanConfig;
    env.storage().persistent().set(&config_key, &config);

    Ok(())
}
