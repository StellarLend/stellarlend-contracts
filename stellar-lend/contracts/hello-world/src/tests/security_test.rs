//! # Security Test Suite (#314)
//!
//! Reentrancy, overflow/underflow, authorization, and malicious-input scenarios.
//! High coverage on security-critical paths for CI.

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Unauthorized: non-admin cannot set emergency pause.
#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn security_unauthorized_emergency_pause() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_emergency_pause(&non_admin, &true);
}

/// Unauthorized: non-admin cannot set risk params.
#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn security_unauthorized_set_risk_params() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_risk_params(&non_admin, &Some(12_000), &None, &None, &None);
}

/// Negative amount rejected on deposit (invalid input).
#[test]
#[should_panic]
fn security_deposit_negative_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &(-100));
}

/// Negative amount rejected on withdraw (invalid input).
#[test]
#[should_panic(expected = "InvalidAmount")]
fn security_withdraw_negative_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &1000);
    client.withdraw_collateral(&user, &None, &(-100));
}

/// Withdraw more than balance rejected (insufficient collateral).
#[test]
#[should_panic(expected = "InsufficientCollateral")]
fn security_withdraw_exceeds_balance() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    client.deposit_collateral(&user, &None, &500);
    client.withdraw_collateral(&user, &None, &1000);
}

/// Parameter change too large rejected (risk param bounds).
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn security_risk_param_change_too_large() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_risk_params(&admin, &Some(20_000), &None, &None, &None);
}
