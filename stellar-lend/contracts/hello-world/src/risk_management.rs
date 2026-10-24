//! Risk management module for the StellarLend lending protocol.
//!
//! Manages the top-level risk configuration (`RiskConfig`), emergency pause
//! state, per-operation pause switches, and core risk parameter storage.
//! All rates are expressed in basis points (bps) where `10_000` equals 100%.
//!
//! This module is the single source of truth for all risk-related state.

use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env, Symbol};

use crate::admin;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BASIS_POINTS: i128 = 10_000;

/// Default minimum collateral ratio: 150% (15 000 bps).
pub const DEFAULT_MIN_COLLATERAL_RATIO_BPS: i128 = 15_000;
/// Default liquidation threshold: 120% (12 000 bps).
pub const DEFAULT_LIQUIDATION_THRESHOLD_BPS: i128 = 12_000;
/// Default close factor: 50% (5 000 bps).
pub const DEFAULT_CLOSE_FACTOR_BPS: i128 = 5_000;
/// Default liquidation incentive: 5% (500 bps).
pub const DEFAULT_LIQUIDATION_INCENTIVE_BPS: i128 = 500;

/// Minimum allowed collateral ratio: 100% (a ratio below this is always unsafe).
const MIN_COLLATERAL_RATIO_FLOOR: i128 = BASIS_POINTS;
/// Maximum allowed collateral ratio: 1000%.
const MAX_COLLATERAL_RATIO: i128 = 100_000;
/// Maximum allowed liquidation threshold.
const MAX_LIQUIDATION_THRESHOLD: i128 = 100_000;
/// Close factor must be in (0, 100%].
const MAX_CLOSE_FACTOR: i128 = BASIS_POINTS;
/// Liquidation incentive is bounded to 0–50%.
const MAX_LIQUIDATION_INCENTIVE: i128 = 5_000;
/// Maximum parameter change per update: 50% (5 000 bps).
const MAX_CHANGE_BPS: i128 = 5_000;

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

