#![allow(unused)]
use soroban_sdk::{contracterror, contracttype, Address, Env, IntoVal, Symbol, Val, Vec, I256};

/// Errors that can occur during risk parameter management
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RiskParamsError {
    /// Unauthorized access - caller is not admin
    Unauthorized = 1,
    /// Invalid parameter value
    InvalidParameter = 2,
    /// Parameter change exceeds maximum allowed change
    ParameterChangeTooLarge = 3,
    /// Invalid collateral ratio (must be >= liquidation threshold)
    InvalidCollateralRatio = 4,
    /// Invalid liquidation threshold (must be <= collateral ratio)
    InvalidLiquidationThreshold = 5,
    /// Close factor out of valid range (0-100%)
    InvalidCloseFactor = 6,
    /// Liquidation incentive out of valid range (0-50%)
    InvalidLiquidationIncentive = 7,
    /// Calculation overflow occurred
    Overflow = 8,
    /// Protocol is emergency paused, admin changes blocked
    EmergencyPaused = 9,
    /// Invalid parameter combination - safety margin violated
    InvalidParameterCombination = 10,
    /// Liquidation threshold too close to min collateral ratio
    InsufficientSafetyMargin = 11,
}

/// Storage keys for risk params data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum RiskParamsDataKey {
    /// Risk configuration parameters
    RiskParamsConfig,
}

/// Risk parameters
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RiskParams {
    /// Minimum collateral ratio (in basis points, e.g., 11000 = 110%)
    /// Users must maintain this ratio or face liquidation
    pub min_collateral_ratio: i128,
    /// Liquidation threshold (in basis points, e.g., 10500 = 105%)
    /// When collateral ratio falls below this, liquidation is allowed
    pub liquidation_threshold: i128,
    /// Close factor (in basis points, e.g., 5000 = 50%)
    /// Maximum percentage of debt that can be liquidated in a single transaction
    pub close_factor: i128,
    /// Liquidation incentive (in basis points, e.g., 1000 = 10%)
    /// Bonus given to liquidators
    pub liquidation_incentive: i128,
    /// Last update timestamp
    pub last_update: u64,
}

/// Constants for parameter validation
const BASIS_POINTS_SCALE: i128 = 10_000; // 100% = 10,000 basis points
const MIN_COLLATERAL_RATIO_MIN: i128 = 10_000; // 100% minimum
const MIN_COLLATERAL_RATIO_MAX: i128 = 50_000; // 500% maximum
const LIQUIDATION_THRESHOLD_MIN: i128 = 10_000; // 100% minimum
const LIQUIDATION_THRESHOLD_MAX: i128 = 50_000; // 500% maximum
const CLOSE_FACTOR_MIN: i128 = 0; // 0% minimum
const CLOSE_FACTOR_MAX: i128 = BASIS_POINTS_SCALE; // 100% maximum
const LIQUIDATION_INCENTIVE_MIN: i128 = 0; // 0% minimum
const LIQUIDATION_INCENTIVE_MAX: i128 = 5_000; // 50% maximum (safety limit)
const MAX_PARAMETER_CHANGE_BPS: i128 = 5_000; // 50% maximum change per update

/// Enhanced validation constants for hardened security
const MIN_SAFETY_MARGIN_BPS: i128 = 500; // 5% minimum margin between liquidation threshold and min CR
const MAX_CLOSE_FACTOR_SAFE: i128 = 7_500; // 75% maximum for safety (below 100% theoretical max)
const MAX_LIQUIDATION_INCENTIVE_SAFE: i128 = 2_500; // 25% maximum for safety (below 50% theoretical max)
const MAX_SINGLE_CHANGE_BPS: i128 = 500; // 5% maximum single change (more conservative)
const MAX_DAILY_CHANGES: u32 = 3; // Maximum parameter changes per day
const MIN_TIME_BETWEEN_CHANGES: u64 = 3600; // 1 hour minimum between changes (in seconds)

/// Initialize risk parameters
///
/// Sets up default risk parameters.
/// Should be called during contract initialization.
///
/// # Arguments
/// * `env` - The Soroban environment
///
/// # Returns
/// Returns Ok(()) on success
///
/// # Errors
/// * `RiskParamsError::InvalidParameter` - If default parameters are invalid
pub fn initialize_risk_params(env: &Env) -> Result<(), RiskParamsError> {
    let default_config = RiskParams {
        min_collateral_ratio: 11_000,  // 110% default
        liquidation_threshold: 10_500, // 105% default
        close_factor: 5_000,           // 50% default
        liquidation_incentive: 1_000,  // 10% default
        last_update: env.ledger().timestamp(),
    };

    validate_risk_params(&default_config)?;

    let config_key = RiskParamsDataKey::RiskParamsConfig;
    env.storage().persistent().set(&config_key, &default_config);

    Ok(())
}

