//! # Analytics and Metrics Tests (#301)
//!
//! Tests for on-contract analytics: protocol metrics (TVL, volume, utilization)
//! updated on core actions (deposit, borrow, repay, withdraw) and exposed via getters.
//! Covers get_protocol_report, get_user_report, edge cases (first deposit, full withdraw).

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

// =============================================================================
// TVL and protocol report
// =============================================================================

#[test]
fn test_protocol_report_tvl_after_first_deposit() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &5000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 5000);
    assert_eq!(report.metrics.total_deposits, 5000);
}

#[test]
fn test_protocol_report_tvl_after_multiple_deposits() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);

    client.deposit_collateral(&u1, &None, &3000);
    client.deposit_collateral(&u2, &None, &2000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 5000);
}

#[test]
fn test_protocol_report_utilization() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &4000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.utilization_rate, 4000);
}

#[test]
fn test_protocol_report_total_borrows_volume() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &2000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_borrows, 2000);
}

// =============================================================================
// Edge cases: first deposit, full withdraw
// =============================================================================

#[test]
fn test_analytics_after_full_withdraw() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    client.ca_withdraw_collateral(&user, &None, &1000);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 0);
}

#[test]
fn test_analytics_utilization_zero_when_no_deposits() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 0);
    assert_eq!(report.metrics.utilization_rate, 0);
}

#[test]
fn test_analytics_user_report_after_repay() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &1000);
    token_client.approve(&user, &contract_id, &1000, &(env.ledger().sequence() + 100));

    client.deposit_collateral(&user, &None, &5000);
    client.borrow_asset(&user, &None, &1000);
    client.repay_debt(&user, &None, &1000);

    let report = client.get_user_report(&user);
    assert_eq!(report.metrics.total_repayments, 1000);
    assert_eq!(report.position.debt, 0);
}

#[test]
fn test_analytics_timestamp_present() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let _user = Address::generate(&env);
    client.deposit_collateral(&_user, &None, &100);
    let report = client.get_protocol_report();
    let _ = report.timestamp;
}

#[test]
fn test_analytics_metrics_no_overflow_large_values() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let _user = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let key = DepositDataKey::ProtocolAnalytics;
        let a = ProtocolAnalytics {
            total_deposits: 1_000_000_000,
            total_borrows: 500_000_000,
            total_value_locked: 1_000_000_000,
        };
        env.storage().persistent().set(&key, &a);
    });

    let report = client.get_protocol_report();
    assert_eq!(report.metrics.total_value_locked, 1_000_000_000);
    assert_eq!(report.metrics.utilization_rate, 5000);
}

#[test]
fn test_analytics_average_borrow_rate_non_negative() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);
    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &1000);
    let report = client.get_protocol_report();
    assert!(report.metrics.average_borrow_rate >= 0);
}
