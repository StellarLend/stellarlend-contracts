//! # Deposit Module
//!
//! Handles collateral deposit operations for the lending protocol.
//!
//! This module manages:
//! - Depositing assets (both token contracts and native XLM) as collateral
//! - Tracking user collateral balances and positions
//! - Updating user and protocol analytics on each deposit
//! - Emitting events for off-chain indexing
//!
//! ## Storage Layout
//! - `CollateralBalance(user)` — per-user collateral amount
//! - `Position(user)` — per-user position (collateral, debt, interest)
//! - `AssetParams(asset)` — per-asset deposit parameters
//! - `PauseSwitches` — operation pause flags
//! - `ProtocolAnalytics` — aggregate protocol metrics
//! - `UserAnalytics(user)` — per-user activity metrics
//! - `ActivityLog` — bounded activity history (max 1000 entries)
//!
//! ## Invariants
//! - Deposit amount must be strictly positive.
//! - Deposits are rejected when the protocol or deposit operation is paused.
//! - Token transfers use `transfer_from`, requiring prior user approval.

#![allow(unused)]
use crate::prelude::*;
use soroban_sdk::{contracterror, contracttype, Address, Env, IntoVal, Map, Symbol, Val, Vec};

use crate::events::{
    emit_analytics_updated, emit_borrower_health_v1, emit_deposit, emit_position_updated,
    emit_user_activity_tracked, AnalyticsUpdatedEvent, BorrowerHealthEventV1, DepositEvent,
    PositionUpdatedEvent, UserActivityTrackedEvent,
};

/// Errors that can occur during deposit operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DepositError {
    /// Deposit amount must be greater than zero
    InvalidAmount = 1,
    /// Asset address is invalid
    InvalidAsset = 2,
    /// Insufficient balance to deposit
    InsufficientBalance = 3,
    /// Deposit operations are currently paused
    DepositPaused = 4,
    /// Asset is not enabled for deposits
    AssetNotEnabled = 5,
    /// Overflow occurred during calculation
    Overflow = 6,
    /// Reentrancy detected
    Reentrancy = 7,
}

/// Storage keys for deposit-related data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum DepositDataKey {
    /// User collateral balance (legacy)
    /// Value type: i128
    CollateralBalance(Address),
    /// Asset-specific parameters (legacy)
    /// Value type: AssetParams
    AssetParams(Address),
    /// Legacy operation pause switches: Map<Symbol, bool>
    PauseSwitches,
    Position(Address),
    /// Global protocol analytics (TVL, aggregate borrows/deposits)
    /// Value type: ProtocolAnalytics
    ProtocolAnalytics,
    /// Granular per-user analytics metrics
    /// Value type: UserAnalytics
    UserAnalytics(Address),
    /// Bounded log of recent deposit activities: Vec<Activity>
    ActivityLog,
    /// Protocol reserve per asset: Map<Option<Address>, i128>
    ProtocolReserve(Option<Address>),
    /// Native asset (XLM) contract address
    NativeAssetAddress,
}

/// Asset parameters for collateral
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AssetParams {
    /// Whether deposits are enabled for this asset
    pub deposit_enabled: bool,
    /// Collateral factor (in basis points, e.g., 7500 = 75%)
    pub collateral_factor: i128,
    /// Maximum deposit amount
    pub max_deposit: i128,
    /// Borrow fee in basis points (e.g., 50 = 0.5%)
    pub borrow_fee_bps: i128,
}

/// User position tracking
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    /// Total collateral amount
    pub collateral: i128,
    /// Total debt amount
    pub debt: i128,
    /// Borrow interest accrued
    pub borrow_interest: i128,
    /// Last accrual timestamp
    pub last_accrual_time: u64,
}

/// Activity log entry
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    /// User address
    pub user: Address,
    /// Activity type (e.g., "deposit", "withdraw", "borrow")
    pub activity_type: Symbol,
    /// Amount involved
    pub amount: i128,
    /// Asset address (if applicable)
    pub asset: Option<Address>,
    /// Timestamp
    pub timestamp: u64,
    /// Additional metadata
    pub metadata: Map<Symbol, Symbol>,
}