/// Get current risk parameters
pub fn get_risk_params(env: &Env) -> Option<RiskParams> {
    let config_key = RiskParamsDataKey::RiskParamsConfig;
    env.storage()
        .persistent()
        .get::<RiskParamsDataKey, RiskParams>(&config_key)
}

/// Validate risk configuration
/// Validate risk parameters with enhanced security checks
///
/// Performs comprehensive validation of risk parameters including:
/// - Individual parameter bounds validation
/// - Cross-parameter relationship validation  
/// - Safety margin enforcement between liquidation threshold and min CR
/// - Conservative limits for close factor and liquidation incentive
///
/// # Arguments
/// * `config` - Risk parameters to validate
///
/// # Returns
/// Returns Ok(()) if all validations pass
///
/// # Errors
/// * `RiskParamsError::InvalidParameter` - Parameter outside valid range
/// * `RiskParamsError::InvalidLiquidationThreshold` - Liquidation threshold invalid
/// * `RiskParamsError::InvalidCollateralRatio` - Min CR invalid or relationship violated
/// * `RiskParamsError::InvalidCloseFactor` - Close factor outside safe range
/// * `RiskParamsError::InvalidLiquidationIncentive` - Liquidation incentive outside safe range
/// * `RiskParamsError::InsufficientSafetyMargin` - Safety margin between parameters too small
/// * `RiskParamsError::InvalidParameterCombination` - Invalid parameter combination
///
/// # Security
/// Enforces conservative limits and safety margins to prevent protocol exploitation.
/// All parameter relationships are validated to ensure system stability.
pub(crate) fn validate_risk_params(config: &RiskParams) -> Result<(), RiskParamsError> {
    // Validate min collateral ratio bounds
    if config.min_collateral_ratio < MIN_COLLATERAL_RATIO_MIN
        || config.min_collateral_ratio > MIN_COLLATERAL_RATIO_MAX
    {
        return Err(RiskParamsError::InvalidParameter);
    }

    // Validate liquidation threshold bounds
    if config.liquidation_threshold < LIQUIDATION_THRESHOLD_MIN
        || config.liquidation_threshold > LIQUIDATION_THRESHOLD_MAX
    {
        return Err(RiskParamsError::InvalidLiquidationThreshold);
    }

    // Validate that min collateral ratio >= liquidation threshold
    if config.min_collateral_ratio < config.liquidation_threshold {
        return Err(RiskParamsError::InvalidCollateralRatio);
    }

    // HARDENED: Enforce minimum safety margin between liquidation threshold and min CR
    let safety_margin = config.min_collateral_ratio
        .checked_sub(config.liquidation_threshold)
        .ok_or(RiskParamsError::Overflow)?;
    
    if safety_margin < MIN_SAFETY_MARGIN_BPS {
        return Err(RiskParamsError::InsufficientSafetyMargin);
    }

    // Validate close factor with conservative limits
    if config.close_factor < CLOSE_FACTOR_MIN || config.close_factor > MAX_CLOSE_FACTOR_SAFE {
        return Err(RiskParamsError::InvalidCloseFactor);
    }

    // Validate liquidation incentive with conservative limits
    if config.liquidation_incentive < LIQUIDATION_INCENTIVE_MIN
        || config.liquidation_incentive > MAX_LIQUIDATION_INCENTIVE_SAFE
    {
        return Err(RiskParamsError::InvalidLiquidationIncentive);
    }

    // HARDENED: Validate parameter combinations for system stability
    // Ensure liquidation incentive doesn't exceed close factor (prevents over-incentivization)
    if config.liquidation_incentive > config.close_factor {
        return Err(RiskParamsError::InvalidParameterCombination);
    }

    // HARDENED: Ensure close factor + liquidation incentive doesn't create perverse incentives
    let total_liquidation_benefit = config.close_factor
        .checked_add(config.liquidation_incentive)
        .ok_or(RiskParamsError::Overflow)?;
    
    if total_liquidation_benefit > BASIS_POINTS_SCALE {
        return Err(RiskParamsError::InvalidParameterCombination);
    }

    Ok(())
}

