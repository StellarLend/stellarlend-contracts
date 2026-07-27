#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::risk_management::{
    get_close_factor, get_liquidation_incentive, get_liquidation_threshold,
    get_min_collateral_ratio, initialize_risk_management, set_risk_params, RiskManagementError,
};

const DEFAULT_MIN_COLLATERAL_RATIO_BPS: i128 = 15_000;
const DEFAULT_LIQUIDATION_THRESHOLD_BPS: i128 = 12_000;
const DEFAULT_CLOSE_FACTOR_BPS: i128 = 5_000;
const DEFAULT_LIQUIDATION_INCENTIVE_BPS: i128 = 500;

fn make_env() -> Env {
    Env::default()
}

fn with_contract<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce() -> T,
{
    let contract_id = env.register(crate::cross_asset::NoOpContract {}, ());
    env.as_contract(&contract_id, f)
}

fn init_with_admin() -> (Env, Address) {
    let env = make_env();
    let admin = Address::generate(&env);
    env.mock_all_auths();
    with_contract(&env, || {
        crate::admin::set_admin(&env, admin.clone(), None).unwrap();
        initialize_risk_management(&env, admin.clone()).unwrap();
    });
    (env, admin)
}

#[test]
fn test_initialize_sets_documented_defaults() {
    let env = make_env();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        crate::admin::set_admin(&env, admin.clone(), None).unwrap();
        initialize_risk_management(&env, admin.clone()).unwrap();

        assert_eq!(
            get_min_collateral_ratio(&env).unwrap(),
            DEFAULT_MIN_COLLATERAL_RATIO_BPS
        );
        assert_eq!(
            get_liquidation_threshold(&env).unwrap(),
            DEFAULT_LIQUIDATION_THRESHOLD_BPS
        );
        assert_eq!(get_close_factor(&env).unwrap(), DEFAULT_CLOSE_FACTOR_BPS);
        assert_eq!(
            get_liquidation_incentive(&env).unwrap(),
            DEFAULT_LIQUIDATION_INCENTIVE_BPS
        );
    });
}

#[test]
fn test_min_cr_accepts_exactly_50pct_up() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), Some(22_500), None, None, None);
        assert_eq!(r, Ok(()));
        assert_eq!(get_min_collateral_ratio(&env).unwrap(), 22_500);
    });
}

#[test]
fn test_min_cr_rejects_more_than_50pct_up() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), Some(22_501), None, None, None);
        assert_eq!(r, Err(RiskManagementError::ParameterChangeTooLarge));
        assert_eq!(
            get_min_collateral_ratio(&env).unwrap(),
            DEFAULT_MIN_COLLATERAL_RATIO_BPS
        );
    });
}

#[test]
fn test_min_cr_accepts_floor_at_50pct_down() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), Some(10_000), None, None, None);
        assert_eq!(r, Ok(()));
    });
}

#[test]
fn test_min_cr_below_absolute_floor_still_rejected() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), Some(9_999), None, None, None);
        assert_eq!(r, Err(RiskManagementError::InvalidCollateralRatio));
    });
}

#[test]
fn test_liquidation_threshold_rejects_more_than_50pct_up() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, Some(18_001), None, None);
        assert_eq!(r, Err(RiskManagementError::ParameterChangeTooLarge));
    });
}

#[test]
fn test_close_factor_rejects_more_than_50pct_up() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, None, Some(7_501), None);
        assert_eq!(r, Err(RiskManagementError::ParameterChangeTooLarge));
        assert_eq!(get_close_factor(&env).unwrap(), DEFAULT_CLOSE_FACTOR_BPS);
    });
}

#[test]
fn test_close_factor_accepts_exactly_50pct_up() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, None, Some(7_500), None);
        assert_eq!(r, Ok(()));
        assert_eq!(get_close_factor(&env).unwrap(), 7_500);
    });
}

#[test]
fn test_close_factor_zero_still_rejected() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, None, Some(0), None);
        assert_eq!(r, Err(RiskManagementError::InvalidCloseFactor));
    });
}

#[test]
fn test_close_factor_above_absolute_ceiling_still_rejected() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, None, Some(10_001), None);
        assert_eq!(r, Err(RiskManagementError::InvalidCloseFactor));
    });
}

#[test]
fn test_liquidation_incentive_rejects_more_than_50pct_up() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, None, None, Some(751));
        assert_eq!(r, Err(RiskManagementError::ParameterChangeTooLarge));
        assert_eq!(
            get_liquidation_incentive(&env).unwrap(),
            DEFAULT_LIQUIDATION_INCENTIVE_BPS
        );
    });
}

#[test]
fn test_liquidation_incentive_accepts_exactly_50pct_up() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, None, None, Some(750));
        assert_eq!(r, Ok(()));
        assert_eq!(get_liquidation_incentive(&env).unwrap(), 750);
    });
}

#[test]
fn test_liquidation_incentive_jump_to_zero_blocked_by_paced_change() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        let r = set_risk_params(&env, admin.clone(), None, None, None, Some(0));
        assert_eq!(r, Err(RiskManagementError::ParameterChangeTooLarge));
    });
}

#[test]
fn test_close_factor_third_step_blocked_after_two_paced_steps() {
    let (env, admin) = init_with_admin();
    with_contract(&env, || {
        set_risk_params(&env, admin.clone(), None, None, Some(7_500), None).unwrap();
        assert_eq!(get_close_factor(&env).unwrap(), 7_500);

        set_risk_params(&env, admin.clone(), None, None, Some(10_000), None).unwrap();
        assert_eq!(get_close_factor(&env).unwrap(), 10_000);

        let r = set_risk_params(&env, admin.clone(), None, None, Some(10_001), None);
        assert_eq!(r, Err(RiskManagementError::InvalidCloseFactor));
    });
}