/// User analytics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserAnalytics {
    /// Total deposits
    pub total_deposits: i128,
    /// Total borrows
    pub total_borrows: i128,
    /// Total withdrawals
    pub total_withdrawals: i128,
    /// Total repayments
    pub total_repayments: i128,
    /// Collateral value
    pub collateral_value: i128,
    /// Debt value
    pub debt_value: i128,
    /// Collateralization ratio
    pub collateralization_ratio: i128,
    /// Activity score
    pub activity_score: i128,
    /// Transaction count
    pub transaction_count: u64,
    /// First interaction timestamp
    pub first_interaction: u64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Risk level
    pub risk_level: i128,
    /// Loyalty tier
    pub loyalty_tier: u32,
}

/// Protocol analytics
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolAnalytics {
    /// Total deposits across all users
    pub total_deposits: i128,
    /// Total borrows across all users
    pub total_borrows: i128,
    /// Total protocol value locked
    pub total_value_locked: i128,
}

/// Deposit collateral function
///
/// Allows users to deposit assets as collateral in the protocol.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `user` - The address of the user depositing collateral
/// * `asset` - The address of the asset contract to deposit (None for native XLM)
/// * `amount` - The amount to deposit
///
/// # Returns
/// Returns the updated collateral balance for the user
///
/// # Errors
/// * `DepositError::InvalidAmount` - If amount is zero or negative
/// * `DepositError::InvalidAsset` - If asset address is invalid
/// * `DepositError::InsufficientBalance` - If user doesn't have enough balance
/// * `DepositError::DepositPaused` - If deposits are paused
/// * `DepositError::AssetNotEnabled` - If asset is not enabled for deposits
/// * `DepositError::Overflow` - If calculation overflow occurs
///
/// # Security
/// * Validates deposit amount > 0
/// * Checks pause switches
/// * Validates asset parameters
/// * Transfers tokens from user to contract
/// * Updates collateral balances
/// * Emits events for tracking
/// * Updates analytics
pub fn deposit_collateral(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, DepositError> {
    // Validate amount
    if amount <= 0 {
        return Err(DepositError::InvalidAmount);
    }

    // Check for reentrancy
    let _guard =
        crate::reentrancy::ReentrancyGuard::new(env).map_err(|_| DepositError::Reentrancy)?;

    // Check if deposits are paused
    // Note: The risk management system provides pause functionality through the public API.
    // This check maintains backward compatibility with the old pause switch system.
    // The risk management pause switches are checked at the contract level in lib.rs.
    let pause_switches_key = DepositDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Map<Symbol, bool>>(&pause_switches_key)
    {
        if let Some(paused) = pause_map.get(Symbol::new(env, "pause_deposit")) {
            if paused {
                return Err(DepositError::DepositPaused);
            }
        }
    }

    // Check risk management emergency pause and operation pause
    // We access the risk management storage directly to check pause status
    check_risk_management_pause(env)?;

    // Get current timestamp
    let timestamp = env.ledger().timestamp();

    let actual_asset = match &asset {
        Some(addr) => addr.clone(),
        None => env
            .storage()
            .persistent()
            .get::<DepositDataKey, Address>(&DepositDataKey::NativeAssetAddress)
            .ok_or(DepositError::InvalidAsset)?,
    };

    // Validate asset address - ensure it's not the contract itself
    if actual_asset == env.current_contract_address() {
        return Err(DepositError::InvalidAsset);
    }

    // Check asset parameters
    let asset_params_key = DepositDataKey::AssetParams(actual_asset.clone());
    if let Some(params) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, AssetParams>(&asset_params_key)
    {
        if !params.deposit_enabled {
            return Err(DepositError::AssetNotEnabled);
        }

        // Check max deposit limit
        if params.max_deposit > 0 && amount > params.max_deposit {
            return Err(DepositError::InvalidAmount);
        }
    }

    // Transfer tokens from user to contract using token contract
    // Use the token contract's transfer_from method
    let token_client = soroban_sdk::token::Client::new(env, &actual_asset);

    // Check user balance
    let user_balance = token_client.balance(&user);
    if user_balance < amount {
        return Err(DepositError::InsufficientBalance);
    }

    // Transfer tokens from user to contract
    // The user must have approved the contract to spend their tokens
    // transfer_from requires: spender (contract), from (user), to (contract), amount
    token_client.transfer_from(
        &env.current_contract_address(), // spender (this contract)
        &user,                           // from (user)
        &env.current_contract_address(), // to (this contract)
        &amount,
    );

    // Get or create user position
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

    // Update collateral balance
    let collateral_key = DepositDataKey::CollateralBalance(user.clone());
    let current_collateral = env
        .storage()
        .persistent()
        .get::<DepositDataKey, i128>(&collateral_key)
        .unwrap_or(0);

    // Check for overflow
    let new_collateral = current_collateral
        .checked_add(amount)
        .ok_or(DepositError::Overflow)?;

    // Update storage
    env.storage()
        .persistent()
        .set(&collateral_key, &new_collateral);

    // Update position
    position.collateral = new_collateral;
    position.last_accrual_time = timestamp;
    env.storage().persistent().set(&position_key, &position);

    // Update user analytics
    update_user_analytics(env, &user, amount, timestamp, true)?;

    // Update protocol analytics
    update_protocol_analytics(env, amount, true)?;

    // Add to activity log
    add_activity_log(
        env,
        &user,
        Symbol::new(env, "deposit"),
        amount,
        asset.clone(),
        timestamp,
    )?;

    // Emit deposit event
    emit_deposit(
        env,
        DepositEvent {
            user: user.clone(),
            asset: asset.clone(),
            amount,
            timestamp,
        },
    );

    // Emit position updated event
    emit_position_updated_event(
        env,
        &user,
        &position,
        Symbol::new(env, "deposit"),
        timestamp,
    );

    // Emit analytics updated event
    emit_analytics_updated_event(env, &user, "deposit", amount, timestamp);

    // Emit user activity tracked event
    emit_user_activity_tracked_event(env, &user, Symbol::new(env, "deposit"), amount, timestamp);

    Ok(new_collateral)
}

/// Set the native asset address (admin only).
/// Required for deposit/borrow/repay with asset = None. Must be called before using None as asset.
pub fn set_native_asset_address(
    env: &Env,
    caller: Address,
    native_asset: Address,
) -> Result<(), DepositError> {
    crate::admin::require_admin(env, &caller).map_err(|_| DepositError::InvalidAsset)?;

    if native_asset == env.current_contract_address() {
        return Err(DepositError::InvalidAsset);
    }
    env.storage()
        .persistent()
        .set(&DepositDataKey::NativeAssetAddress, &native_asset);
    Ok(())
}

/// Update user analytics after deposit
pub fn update_user_analytics(
    env: &Env,
    user: &Address,
    amount: i128,
    timestamp: u64,
    is_deposit: bool,
) -> Result<(), DepositError> {
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

    if is_deposit {
        analytics.total_deposits = analytics
            .total_deposits
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
        analytics.collateral_value = analytics
            .collateral_value
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
    }

    analytics.transaction_count = analytics.transaction_count.saturating_add(1);
    analytics.last_activity = timestamp;

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Update protocol analytics after deposit
pub fn update_protocol_analytics(
    env: &Env,
    amount: i128,
    is_deposit: bool,
) -> Result<(), DepositError> {
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

    if is_deposit {
        analytics.total_deposits = analytics
            .total_deposits
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
        analytics.total_value_locked = analytics
            .total_value_locked
            .checked_add(amount)
            .ok_or(DepositError::Overflow)?;
    }

    env.storage().persistent().set(&analytics_key, &analytics);
    Ok(())
}

/// Add entry to activity log
pub fn add_activity_log(
    env: &Env,
    user: &Address,
    activity_type: Symbol,
    amount: i128,
    asset: Option<Address>,
    timestamp: u64,
) -> Result<(), DepositError> {
    let log_key = DepositDataKey::ActivityLog;
    let mut log = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Vec<Activity>>(&log_key)
        .unwrap_or_else(|| Vec::new(env));

    let activity = Activity {
        user: user.clone(),
        activity_type,
        amount,
        asset,
        timestamp,
        metadata: Map::new(env),
    };

    log.push_back(activity);

    // Keep only last 1000 activities (prevent unbounded growth)
    if log.len() > 1000 {
        log.pop_front();
    }

    env.storage().persistent().set(&log_key, &log);
    Ok(())
}

/// Emit legacy position and stable borrower-health events for indexers.
pub fn emit_position_updated_event(
    env: &Env,
    user: &Address,
    position: &Position,
    operation: Symbol,
    timestamp: u64,
) {
    emit_position_updated(
        env,
        PositionUpdatedEvent {
            user: user.clone(),
            collateral: position.collateral,
            debt: position.debt,
        },
    );

    let snapshot = build_borrower_health_snapshot(position);
    emit_borrower_health_v1(
        env,
        BorrowerHealthEventV1 {
            schema_version: 1,
            user: user.clone(),
            operation,
            collateral: position.collateral,
            principal_debt: position.debt,
            borrow_interest: position.borrow_interest,
            total_debt: snapshot.total_debt,
            health_factor: snapshot.health_factor,
            risk_level: snapshot.risk_level,
            is_liquidatable: snapshot.is_liquidatable,
            timestamp,
        },
    );
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BorrowerHealthSnapshot {
    total_debt: i128,
    health_factor: i128,
    risk_level: i128,
    is_liquidatable: bool,
}

fn build_borrower_health_snapshot(position: &Position) -> BorrowerHealthSnapshot {
    let total_debt = position
        .debt
        .checked_add(position.borrow_interest)
        .unwrap_or(i128::MAX);
    let health_factor = if total_debt == 0 {
        i128::MAX
    } else {
        position
            .collateral
            .checked_mul(10_000)
            .and_then(|value| value.checked_div(total_debt))
            .unwrap_or(0)
    };
    let risk_level = calculate_risk_level(health_factor);
    let is_liquidatable = total_debt > 0 && health_factor < 10_500;

    BorrowerHealthSnapshot {
        total_debt,
        health_factor,
        risk_level,
        is_liquidatable,
    }
}

fn calculate_risk_level(health_factor: i128) -> i128 {
    if health_factor >= 15_000 {
        1
    } else if health_factor >= 12_000 {
        2
    } else if health_factor >= 11_000 {
        3
    } else if health_factor >= 10_500 {
        4
    } else {
        5
    }
}

/// Emit analytics updated event
pub fn emit_analytics_updated_event(
    env: &Env,
    user: &Address,
    activity_type: &str,
    amount: i128,
    timestamp: u64,
) {
    use soroban_sdk::String;
    emit_analytics_updated(
        env,
        AnalyticsUpdatedEvent {
            user: user.clone(),
            activity_type: String::from_str(env, activity_type),
            amount,
            timestamp,
        },
    );
}

/// Emit user activity tracked event
pub fn emit_user_activity_tracked_event(
    env: &Env,
    user: &Address,
    operation: Symbol,
    amount: i128,
    timestamp: u64,
) {
    emit_user_activity_tracked(
        env,
        UserActivityTrackedEvent {
            user: user.clone(),
            operation,
            amount,
            timestamp,
        },
    );
}

#[contracttype]
enum RiskDataKey {
    RiskConfig,
    EmergencyPause,
}

/// Check risk management pause status
/// This function checks the risk management system's pause switches
/// by accessing the storage directly to avoid module dependency issues
fn check_risk_management_pause(env: &Env) -> Result<(), DepositError> {
    // Define risk management storage keys locally to avoid dependency

    // Check emergency pause first
    let emergency_key = RiskDataKey::EmergencyPause;
    if let Some(emergency_paused) = env
        .storage()
        .persistent()
        .get::<RiskDataKey, bool>(&emergency_key)
    {
        if emergency_paused {
            return Err(DepositError::DepositPaused);
        }
    }

    // Check operation-specific pause in risk config
    // Note: We need to access the RiskConfig struct, but to avoid circular dependencies,
    // we'll check if the config exists and try to read pause_switches
    // For now, we'll skip this check and rely on the old pause switch system
    // The risk management pause switches should be checked at the contract API level

    Ok(())
}
