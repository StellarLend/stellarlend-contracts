//! Risk management module for the StellarLend lending protocol.
//!
//! Manages the top-level risk configuration (`RiskConfig`), emergency pause
//! state, and per-operation pause switches.  All rates are expressed in basis
//! points (bps) where `10_000` equals 100%.

use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env, Symbol};

use crate::admin;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default minimum collateral ratio: 150% (15 000 bps).
pub const DEFAULT_MIN_COLLATERAL_RATIO_BPS: i128 = 15_000;
/// Default liquidation threshold: 120% (12 000 bps).
pub const DEFAULT_LIQUIDATION_THRESHOLD_BPS: i128 = 12_000;
/// Default close factor: 50% (5 000 bps).
pub const DEFAULT_CLOSE_FACTOR_BPS: i128 = 5_000;
/// Default liquidation incentive: 5% (500 bps).
pub const DEFAULT_LIQUIDATION_INCENTIVE_BPS: i128 = 500;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RiskDataKey {
    /// The admin-configurable risk parameters.
    RiskConfig,
    /// Global emergency pause flag.
    EmergencyPaused,
    /// Per-operation pause flag, keyed by operation symbol.
    OperationPaused(Symbol),
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Admin-configurable top-level risk parameters.
///
/// All ratio fields are expressed in basis points (bps).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskConfig {
    /// Minimum collateral ratio required to open / maintain a position, in bps.
    pub min_collateral_ratio_bps: i128,
    /// Collateral value threshold at which a position becomes liquidatable, in bps.
    pub liquidation_threshold_bps: i128,
    /// Maximum fraction of a position's debt that can be repaid in a single
    /// liquidation, in bps.
    pub close_factor_bps: i128,
    /// Additional collateral awarded to the liquidator as a percentage of the
    /// seized collateral, in bps.
    pub liquidation_incentive_bps: i128,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            min_collateral_ratio_bps: DEFAULT_MIN_COLLATERAL_RATIO_BPS,
            liquidation_threshold_bps: DEFAULT_LIQUIDATION_THRESHOLD_BPS,
            close_factor_bps: DEFAULT_CLOSE_FACTOR_BPS,
            liquidation_incentive_bps: DEFAULT_LIQUIDATION_INCENTIVE_BPS,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by risk-management operations.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RiskManagementError {
    /// Caller is not the stored protocol admin.
    Unauthorized = 1,
    /// Contract has not been initialised.
    NotInitialized = 2,
    /// Contract is already initialised.
    AlreadyInitialized = 3,
    /// A provided parameter is outside its allowed range.
    InvalidParameter = 4,
    /// The protocol is currently in emergency pause mode.
    EmergencyPaused = 5,
    /// The requested operation is individually paused.
    OperationPaused = 6,
    /// A parameter change exceeds the maximum allowed delta.
    ParameterChangeTooLarge = 7,
    /// The supplied collateral ratio is invalid.
    InvalidCollateralRatio = 8,
    /// The supplied liquidation threshold is invalid.
    InvalidLiquidationThreshold = 9,
    /// The supplied close factor is invalid.
    InvalidCloseFactor = 10,
    /// The supplied liquidation incentive is invalid.
    InvalidLiquidationIncentive = 11,
    /// Checked arithmetic overflowed.
    Overflow = 12,
    /// The position does not meet the minimum collateral ratio.
    InsufficientCollateralRatio = 13,
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize risk management with default parameters.
///
/// Must be called once during contract initialization.  Subsequent calls
/// return [`RiskManagementError::AlreadyInitialized`].
pub fn initialize_risk_management(
    env: &Env,
    _admin: Address,
) -> Result<(), RiskManagementError> {
    if env
        .storage()
        .persistent()
        .has(&RiskDataKey::RiskConfig)
    {
        return Err(RiskManagementError::AlreadyInitialized);
    }

    env.storage()
        .persistent()
        .set(&RiskDataKey::RiskConfig, &RiskConfig::default());
    env.storage()
        .persistent()
        .set(&RiskDataKey::EmergencyPaused, &false);
    Ok(())
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

/// Return the current [`RiskConfig`], or `None` if not yet initialized.
pub fn get_risk_config(env: &Env) -> Option<RiskConfig> {
    env.storage()
        .persistent()
        .get(&RiskDataKey::RiskConfig)
}

/// Return the minimum collateral ratio in bps, or `None` if not initialized.
pub fn get_min_collateral_ratio(env: &Env) -> Option<i128> {
    get_risk_config(env).map(|c| c.min_collateral_ratio_bps)
}

/// Return the liquidation threshold in bps, or `None` if not initialized.
pub fn get_liquidation_threshold(env: &Env) -> Option<i128> {
    get_risk_config(env).map(|c| c.liquidation_threshold_bps)
}

/// Return the close factor in bps, or `None` if not initialized.
pub fn get_close_factor(env: &Env) -> Option<i128> {
    get_risk_config(env).map(|c| c.close_factor_bps)
}

/// Return the liquidation incentive in bps, or `None` if not initialized.
pub fn get_liquidation_incentive(env: &Env) -> Option<i128> {
    get_risk_config(env).map(|c| c.liquidation_incentive_bps)
}

// ---------------------------------------------------------------------------
// Pause controls
// ---------------------------------------------------------------------------

/// Return `true` if the global emergency pause is active.
pub fn is_emergency_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get::<RiskDataKey, bool>(&RiskDataKey::EmergencyPaused)
        .unwrap_or(false)
}

/// Return `true` if the named operation is individually paused.
pub fn is_operation_paused(env: &Env, operation: Symbol) -> bool {
    env.storage()
        .persistent()
        .get::<RiskDataKey, bool>(&RiskDataKey::OperationPaused(operation))
        .unwrap_or(false)
}

/// Check that neither the emergency pause nor the named operation pause is
/// active.  Returns [`RiskManagementError::EmergencyPaused`] or
/// [`RiskManagementError::OperationPaused`] if either is set.
pub fn check_emergency_pause(env: &Env) -> Result<(), RiskManagementError> {
    if is_emergency_paused(env) {
        return Err(RiskManagementError::EmergencyPaused);
    }
    Ok(())
}

/// Set or clear the global emergency pause (admin only).
pub fn set_emergency_pause(
    env: &Env,
    admin: Address,
    paused: bool,
) -> Result<(), RiskManagementError> {
    admin::require_admin(env, &admin).map_err(|_| RiskManagementError::Unauthorized)?;
    env.storage()
        .persistent()
        .set(&RiskDataKey::EmergencyPaused, &paused);
    env.events().publish(
        (symbol_short!("risk"), symbol_short!("emerg")),
        paused,
    );
    Ok(())
}

/// Set or clear a pause switch for a specific operation (admin only).
pub fn set_pause_switch(
    env: &Env,
    admin: Address,
    operation: Symbol,
    paused: bool,
) -> Result<(), RiskManagementError> {
    admin::require_admin(env, &admin).map_err(|_| RiskManagementError::Unauthorized)?;
    env.storage()
        .persistent()
        .set(&RiskDataKey::OperationPaused(operation.clone()), &paused);
    env.events().publish(
        (symbol_short!("risk"), symbol_short!("pause")),
        (operation, paused),
    );
    Ok(())
}

/// Set pause switches for multiple operations at once (admin only).
pub fn set_pause_switches(
    env: &Env,
    admin: Address,
    operations: soroban_sdk::Vec<(Symbol, bool)>,
) -> Result<(), RiskManagementError> {
    admin::require_admin(env, &admin).map_err(|_| RiskManagementError::Unauthorized)?;
    for item in operations.iter() {
        let (operation, paused) = item;
        env.storage()
            .persistent()
            .set(&RiskDataKey::OperationPaused(operation), &paused);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Parameter updates (thin wrapper — delegates to risk_params)
// ---------------------------------------------------------------------------

/// Update risk parameters (admin only).  Delegates bounds validation to
/// [`crate::risk_params::set_risk_params`].
pub fn set_risk_params(
    env: &Env,
    admin: Address,
    min_collateral_ratio: Option<i128>,
    liquidation_threshold: Option<i128>,
    close_factor: Option<i128>,
    liquidation_incentive: Option<i128>,
) -> Result<(), RiskManagementError> {
    admin::require_admin(env, &admin).map_err(|_| RiskManagementError::Unauthorized)?;
    crate::risk_params::set_risk_params(
        env,
        min_collateral_ratio,
        liquidation_threshold,
        close_factor,
        liquidation_incentive,
    )
    .map_err(|e| match e {
        crate::risk_params::RiskParamsError::ParameterChangeTooLarge => {
            RiskManagementError::ParameterChangeTooLarge
        }
        crate::risk_params::RiskParamsError::InvalidCollateralRatio => {
            RiskManagementError::InvalidCollateralRatio
        }
        crate::risk_params::RiskParamsError::InvalidLiquidationThreshold => {
            RiskManagementError::InvalidLiquidationThreshold
        }
        crate::risk_params::RiskParamsError::InvalidCloseFactor => {
            RiskManagementError::InvalidCloseFactor
        }
        crate::risk_params::RiskParamsError::InvalidLiquidationIncentive => {
            RiskManagementError::InvalidLiquidationIncentive
        }
        _ => RiskManagementError::InvalidParameter,
    })
}

// ---------------------------------------------------------------------------
// Liquidation helpers (convenience re-exports matching lib.rs imports)
// ---------------------------------------------------------------------------

/// Check whether a position can be liquidated.
///
/// A position is liquidatable when `collateral_value * 10_000 <
/// debt_value * liquidation_threshold_bps`.
pub fn can_be_liquidated(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<bool, RiskManagementError> {
    crate::risk_params::can_be_liquidated(env, collateral_value, debt_value)
        .map_err(|_| RiskManagementError::InvalidParameter)
}

/// Return the maximum amount that can be liquidated in a single call
/// (close_factor × debt_value).
pub fn get_max_liquidatable_amount(
    env: &Env,
    debt_value: i128,
) -> Result<i128, RiskManagementError> {
    crate::risk_params::get_max_liquidatable_amount(env, debt_value)
        .map_err(|_| RiskManagementError::Overflow)
}

/// Return the collateral bonus awarded to the liquidator.
pub fn get_liquidation_incentive_amount(
    env: &Env,
    liquidated_amount: i128,
) -> Result<i128, RiskManagementError> {
    crate::risk_params::get_liquidation_incentive_amount(env, liquidated_amount)
        .map_err(|_| RiskManagementError::Overflow)
}

/// Require `collateral_value / debt_value >= min_collateral_ratio`.
pub fn require_min_collateral_ratio(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<(), RiskManagementError> {
    crate::risk_params::require_min_collateral_ratio(env, collateral_value, debt_value)
        .map_err(|_| RiskManagementError::InsufficientCollateralRatio)
}