/// Errors returned by risk management operations.
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
/// Must be called once during contract initialization. Subsequent calls
/// return [`RiskManagementError::AlreadyInitialized`].
pub fn initialize_risk_management(env: &Env, _admin: Address) -> Result<(), RiskManagementError> {
    if env.storage().persistent().has(&RiskDataKey::RiskConfig) {
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
// RiskConfig accessors
// ---------------------------------------------------------------------------

/// Return the current [`RiskConfig`], or `None` if not yet initialized.
pub fn get_risk_config(env: &Env) -> Option<RiskConfig> {
    env.storage().persistent().get(&RiskDataKey::RiskConfig)
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
/// active. Returns [`RiskManagementError::EmergencyPaused`] or
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
    env.events()
        .publish((symbol_short!("risk"), symbol_short!("emerg")), paused);
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
// Parameter updates
// ---------------------------------------------------------------------------

/// Validate and store updated risk parameters (admin only).
///
/// Any `None` field is left unchanged. Each provided value is validated
/// independently; an out-of-range value returns the corresponding error before
/// any storage is modified.
pub fn set_risk_params(
    env: &Env,
    admin: Address,
    min_collateral_ratio: Option<i128>,
    liquidation_threshold: Option<i128>,
    close_factor: Option<i128>,
    liquidation_incentive: Option<i128>,
) -> Result<(), RiskManagementError> {
    admin::require_admin(env, &admin).map_err(|_| RiskManagementError::Unauthorized)?;

    let config = get_risk_config(env).ok_or(RiskManagementError::NotInitialized)?;

    let new_min_cr = min_collateral_ratio.unwrap_or(config.min_collateral_ratio_bps);
    let new_liq_thresh = liquidation_threshold.unwrap_or(config.liquidation_threshold_bps);
    let new_close = close_factor.unwrap_or(config.close_factor_bps);
    let new_incentive = liquidation_incentive.unwrap_or(config.liquidation_incentive_bps);

    // Validate min_collateral_ratio
    if let Some(v) = min_collateral_ratio {
        validate_bounded(v, MIN_COLLATERAL_RATIO_FLOOR, MAX_COLLATERAL_RATIO)
            .map_err(|_| RiskManagementError::InvalidCollateralRatio)?;
        validate_change(v, config.min_collateral_ratio_bps)
            .map_err(|_| RiskManagementError::ParameterChangeTooLarge)?;
    }

    // Validate liquidation_threshold
    if let Some(v) = liquidation_threshold {
        validate_bounded(v, 1, MAX_LIQUIDATION_THRESHOLD)
            .map_err(|_| RiskManagementError::InvalidLiquidationThreshold)?;
        validate_change(v, config.liquidation_threshold_bps)
            .map_err(|_| RiskManagementError::ParameterChangeTooLarge)?;
    }

    // Validate close_factor
    if let Some(v) = close_factor {
        if v <= 0 || v > MAX_CLOSE_FACTOR {
            return Err(RiskManagementError::InvalidCloseFactor);
        }
    }

    // Validate liquidation_incentive
    if let Some(v) = liquidation_incentive {
        if v < 0 || v > MAX_LIQUIDATION_INCENTIVE {
            return Err(RiskManagementError::InvalidLiquidationIncentive);
        }
    }

    // Store the updated config
    let updated = RiskConfig {
        min_collateral_ratio_bps: new_min_cr,
        liquidation_threshold_bps: new_liq_thresh,
        close_factor_bps: new_close,
        liquidation_incentive_bps: new_incentive,
    };
    env.storage()
        .persistent()
        .set(&RiskDataKey::RiskConfig, &updated);

    Ok(())
}

/// Validate `value` is within `[min, max]`.
fn validate_bounded(value: i128, min: i128, max: i128) -> Result<(), RiskManagementError> {
    if value < min || value > max {
        return Err(RiskManagementError::InvalidParameter);
    }
    Ok(())
}

/// Validate that the change from `current` to `new_value` does not exceed
/// `MAX_CHANGE_BPS` (50%).
fn validate_change(new_value: i128, current: i128) -> Result<(), RiskManagementError> {
    let delta = (new_value - current).abs();
    let limit = current
        .checked_mul(MAX_CHANGE_BPS)
        .ok_or(RiskManagementError::Overflow)?
        .checked_div(BASIS_POINTS)
        .ok_or(RiskManagementError::InvalidParameter)?;
    if delta > limit {
        return Err(RiskManagementError::ParameterChangeTooLarge);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Liquidation logic
// ---------------------------------------------------------------------------

/// Return `true` if `collateral_value * 10_000 < debt_value *
/// liquidation_threshold_bps` (position is undercollateralized).
pub fn can_be_liquidated(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<bool, RiskManagementError> {
    if debt_value <= 0 {
        return Ok(false);
    }
    let threshold = get_liquidation_threshold(env).ok_or(RiskManagementError::NotInitialized)?;
    let lhs = collateral_value
        .checked_mul(BASIS_POINTS)
        .ok_or(RiskManagementError::Overflow)?;
    let rhs = debt_value
        .checked_mul(threshold)
        .ok_or(RiskManagementError::Overflow)?;
    Ok(lhs < rhs)
}

/// Return the maximum amount that may be repaid in a single liquidation call:
/// `debt_value * close_factor_bps / 10_000`.
pub fn get_max_liquidatable_amount(
    env: &Env,
    debt_value: i128,
) -> Result<i128, RiskManagementError> {
    let close_factor = get_close_factor(env).ok_or(RiskManagementError::NotInitialized)?;
    debt_value
        .checked_mul(close_factor)
        .ok_or(RiskManagementError::Overflow)?
        .checked_div(BASIS_POINTS)
        .ok_or(RiskManagementError::InvalidParameter)
}

/// Return the collateral bonus paid to the liquidator:
/// `liquidated_amount * liquidation_incentive_bps / 10_000`.
pub fn get_liquidation_incentive_amount(
    env: &Env,
    liquidated_amount: i128,
) -> Result<i128, RiskManagementError> {
    let incentive = get_liquidation_incentive(env).ok_or(RiskManagementError::NotInitialized)?;
    liquidated_amount
        .checked_mul(incentive)
        .ok_or(RiskManagementError::Overflow)?
        .checked_div(BASIS_POINTS)
        .ok_or(RiskManagementError::InvalidParameter)
}

/// Require that `collateral_value / debt_value >= min_collateral_ratio`.
///
/// Expressed as: `collateral_value * 10_000 >= debt_value * min_ratio_bps`.
pub fn require_min_collateral_ratio(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<(), RiskManagementError> {
    if debt_value <= 0 {
        return Ok(());
    }
    let min_ratio = get_min_collateral_ratio(env).ok_or(RiskManagementError::NotInitialized)?;
    let lhs = collateral_value
        .checked_mul(BASIS_POINTS)
        .ok_or(RiskManagementError::Overflow)?;
    let rhs = debt_value
        .checked_mul(min_ratio)
        .ok_or(RiskManagementError::Overflow)?;
    if lhs < rhs {
        return Err(RiskManagementError::InsufficientCollateralRatio);
    }
    Ok(())
}
