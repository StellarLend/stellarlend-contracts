#![cfg(test)]

use crate::risk_params::{RiskParams, RiskParamsError, validate_risk_params, validate_parameter_change};
use soroban_sdk::{Env, testutils::Address as _};

#[test]
fn test_hardened_insufficient_safety_margin() {
    // Test that liquidation threshold too close to min CR fails
    let config = RiskParams {
        min_collateral_ratio: 11_000,  // 110%
        liquidation_threshold: 10_600,  // 106% - only 4% margin, need 5%
        close_factor: 5_000,
        liquidation_incentive: 1_000,
        last_update: 0,
    };
    
    let result = validate_risk_params(&config);
    assert_eq!(result, Err(RiskParamsError::InsufficientSafetyMargin));
}

#[test]
fn test_hardened_valid_safety_margin() {
    // Test that exactly 5% safety margin passes
    let config = RiskParams {
        min_collateral_ratio: 11_000,  // 110%
        liquidation_threshold: 10_500,  // 105% - exactly 5% margin
        close_factor: 5_000,
        liquidation_incentive: 1_000,
        last_update: 0,
    };
    
    let result = validate_risk_params(&config);
    assert!(result.is_ok());
}

#[test]
fn test_hardened_conservative_close_factor_limit() {
    // Test that close factor above 75% fails
    let config = RiskParams {
        min_collateral_ratio: 11_000,
        liquidation_threshold: 10_500,
        close_factor: 8_000,  // 80% - above 75% conservative limit
        liquidation_incentive: 1_000,
        last_update: 0,
    };
    
    let result = validate_risk_params(&config);
    assert_eq!(result, Err(RiskParamsError::InvalidCloseFactor));
}

#[test]
fn test_hardened_conservative_liquidation_incentive_limit() {
    // Test that liquidation incentive above 25% fails
    let config = RiskParams {
        min_collateral_ratio: 11_000,
        liquidation_threshold: 10_500,
        close_factor: 5_000,
        liquidation_incentive: 3_000,  // 30% - above 25% conservative limit
        last_update: 0,
    };
    
    let result = validate_risk_params(&config);
    assert_eq!(result, Err(RiskParamsError::InvalidLiquidationIncentive));
}

#[test]
fn test_hardened_invalid_parameter_combination_incentive_exceeds_close_factor() {
    // Test that liquidation incentive higher than close factor fails
    let config = RiskParams {
        min_collateral_ratio: 11_000,
        liquidation_threshold: 10_500,
        close_factor: 1_000,   // 10%
        liquidation_incentive: 1_500,  // 15% - higher than close factor
        last_update: 0,
    };
    
    let result = validate_risk_params(&config);
    assert_eq!(result, Err(RiskParamsError::InvalidParameterCombination));
}

#[test]
fn test_hardened_invalid_parameter_combination_total_benefit_exceeds_100() {
    // Test that close factor + liquidation incentive = 100% passes (boundary case)
    let config = RiskParams {
        min_collateral_ratio: 11_000,
        liquidation_threshold: 10_500,
        close_factor: 7_500,   // 75% (max safe)
        liquidation_incentive: 2_500,  // 25% (max safe) - total 100%
        last_update: 0,
    };
    
    // This should pass since total is exactly 100%
    let result = validate_risk_params(&config);
    assert!(result.is_ok());
}

#[test]
fn test_hardened_conservative_change_limit() {
    // Test that change larger than 5% fails
    let old_value = 11_000;
    let new_value = 11_600;  // 5.45% change - above 5% limit
    let last_update = 0;
    let current_time = 3601;  // More than 1 hour later
    
    let result = validate_parameter_change(old_value, new_value, last_update, current_time);
    assert_eq!(result, Err(RiskParamsError::ParameterChangeTooLarge));
}

#[test]
fn test_hardened_time_based_change_restriction() {
    // Test that change too soon fails
    let old_value = 11_000;
    let new_value = 11_100;  // Small change within limits
    let last_update = 1000;
    let current_time = 1500;  // Only 500 seconds later, need 3600
    
    let result = validate_parameter_change(old_value, new_value, last_update, current_time);
    assert_eq!(result, Err(RiskParamsError::ParameterChangeTooLarge));
}

#[test]
fn test_hardened_valid_change_after_time_delay() {
    // Test that change after sufficient time passes
    let old_value = 11_000;
    let new_value = 11_100;  // Small change within limits
    let last_update = 1000;
    let current_time = 4601;  // More than 1 hour later
    
    let result = validate_parameter_change(old_value, new_value, last_update, current_time);
    assert!(result.is_ok());
}

#[test]
fn test_hardened_valid_conservative_limits() {
    // Test parameters at conservative limits
    let config = RiskParams {
        min_collateral_ratio: 11_000,
        liquidation_threshold: 10_500,
        close_factor: 7_500,   // 75% - at conservative limit
        liquidation_incentive: 2_500,  // 25% - at conservative limit
        last_update: 0,
    };
    
    let result = validate_risk_params(&config);
    assert!(result.is_ok());
}

#[test]
fn test_hardened_valid_parameter_combination() {
    // Test valid parameter combination
    let config = RiskParams {
        min_collateral_ratio: 11_000,
        liquidation_threshold: 10_500,
        close_factor: 5_000,   // 50%
        liquidation_incentive: 1_000,  // 10% - total 60%, incentive < close factor
        last_update: 0,
    };
    
    let result = validate_risk_params(&config);
    assert!(result.is_ok());
}