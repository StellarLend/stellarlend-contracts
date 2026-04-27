//! # View and Health Factor Tests (#298)
//!
//! Comprehensive test suite for view functions and health factor calculation.
//! Covers get_user_report (position), get_health_factor via report, collateral/debt balances,
//! and edge cases (no debt, boundary health, risk getters).

use crate::deposit::{DepositDataKey, Position, ProtocolAnalytics};
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

/// Helper to set user position directly for boundary tests
fn set_user_position(
    env: &Env,
    contract_id: &Address,
    user: &Address,
    collateral: i128,
    debt: i128,
    borrow_interest: i128,
) {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        let now = env.ledger().timestamp();
        let position = Position {
            collateral,
            debt,
            borrow_interest,
            last_accrual_time: now,
        };
        env.storage().persistent().set(&key, &position);
    });
}

// =============================================================================
// get_user_report / position view tests
// =============================================================================

#[test]
fn test_get_user_report_after_deposit() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &2000);
    let report = client.get_user_report(&user);

    assert_eq!(report.position.collateral, 2000);
    assert_eq!(report.position.debt, 0);
    assert_eq!(report.metrics.collateral, 2000);
    assert_eq!(report.metrics.debt, 0);
}

#[test]
fn test_get_user_report_collateral_and_debt_balances() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &5000);
    client.borrow_asset(&user, &None, &1000);

    let report = client.get_user_report(&user);
    assert_eq!(report.position.collateral, 5000);
    assert_eq!(report.position.debt, 1000);
    assert_eq!(report.metrics.collateral, 5000);
    assert_eq!(report.metrics.debt, 1000);
}

#[test]
fn test_get_user_report_after_withdraw() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &3000);
    client.withdraw_collateral(&user, &None, &500);

    let report = client.get_user_report(&user);
    assert_eq!(report.position.collateral, 2500);
}

#[test]
fn test_get_user_report_after_repay() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &2500);

    client.deposit_collateral(&user, &None, &5000);
    client.borrow_asset(&user, &None, &2000);
    token_client.approve(&user, &contract_id, &500, &(env.ledger().sequence() + 100));
    client.repay_debt(&user, &None, &500);

    let report = client.get_user_report(&user);
    assert_eq!(report.position.debt, 1500);
}

// =============================================================================
// Health factor tests
// =============================================================================

#[test]
fn test_health_factor_no_debt() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &1000);
    let report = client.get_user_report(&user);
    assert_eq!(report.metrics.health_factor, i128::MAX);
}

#[test]
fn test_health_factor_at_boundary_150_percent() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &100);
    set_user_position(&env, &contract_id, &user, 15000, 10000, 0);
    let report = client.get_user_report(&user);
    assert_eq!(report.metrics.health_factor, 15000);
}

#[test]
fn test_health_factor_at_boundary_100_percent() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &100);
    set_user_position(&env, &contract_id, &user, 10000, 10000, 0);
    let report = client.get_user_report(&user);
    assert_eq!(report.metrics.health_factor, 10000);
}

#[test]
fn test_health_factor_below_threshold() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &100);
    set_user_position(&env, &contract_id, &user, 9000, 10000, 0);
    let report = client.get_user_report(&user);
    assert_eq!(report.metrics.health_factor, 9000);
}

#[test]
fn test_health_factor_risk_level_reflected() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &100);
    set_user_position(&env, &contract_id, &user, 20000, 10000, 0);
    let report = client.get_user_report(&user);
    assert!(report.metrics.health_factor >= 15000);
    assert_eq!(report.metrics.risk_level, 1);
}

// =============================================================================
// Risk / view getters
// =============================================================================

#[test]
fn test_get_risk_config_after_init() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);

    let config = client.get_risk_config().unwrap();
    assert!(config.min_collateral_ratio > 0);
    assert!(config.liquidation_threshold > 0);
    assert!(config.close_factor > 0);
    assert!(config.liquidation_incentive > 0);
}

#[test]
fn test_get_min_collateral_ratio() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let ratio = client.get_min_collateral_ratio();
    assert!(ratio > 0);
}

#[test]
fn test_get_liquidation_threshold() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let threshold = client.get_liquidation_threshold();
    assert!(threshold > 0);
}

#[test]
fn test_get_utilization_view() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);

    env.as_contract(&contract_id, || {
        let key = DepositDataKey::ProtocolAnalytics;
        let a = ProtocolAnalytics {
            total_deposits: 10000,
            total_borrows: 3000,
            total_value_locked: 10000,
        };
        env.storage().persistent().set(&key, &a);
    });

    let util = client.get_utilization();
    assert_eq!(util, 3000);
}

#[test]
fn test_get_borrow_rate_and_supply_rate() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);

    let borrow_rate = client.get_borrow_rate();
    let supply_rate = client.get_supply_rate();
    assert!(borrow_rate >= 0);
    assert!(supply_rate >= 0);
    assert!(supply_rate <= borrow_rate);
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
#[should_panic(expected = "HostError")]
fn test_get_user_report_no_activity_fails() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);
    let _ = client.get_user_report(&user);
}

#[test]
fn test_position_consistency_after_multiple_ops() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &2500);

    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &2000);
    client.deposit_collateral(&user, &None, &1000);
    token_client.approve(&user, &contract_id, &500, &(env.ledger().sequence() + 100));
    client.repay_debt(&user, &None, &500);

    let report = client.get_user_report(&user);
    assert_eq!(report.position.collateral, 11000);
    assert_eq!(report.position.debt, 1500);
    assert_eq!(report.metrics.collateral, report.position.collateral);
    assert_eq!(report.metrics.debt, report.position.debt);
}

#[test]
fn test_two_users_independent_positions() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.deposit_collateral(&user1, &None, &5000);
    client.deposit_collateral(&user2, &None, &3000);
    client.borrow_asset(&user1, &None, &1000);

    let r1 = client.get_user_report(&user1);
    let r2 = client.get_user_report(&user2);
    assert_eq!(r1.position.collateral, 5000);
    assert_eq!(r1.position.debt, 1000);
    assert_eq!(r2.position.collateral, 3000);
    assert_eq!(r2.position.debt, 0);
}