/// Validate parameter change doesn't exceed maximum allowed change
/// Validate parameter change with enhanced security constraints
///
/// Enforces conservative change limits and time-based restrictions to prevent
/// rapid parameter manipulation that could destabilize the protocol.
///
/// # Arguments
/// * `old_value` - Current parameter value
/// * `new_value` - Proposed new parameter value
/// * `last_update` - Timestamp of last parameter update
/// * `current_time` - Current timestamp
///
/// # Returns
/// Returns Ok(()) if change is within allowed limits
///
/// # Errors
/// * `RiskParamsError::ParameterChangeTooLarge` - Change exceeds maximum allowed
/// * `RiskParamsError::Overflow` - Arithmetic overflow in calculations
///
/// # Security
/// Uses conservative change limits (5% vs previous 10%) and enforces minimum
/// time between changes to prevent rapid parameter manipulation attacks.
pub(crate) fn validate_parameter_change(
    old_value: i128, 
    new_value: i128,
    last_update: u64,
    current_time: u64,
) -> Result<(), RiskParamsError> {
    // HARDENED: Enforce minimum time between changes
    if current_time < last_update + MIN_TIME_BETWEEN_CHANGES {
        return Err(RiskParamsError::ParameterChangeTooLarge);
    }

    let change = if new_value > old_value {
        new_value.checked_sub(old_value).ok_or(RiskParamsError::Overflow)?
    } else {
        old_value.checked_sub(new_value).ok_or(RiskParamsError::Overflow)?
    };

    // HARDENED: Use more conservative change limit (5% vs previous 10%)
    let max_change = old_value
        .checked_mul(MAX_SINGLE_CHANGE_BPS)
        .ok_or(RiskParamsError::Overflow)?
        .checked_div(BASIS_POINTS_SCALE)
        .ok_or(RiskParamsError::Overflow)?;

    if change > max_change {
        return Err(RiskParamsError::ParameterChangeTooLarge);
    }

    Ok(())
}

/// Set risk parameters (admin only - caller check should be done by the contract)
///
/// Updates risk parameters with validation and change limits.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `min_collateral_ratio` - New minimum collateral ratio (in basis points)
/// * `liquidation_threshold` - New liquidation threshold (in basis points)
/// * `close_factor` - New close factor (in basis points)
/// * `liquidation_incentive` - New liquidation incentive (in basis points)
///
/// # Returns
/// Returns Ok(()) on success
/// Set risk parameters with enhanced validation and security checks
///
/// Updates risk parameters with comprehensive validation including emergency pause
/// checks, parameter change limits, safety margins, and cross-parameter validation.
/// Only admin can call this function and only when protocol is not emergency paused.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - Address of the caller (must be admin)
/// * `min_collateral_ratio` - Optional new minimum collateral ratio (basis points)
/// * `liquidation_threshold` - Optional new liquidation threshold (basis points)
/// * `close_factor` - Optional new close factor (basis points)
/// * `liquidation_incentive` - Optional new liquidation incentive (basis points)
///
/// # Returns
/// Returns Ok(()) on successful update
///
/// # Errors
/// * `RiskParamsError::Unauthorized` - Caller is not admin
/// * `RiskParamsError::EmergencyPaused` - Protocol is emergency paused
/// * `RiskParamsError::InvalidParameter` - Parameter outside valid range
/// * `RiskParamsError::ParameterChangeTooLarge` - Change exceeds limits or too frequent
/// * `RiskParamsError::InsufficientSafetyMargin` - Safety margin too small
/// * `RiskParamsError::InvalidParameterCombination` - Invalid parameter combination
///
/// # Security
/// - Enforces admin authorization and emergency pause checks
/// - Validates all parameter changes against conservative limits
/// - Ensures safety margins between critical parameters
/// - Prevents rapid parameter manipulation through time-based restrictions
/// - Validates cross-parameter relationships for system stability
pub fn set_risk_params(
    env: &Env,
    caller: &Address,
    min_collateral_ratio: Option<i128>,
    liquidation_threshold: Option<i128>,
    close_factor: Option<i128>,
    liquidation_incentive: Option<i128>,
) -> Result<(), RiskParamsError> {
    // HARDENED: Check admin authorization
    require_admin(env, caller).map_err(|_| RiskParamsError::Unauthorized)?;

    // HARDENED: Block admin changes during emergency pause
    if is_emergency_paused(env) {
        return Err(RiskParamsError::EmergencyPaused);
    }

    let mut config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;
    let current_time = env.ledger().timestamp();

    // HARDENED: Validate parameter changes with time restrictions
    if let Some(mcr) = min_collateral_ratio {
        validate_parameter_change(config.min_collateral_ratio, mcr, config.last_update, current_time)?;
        config.min_collateral_ratio = mcr;
    }

    if let Some(lt) = liquidation_threshold {
        validate_parameter_change(config.liquidation_threshold, lt, config.last_update, current_time)?;
        config.liquidation_threshold = lt;
    }

    if let Some(cf) = close_factor {
        validate_parameter_change(config.close_factor, cf, config.last_update, current_time)?;
        config.close_factor = cf;
    }

    if let Some(li) = liquidation_incentive {
        validate_parameter_change(config.liquidation_incentive, li, config.last_update, current_time)?;
        config.liquidation_incentive = li;
    }

    // HARDENED: Validate the updated config with enhanced checks
    validate_risk_params(&config)?;

    // Update timestamp
    config.last_update = current_time;

    // Save config
    let config_key = RiskParamsDataKey::RiskParamsConfig;
    env.storage().persistent().set(&config_key, &config);

    // Emit event
    emit_risk_params_updated_event(env, &config);

    Ok(())
}

