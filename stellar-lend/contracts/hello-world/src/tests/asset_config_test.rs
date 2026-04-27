//! # Asset and Protocol Config Tests (#310)
//!
//! Tests for collateral/asset configuration and config enforcement.
//! Covers interest rate config, risk params, and per-parameter validation.

use crate::deposit::{DepositDataKey, ProtocolAnalytics};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract_with_admin(env: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, admin, client)
}

fn set_protocol_analytics(
    env: &Env,
    contract_id: &Address,
    total_deposits: i128,
    total_borrows: i128,
) {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::ProtocolAnalytics;
        let a = ProtocolAnalytics {
            total_deposits,
            total_borrows,
            total_value_locked: total_deposits,
        };
        env.storage().persistent().set(&key, &a);
    });
}

// =============================================================================
// Interest rate config (protocol-level "asset" config)
// =============================================================================

#[test]
fn test_update_interest_rate_config_base_rate() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &Some(200),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    let rate = client.get_borrow_rate();
    assert!(rate >= 200);
}

#[test]
fn test_update_interest_rate_config_kink() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 5000);
    client.update_interest_rate_config(
        &admin,
        &None,
        &Some(5000),
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    let util = client.get_utilization();
    assert_eq!(util, 5000);
}

#[test]
fn test_update_interest_rate_config_spread() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(300),
    );
    let borrow_rate = client.get_borrow_rate();
    let supply_rate = client.get_supply_rate();
    assert!(borrow_rate >= supply_rate);
}

#[test]
fn test_interest_rate_config_floor_ceiling_enforcement() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 100, 0);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &Some(100),
        &Some(10000),
        &None,
        &None,
    );
    let rate = client.get_borrow_rate();
    assert!(rate >= 100);
    assert!(rate <= 10000);
}

// =============================================================================
// Risk config (min collateral ratio, liquidation threshold, etc.)
// =============================================================================

#[test]
fn test_get_risk_config_returns_all_params() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let config = client.get_risk_config().unwrap();
    assert!(config.min_collateral_ratio > 0);
    assert!(config.min_collateral_ratio >= config.liquidation_threshold);
    assert!(config.close_factor > 0);
    assert!(config.close_factor <= 10_000);
    assert!(config.liquidation_incentive > 0);
}

#[test]
fn test_set_risk_params_success() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let config_before = client.get_risk_config().unwrap();
    let new_min_cr = config_before.min_collateral_ratio + 100;
    if new_min_cr <= 10_000 {
        client.set_risk_params(&admin, &Some(new_min_cr), &None, &None, &None);
        let config_after = client.get_risk_config().unwrap();
        assert_eq!(config_after.min_collateral_ratio, new_min_cr);
    }
}

#[test]
fn test_min_collateral_ratio_getter() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let ratio = client.get_min_collateral_ratio();
    assert!(ratio > 0);
}

#[test]
fn test_liquidation_threshold_getter() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let threshold = client.get_liquidation_threshold();
    assert!(threshold > 0);
}

#[test]
fn test_close_factor_getter() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let cf = client.get_close_factor();
    assert!(cf > 0);
    assert!(cf <= 10_000);
}

#[test]
fn test_liquidation_incentive_getter() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let inc = client.get_liquidation_incentive();
    assert!(inc > 0);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_update_interest_rate_config_unauthorized() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let non_admin = Address::generate(&env);
    client.update_interest_rate_config(
        &non_admin,
        &Some(500),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
}
