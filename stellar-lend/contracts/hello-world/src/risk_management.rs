//! # Risk Management Module
//!
//! Provides configurable risk parameters and pause controls for the lending protocol.
//!
//! ## Risk Parameters (all in basis points)
//! - **Minimum collateral ratio** (default 110%): below this, new borrows are rejected
//! - **Liquidation threshold** (default 105%): below this, positions can be liquidated
//! - **Close factor** (default 50%): max percentage of debt liquidatable per transaction
//! - **Liquidation incentive** (default 10%): bonus awarded to liquidators
//!
//! ## Pause Controls
//! - Per-operation pause switches (deposit, withdraw, borrow, repay, liquidate)
//! - Global emergency pause that halts all operations immediately
//!
//! ## Safety
//! - Parameter changes are limited to ±10% per update to prevent drastic shifts.
//! - Min collateral ratio must always be ≥ liquidation threshold.
//! - Only the admin address can modify risk parameters.

#![allow(unused)]
use crate::events::{
    emit_admin_action, emit_pause_state_changed, emit_risk_params_updated, AdminActionEvent,
    PauseStateChangedEvent, RiskParamsUpdatedEvent,
};
use soroban_sdk::{contracterror, contracttype, Address, Env, IntoVal, Map, Symbol, Val, Vec};
use crate::prelude::*;

/// Errors that can occur during risk management operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RiskManagementError {
    /// Unauthorized access - caller is not admin
    Unauthorized = 1,
    /// Invalid parameter value
    InvalidParameter = 2,
    /// Parameter change exceeds maximum allowed change
    ParameterChangeTooLarge = 3,
    /// Minimum collateral ratio not met
    InsufficientCollateralRatio = 4,
    /// Operation is paused
    OperationPaused = 5,
    /// Emergency pause is active
    EmergencyPaused = 6,
    /// Invalid collateral ratio (must be >= liquidation threshold)
    InvalidCollateralRatio = 7,
    /// Invalid liquidation threshold (must be <= collateral ratio)
    InvalidLiquidationThreshold = 8,
    /// Close factor out of valid range (0-100%)
    InvalidCloseFactor = 9,
    /// Liquidation incentive out of valid range (0-50%)
    InvalidLiquidationIncentive = 10,
    /// Overflow occurred during calculation
    Overflow = 11,
    /// Action requires governance approval
    GovernanceRequired = 12,
    /// Contract has already been initialized
    AlreadyInitialized = 13,
}
/// Storage keys for risk management data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum RiskDataKey {
    /// Global risk configuration parameters (MCR, liquidation threshold, etc.)
    /// Value type: RiskConfig
    RiskConfig,
    /// Global emergency pause flag. If true, all protocol operations are halted.
    /// Value type: bool
    EmergencyPause,
    /// Timelock for safety of sensitive parameter changes
    /// Value type: u64 (timestamp)
    ParameterChangeTimelock,
}

/// Risk configuration parameters for pause switches
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RiskConfig {
    /// Pause switches for different operations
    pub pause_switches: Map<Symbol, bool>,
    /// Last update timestamp
    pub last_update: u64,
}

/// Pause switch operation types
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PauseOperation {
    /// Pause deposit operations
    Deposit,
    /// Pause withdraw operations
    Withdraw,
    /// Pause borrow operations
    Borrow,
    /// Pause repay operations
    Repay,
    /// Pause liquidation operations
    Liquidate,
    /// Pause all operations (emergency)
    All,
}