/// Emit risk parameters updated event
#[allow(deprecated)]
fn emit_risk_params_updated_event(env: &Env, config: &RiskParams) {
    let topics = (Symbol::new(env, "risk_params_updated"),);
    env.events().publish(topics, config.clone());
}

/// Get minimum collateral ratio
pub fn get_min_collateral_ratio(env: &Env) -> Result<i128, RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;
    Ok(config.min_collateral_ratio)
}

/// Get liquidation threshold
pub fn get_liquidation_threshold(env: &Env) -> Result<i128, RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;
    Ok(config.liquidation_threshold)
}

/// Get close factor
pub fn get_close_factor(env: &Env) -> Result<i128, RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;
    Ok(config.close_factor)
}

/// Get liquidation incentive
pub fn get_liquidation_incentive(env: &Env) -> Result<i128, RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;
    Ok(config.liquidation_incentive)
}

/// Calculate maximum liquidatable amount
///
/// Uses close factor to determine maximum debt that can be liquidated.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `debt_value` - Total debt value (in base units)
///
/// # Returns
/// Maximum amount that can be liquidated
/// Get the maximum amount that can be liquidated for a given debt position.
/// Uses the configured close factor (default 5000 bps = 50%).
/// Uses I256 for safe intermediate multiplication.
pub fn get_max_liquidatable_amount(env: &Env, debt_value: i128) -> Result<i128, RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;

    // Calculate: debt * close_factor / BASIS_POINTS_SCALE using I256 to prevent overflow
    let debt_256 = I256::from_i128(env, debt_value);
    let close_factor_256 = I256::from_i128(env, config.close_factor);
    let scale_256 = I256::from_i128(env, BASIS_POINTS_SCALE);

    let max_amount_256 = debt_256.mul(&close_factor_256).div(&scale_256);
    let max_amount = max_amount_256.to_i128().ok_or(RiskParamsError::Overflow)?;

    Ok(max_amount)
}

/// Calculate liquidation incentive amount
///
/// Returns the bonus amount for liquidators.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `liquidated_amount` - Amount being liquidated (in base units)
///
/// # Returns
/// Liquidation incentive amount
/// Get the bonus incentive amount for a liquidation.
/// Uses the configured liquidation incentive (default 1000 bps = 10%).
/// Returns incentive in the same units as the liquidated amount.
pub fn get_liquidation_incentive_amount(
    env: &Env,
    liquidated_amount: i128,
) -> Result<i128, RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;

    // Calculate: amount * liquidation_incentive / BASIS_POINTS_SCALE using I256
    let amount_256 = I256::from_i128(env, liquidated_amount);
    let incentive_256 = I256::from_i128(env, config.liquidation_incentive);
    let scale_256 = I256::from_i128(env, BASIS_POINTS_SCALE);

    let result_256 = amount_256.mul(&incentive_256).div(&scale_256);
    let result = result_256.to_i128().ok_or(RiskParamsError::Overflow)?;

    Ok(result)
}

/// Require minimum collateral ratio
pub fn require_min_collateral_ratio(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<(), RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;

    if debt_value == 0 {
        return Ok(());
    }

    let ratio = (collateral_value * BASIS_POINTS_SCALE)
        .checked_div(debt_value)
        .ok_or(RiskParamsError::InvalidParameter)?;

    if ratio < config.min_collateral_ratio {
        return Err(RiskParamsError::InvalidCollateralRatio);
    }

    Ok(())
}

/// Can be liquidated check
pub fn can_be_liquidated(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<bool, RiskParamsError> {
    let config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;

    if debt_value == 0 {
        return Ok(false);
    }

    let ratio = (collateral_value * BASIS_POINTS_SCALE)
        .checked_div(debt_value)
        .ok_or(RiskParamsError::InvalidParameter)?;

    Ok(ratio < config.liquidation_threshold)
}
