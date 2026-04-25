#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::cross_asset::{AssetParams, CrossAssetError};
use crate::borrow::BorrowError;
use crate::oracle::{OracleError, OracleConfig};
use crate::deposit::DepositError;
use crate::constants::*;

fn setup(env: &Env) -> (LendingContractClient<'_>, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_admin(&admin);
    (client, admin)
}

#[test]
fn test_property_ltv_validation() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Invalid LTVs (below min, above max, extreme values)
    let invalid_ltvs = std::vec![MIN_LTV_BPS - 1, MAX_LTV_BPS + 1, -1, 10001];
    for ltv in invalid_ltvs {
        let params = AssetParams {
            ltv,
            liquidation_threshold: 8000,
            debt_ceiling: 100_000,
            price_feed: oracle.clone(),
            is_active: true,
        };
        let res = client.try_set_asset_params(&admin, &asset, &params);
        assert_eq!(res, Err(Ok(CrossAssetError::InvalidParameterRange)), "LTV {} should be invalid", ltv);
    }

    // Valid boundary LTVs
    let valid_ltvs = std::vec![MIN_LTV_BPS, MAX_LTV_BPS, (MIN_LTV_BPS + MAX_LTV_BPS) / 2];
    for ltv in valid_ltvs {
        let params = AssetParams {
            ltv,
            liquidation_threshold: 10000,
            debt_ceiling: 100_000,
            price_feed: oracle.clone(),
            is_active: true,
        };
        let res = client.try_set_asset_params(&admin, &asset, &params);
        assert!(res.is_ok(), "LTV {} should be valid", ltv);
    }
}

#[test]
fn test_property_threshold_validation() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Threshold MUST be > LTV and <= 10000
    let ltv = 5000;
    let invalid_thresholds = std::vec![ltv, ltv - 1, 10001, -1];
    for threshold in invalid_thresholds {
        let params = AssetParams {
            ltv,
            liquidation_threshold: threshold,
            debt_ceiling: 100_000,
            price_feed: oracle.clone(),
            is_active: true,
        };
        let res = client.try_set_asset_params(&admin, &asset, &params);
        assert_eq!(res, Err(Ok(CrossAssetError::InvalidParameterRange)), "Threshold {} should be invalid for LTV {}", threshold, ltv);
    }

    // Valid boundary
    let params = AssetParams {
        ltv,
        liquidation_threshold: ltv + 1,
        debt_ceiling: 100_000,
        price_feed: oracle.clone(),
        is_active: true,
    };
    assert!(client.try_set_asset_params(&admin, &asset, &params).is_ok());
}

#[test]
fn test_property_bps_bounds_borrow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    // Close Factor: valid 1-10000
    let invalid_close_factors = std::vec![0, 10001, -1, i128::MIN, i128::MAX];
    for bps in invalid_close_factors {
        assert_eq!(client.try_set_close_factor_bps(&admin, &bps), Err(Ok(BorrowError::InvalidParameterRange)));
    }
    assert!(client.try_set_close_factor_bps(&admin, &1).is_ok());
    assert!(client.try_set_close_factor_bps(&admin, &10000).is_ok());

    // Liquidation Incentive: valid 0-2000 (MAX_LIQUIDATION_INCENTIVE_BPS)
    let invalid_incentives = std::vec![-1, 2001, i128::MAX];
    for bps in invalid_incentives {
        assert_eq!(client.try_set_liquidation_incentive_bps(&admin, &bps), Err(Ok(BorrowError::InvalidParameterRange)));
    }
    assert!(client.try_set_liquidation_incentive_bps(&admin, &0).is_ok());
    assert!(client.try_set_liquidation_incentive_bps(&admin, &2000).is_ok());
}

#[test]
fn test_property_staleness_bounds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let invalid_staleness = std::vec![
        MIN_ORACLE_STALENESS_SECONDS as u64 - 1,
        MAX_ORACLE_STALENESS_SECONDS as u64 + 1,
        0,
    ];

    for s in invalid_staleness {
        let config = OracleConfig {
            max_staleness_seconds: s,
        };
        let res = client.try_configure_oracle(&admin, &config);
        assert_eq!(res, Err(Ok(OracleError::InvalidParameterRange)), "Staleness {} should be invalid", s);
    }
}

#[test]
fn test_property_deposit_withdraw_bounds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    // Deposit bounds
    assert_eq!(client.try_initialize_deposit_settings(&-1, &0), Err(Ok(DepositError::InvalidParameterRange)));
    assert_eq!(client.try_initialize_deposit_settings(&0, &-1), Err(Ok(DepositError::InvalidParameterRange)));
    assert!(client.try_initialize_deposit_settings(&0, &0).is_ok());

    // Withdraw bounds
    assert_eq!(client.try_initialize_withdraw_settings(&-1), Err(Ok(WithdrawError::InvalidParameterRange)));
    assert!(client.try_initialize_withdraw_settings(&0).is_ok());
    assert!(client.try_initialize_withdraw_settings(&100).is_ok());
}

#[test]
fn test_property_flash_loan_fee_bounds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    // valid: 0-1000 (MAX_FLASH_LOAN_FEE_BPS)
    let invalid_fees = std::vec![-1, 1001, 10001, i128::MAX];
    for fee in invalid_fees {
        assert_eq!(client.try_set_flash_loan_fee_bps(&fee), Err(Ok(FlashLoanError::InvalidFee)));
    }
    assert!(client.try_set_flash_loan_fee_bps(&0).is_ok());
    assert!(client.try_set_flash_loan_fee_bps(&1000).is_ok());
}

#[test]
fn test_property_helper_direct_validation() {
    // Test helpers that don't have direct contract entry points yet
    use crate::validation;
    
    // Utilization kink: (0, 10000)
    assert!(!validation::is_valid_utilization_kink(0));
    assert!(!validation::is_valid_utilization_kink(10000));
    assert!(!validation::is_valid_utilization_kink(-1));
    assert!(validation::is_valid_utilization_kink(1));
    assert!(validation::is_valid_utilization_kink(8000));
    assert!(validation::is_valid_utilization_kink(9999));

    // Multiplier: [0, 20000]
    assert!(!validation::is_valid_multiplier(-1));
    assert!(validation::is_valid_multiplier(0));
    assert!(validation::is_valid_multiplier(10000));
    assert!(validation::is_valid_multiplier(20000));
    assert!(!validation::is_valid_multiplier(20001));
}