/// Initialize risk management system
///
/// Sets up default risk parameters and admin address.
/// Should be called during contract initialization.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `admin` - The admin address
///
/// # Returns
/// Returns Ok(()) on success
///
/// # Errors
/// * `RiskManagementError::InvalidParameter` - If default parameters are invalid
pub fn initialize_risk_management(env: &Env, admin: Address) -> Result<(), RiskManagementError> {
    // Check if initialized
    let config_key = RiskDataKey::RiskConfig;
    if env.storage().persistent().has(&config_key) {
        return Ok(());
    }

    // Admin is already set in the centralized admin module during contract initialize
    // We don't set it here anymore to maintain a single source of truth.


    // Initialize default risk config for pause switches
    let default_config = RiskConfig {
        pause_switches: create_default_pause_switches(env),
        last_update: env.ledger().timestamp(),
    };

    let config_key = RiskDataKey::RiskConfig;
    env.storage().persistent().set(&config_key, &default_config);

    // Initialize emergency pause as false
    let emergency_key = RiskDataKey::EmergencyPause;
    env.storage().persistent().set(&emergency_key, &false);

    emit_admin_action(
        env,
        AdminActionEvent {
            actor: admin.clone(),
            action: Symbol::new(env, "initialize"),
            timestamp: env.ledger().timestamp(),
        },
    );

    Ok(())
}

/// Create default pause switches map
fn create_default_pause_switches(env: &Env) -> Map<Symbol, bool> {
    let mut switches = Map::new(env);
    switches.set(Symbol::new(env, "pause_deposit"), false);
    switches.set(Symbol::new(env, "pause_withdraw"), false);
    switches.set(Symbol::new(env, "pause_borrow"), false);
    switches.set(Symbol::new(env, "pause_repay"), false);
    switches.set(Symbol::new(env, "pause_liquidate"), false);
    switches
}

/// Get the admin address (deprecated, delegates to new admin module)
#[deprecated(note = "Use crate::admin::get_admin instead")]
pub fn get_admin(env: &Env) -> Option<Address> {
    crate::admin::get_admin(env)
}

/// Check if caller is admin (delegates to new admin module)
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), RiskManagementError> {
    crate::admin::require_admin(env, caller).map_err(|_| RiskManagementError::Unauthorized)
}

/// Get current risk configuration
pub fn get_risk_config(env: &Env) -> Option<RiskConfig> {
    let config_key = RiskDataKey::RiskConfig;
    env.storage()
        .persistent()
        .get::<RiskDataKey, RiskConfig>(&config_key)
}

/// Set pause switches (admin only)
///
/// Updates pause switches for different operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `operation` - The operation to pause/unpause (as Symbol)
/// * `paused` - Whether to pause (true) or unpause (false)
///
/// # Returns
/// Returns Ok(()) on success
///
/// # Errors
/// * `RiskManagementError::Unauthorized` - If caller is not admin
pub fn set_pause_switch(
    env: &Env,
    caller: Address,
    operation: Symbol,
    paused: bool,
) -> Result<(), RiskManagementError> {
    // Check admin
    require_admin(env, &caller)?;

    // Get current config
    let mut config = get_risk_config(env).ok_or(RiskManagementError::InvalidParameter)?;

    // Update pause switch
    config.pause_switches.set(operation.clone(), paused);

    // Update timestamp
    config.last_update = env.ledger().timestamp();

    // Save config
    let config_key = RiskDataKey::RiskConfig;
    env.storage().persistent().set(&config_key, &config);

    // Emit event
    emit_pause_switch_updated_event(env, &caller, &operation, paused);

    Ok(())
}

/// Set multiple pause switches at once (admin only)
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `switches` - Map of operation symbols to pause states
///
/// # Returns
/// Returns Ok(()) on success
pub fn set_pause_switches(
    env: &Env,
    caller: Address,
    switches: Map<Symbol, bool>,
) -> Result<(), RiskManagementError> {
    // Check admin
    require_admin(env, &caller)?;

    // Get current config
    let mut config = get_risk_config(env).ok_or(RiskManagementError::InvalidParameter)?;

    // Update all pause switches
    for (op, paused) in switches.iter() {
        config.pause_switches.set(op, paused);
    }

    // Update timestamp
    config.last_update = env.ledger().timestamp();

    // Save config
    let config_key = RiskDataKey::RiskConfig;
    env.storage().persistent().set(&config_key, &config);

    // Emit event
    emit_pause_switches_updated_event(env, &caller, &switches);

    Ok(())
}

/// Check if an operation is paused
pub fn is_operation_paused(env: &Env, operation: Symbol) -> bool {
    if let Some(config) = get_risk_config(env) {
        config.pause_switches.get(operation).unwrap_or(false)
    } else {
        false
    }
}

