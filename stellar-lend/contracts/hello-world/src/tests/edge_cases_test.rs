//! # Edge Cases and Security Test Suite (#314)
//!
//! Covers boundary conditions, overflow/underflow resistance, unauthorized access,
//! and malicious or boundary inputs. Run as part of CI for security-critical paths.

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Non-admin cannot set risk params (authorization).
#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn edge_unauthorized_set_risk_params() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_risk_params(&non_admin, &Some(12_000), &None, &None, &None);
}

/// Non-admin cannot set pause switch (authorization).
#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn edge_unauthorized_set_pause_switch() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_pause_switch(&non_admin, &Symbol::new(&env, "pause_deposit"), &true);
}

/// Boundary: deposit zero amount rejected.
#[test]
#[should_panic(expected = "InvalidAmount")]
fn edge_deposit_zero_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &0);
}

/// Boundary: withdraw zero amount rejected.
#[test]
#[should_panic(expected = "InvalidAmount")]
fn edge_withdraw_zero_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &1000);
    client.withdraw_collateral(&user, &None, &0);
}

/// Boundary: borrow zero amount rejected.
#[test]
#[should_panic(expected = "InvalidAmount")]
fn edge_borrow_zero_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &1000);
    client.borrow_asset(&user, &None, &0);
}

/// Boundary: repay zero amount rejected.
#[test]
#[should_panic(expected = "InvalidAmount")]
fn edge_repay_zero_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &1000);
    client.borrow_asset(&user, &None, &100);
    client.repay_debt(&user, &None, &0);
}

/// Boundary: require_min_collateral_ratio at exact boundary (110%) succeeds.
#[test]
fn edge_require_min_collateral_ratio_boundary() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.require_min_collateral_ratio(&1_100, &1_000);
}

/// Boundary: can_be_liquidated at exact threshold (105%) is false.
#[test]
fn edge_can_be_liquidated_at_threshold() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert!(!client.can_be_liquidated(&1_050, &1_000));
}

/// Boundary: get_max_liquidatable_amount with zero debt returns zero.
#[test]
fn edge_max_liquidatable_zero_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.get_max_liquidatable_amount(&0), 0);
}
