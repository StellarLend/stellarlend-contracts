use soroban_sdk::Env;
use crate::risk_management::RiskManagementError;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RiskParamsError {
    InvalidParameter = 1,
}

pub fn initialize_risk_params(_env: &Env) -> Result<(), RiskManagementError> {
    Ok(())
}

pub fn set_risk_params(
    _env: &Env,
    _min_collateral_ratio: Option<i128>,
    _liquidation_threshold: Option<i128>,
    _close_factor: Option<i128>,
    _liquidation_incentive: Option<i128>,
) -> Result<(), RiskManagementError> {
    Ok(())
}

pub fn can_be_liquidated(_env: &Env, _collateral_value: i128, _debt_value: i128) -> Result<bool, RiskManagementError> {
    Ok(false)
}

pub fn get_liquidation_incentive_amount(_env: &Env) -> i128 {
    0
}

pub fn get_max_liquidatable_amount(_env: &Env, _debt_value: i128) -> Result<i128, RiskManagementError> {
    Ok(0)
}

pub fn require_min_collateral_ratio(_env: &Env, _collateral_value: i128, _debt_value: i128) -> Result<(), RiskManagementError> {
    Ok(())
}
