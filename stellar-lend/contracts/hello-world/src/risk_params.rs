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
fn validate_risk_params(config: &RiskParams) -> Result<(), RiskParamsError> {
    // Validate min collateral ratio
    if config.min_collateral_ratio < MIN_COLLATERAL_RATIO_MIN
        || config.min_collateral_ratio > MIN_COLLATERAL_RATIO_MAX
    {
        return Err(RiskParamsError::InvalidParameter);
    }

    // Validate liquidation threshold
    if config.liquidation_threshold < LIQUIDATION_THRESHOLD_MIN
        || config.liquidation_threshold > LIQUIDATION_THRESHOLD_MAX
    {
        return Err(RiskParamsError::InvalidLiquidationThreshold);
    }

    // Validate that min collateral ratio >= liquidation threshold
    if config.min_collateral_ratio < config.liquidation_threshold {
        return Err(RiskParamsError::InvalidCollateralRatio);
    }

    // Validate close factor
    if config.close_factor < CLOSE_FACTOR_MIN || config.close_factor > CLOSE_FACTOR_MAX {
        return Err(RiskParamsError::InvalidCloseFactor);
    }

    // Validate liquidation incentive
    if config.liquidation_incentive < LIQUIDATION_INCENTIVE_MIN
        || config.liquidation_incentive > LIQUIDATION_INCENTIVE_MAX
    {
        return Err(RiskParamsError::InvalidLiquidationIncentive);
    }

    Ok(())
}

/// Validate parameter change doesn't exceed maximum allowed change
fn validate_parameter_change(old_value: i128, new_value: i128) -> Result<(), RiskParamsError> {
    let change = if new_value > old_value {
        new_value - old_value
    } else {
        old_value - new_value
    };

    // Calculate maximum allowed change (10% of old value)
    let max_change = (old_value * MAX_PARAMETER_CHANGE_BPS) / BASIS_POINTS_SCALE;

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
pub fn set_risk_params(
    env: &Env,
    min_collateral_ratio: Option<i128>,
    liquidation_threshold: Option<i128>,
    close_factor: Option<i128>,
    liquidation_incentive: Option<i128>,
) -> Result<(), RiskParamsError> {
    let mut config = get_risk_params(env).ok_or(RiskParamsError::InvalidParameter)?;

    // Update parameters if provided
    if let Some(mcr) = min_collateral_ratio {
        validate_parameter_change(config.min_collateral_ratio, mcr)?;
        config.min_collateral_ratio = mcr;
    }

    if let Some(lt) = liquidation_threshold {
        validate_parameter_change(config.liquidation_threshold, lt)?;
        config.liquidation_threshold = lt;
    }

    if let Some(cf) = close_factor {
        validate_parameter_change(config.close_factor, cf)?;
        config.close_factor = cf;
    }

    if let Some(li) = liquidation_incentive {
        validate_parameter_change(config.liquidation_incentive, li)?;
        config.liquidation_incentive = li;
    }

    // Validate the updated config
    validate_risk_params(&config)?;

    // Update timestamp
    config.last_update = env.ledger().timestamp();

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