/// Require that an operation is not paused.
///
/// Checks the **emergency pause first**, then the per-operation switch.
/// This layering ensures a single `set_emergency_pause(true)` call halts
/// every operation without having to update every individual switch.
///
/// ## Pause precedence matrix
///
/// | emergency | per-op | result                        |
/// |-----------|--------|-------------------------------|
/// | false     | false  | ✅ allowed                     |
/// | false     | true   | ❌ `OperationPaused`           |
/// | true      | false  | ❌ `EmergencyPaused`           |
/// | true      | true   | ❌ `EmergencyPaused` (wins)    |
///
/// # Errors
/// * [`RiskManagementError::EmergencyPaused`] – global emergency halt is active.
/// * [`RiskManagementError::OperationPaused`] – this specific operation is paused.
///
/// # Security
/// Always call this at the start of every user-facing entry point before
/// reading or writing any protocol state.
pub fn require_operation_not_paused(
    env: &Env,
    operation: Symbol,
) -> Result<(), RiskManagementError> {
    // Emergency pause takes precedence over per-operation switches.
    if is_emergency_paused(env) {
        return Err(RiskManagementError::EmergencyPaused);
    }
    if is_operation_paused(env, operation) {
        return Err(RiskManagementError::OperationPaused);
    }
    Ok(())
}

/// Check if operation is paused (public helper for other modules)
/// This is a convenience function that can be called from other modules
pub fn check_operation_paused(env: &Env, operation: Symbol) -> bool {
    // First check emergency pause
    if is_emergency_paused(env) {
        return true;
    }
    // Then check specific operation pause
    is_operation_paused(env, operation)
}

/// Set emergency pause (admin only)
///
/// Emergency pause stops all operations immediately.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `paused` - Whether to enable (true) or disable (false) emergency pause
///
/// # Returns
/// Returns Ok(()) on success
pub fn set_emergency_pause(
    env: &Env,
    caller: Address,
    paused: bool,
) -> Result<(), RiskManagementError> {
    // Check admin
    require_admin(env, &caller)?;

    // Set emergency pause
    let emergency_key = RiskDataKey::EmergencyPause;
    env.storage().persistent().set(&emergency_key, &paused);

    // Emit event
    emit_emergency_pause_event(env, &caller, paused);

    Ok(())
}

/// Check if emergency pause is active
pub fn is_emergency_paused(env: &Env) -> bool {
    let emergency_key = RiskDataKey::EmergencyPause;
    env.storage()
        .persistent()
        .get::<RiskDataKey, bool>(&emergency_key)
        .unwrap_or(false)
}

/// Require that emergency pause is not active
pub fn check_emergency_pause(env: &Env) -> Result<(), RiskManagementError> {
    if is_emergency_paused(env) {
        return Err(RiskManagementError::EmergencyPaused);
    }
    Ok(())
}

/// Emit pause switch updated event
fn emit_pause_switch_updated_event(env: &Env, caller: &Address, operation: &Symbol, paused: bool) {
    emit_pause_state_changed(
        env,
        PauseStateChangedEvent {
            actor: caller.clone(),
            operation: operation.clone(),
            paused,
            timestamp: env.ledger().timestamp(),
        },
    );
}

/// Emit pause switches updated event
fn emit_pause_switches_updated_event(env: &Env, caller: &Address, switches: &Map<Symbol, bool>) {
    for (operation, paused) in switches.iter() {
        emit_pause_state_changed(
            env,
            PauseStateChangedEvent {
                actor: caller.clone(),
                operation,
                paused,
                timestamp: env.ledger().timestamp(),
            },
        );
    }
}

/// Emit emergency pause event
fn emit_emergency_pause_event(env: &Env, caller: &Address, paused: bool) {
    emit_pause_state_changed(
        env,
        PauseStateChangedEvent {
            actor: caller.clone(),
            operation: Symbol::new(env, "emergency"),
            paused,
            timestamp: env.ledger().timestamp(),
        },
    );
}
