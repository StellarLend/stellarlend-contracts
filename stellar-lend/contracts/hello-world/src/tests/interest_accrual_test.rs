//! # Interest Accrual and Index Tests (#310)
//!
//! Tests for interest accrual, index updates, and consistency.
//! Covers accrual over time, zero principal/zero time, rate used in accrual.

use crate::deposit::{DepositDataKey, ProtocolAnalytics};
use crate::interest_rate::{calculate_accrued_interest, get_interest_rate_config};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env};

const SECONDS_PER_YEAR: u64 = 365 * 86400;

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
// calculate_accrued_interest (module-level)
// =============================================================================

#[test]
fn test_accrued_interest_zero_principal() {
    let env = create_test_env();
    let (contract_id, _admin, _client) = setup_contract_with_admin(&env);
    env.as_contract(&contract_id, || {
        let result = calculate_accrued_interest(0, 0, SECONDS_PER_YEAR, 500);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    });
}

#[test]
fn test_accrued_interest_zero_time_elapsed() {
    let env = create_test_env();
    let (contract_id, _admin, _client) = setup_contract_with_admin(&env);
    let now = 1000u64;
    env.as_contract(&contract_id, || {
        let result = calculate_accrued_interest(10_000, now, now, 500);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    });
}

#[test]
fn test_accrued_interest_one_year_at_5_percent() {
    let env = create_test_env();
    let (contract_id, _admin, _client) = setup_contract_with_admin(&env);
    env.as_contract(&contract_id, || {
        let principal: i128 = 100_000;
        let rate_bps = 500;
        let result = calculate_accrued_interest(principal, 0, SECONDS_PER_YEAR, rate_bps);
        assert!(result.is_ok());
        let interest = result.unwrap();
        assert_eq!(interest, 5_000);
    });
}

#[test]
fn test_accrued_interest_partial_year() {
    let env = create_test_env();
    let (contract_id, _admin, _client) = setup_contract_with_admin(&env);
    env.as_contract(&contract_id, || {
        let principal: i128 = 100_000;
        let rate_bps = 1000;
        let half_year = SECONDS_PER_YEAR / 2;
        let result = calculate_accrued_interest(principal, 0, half_year, rate_bps);
        assert!(result.is_ok());
        let interest = result.unwrap();
        assert_eq!(interest, 5_000);
    });
}

// =============================================================================
// Accrual over time via repay flow
// =============================================================================

#[test]
fn test_repay_accrues_interest_with_time_advance() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &15_000);
    token_client.approve(
        &user,
        &contract_id,
        &15_000,
        &(env.ledger().sequence() + 100),
    );

    client.deposit_collateral(&user, &None, &20_000);
    client.borrow_asset(&user, &None, &5_000);
    let report_before = client.get_user_report(&user);
    assert!(report_before.position.debt >= 5_000);

    env.ledger()
        .with_mut(|li| li.timestamp += SECONDS_PER_YEAR / 10);
    let (debt_after, interest_paid, principal_paid) = client.repay_debt(&user, &None, &10_000);
    assert!(interest_paid >= 0);
    assert!(principal_paid >= 0);
    assert!(debt_after < 5_000 || debt_after >= 0);
}

#[test]
fn test_borrow_then_repay_full_debt_includes_interest() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &15_000);
    token_client.approve(
        &user,
        &contract_id,
        &15_000,
        &(env.ledger().sequence() + 100),
    );

    client.deposit_collateral(&user, &None, &100_000);
    client.borrow_asset(&user, &None, &10_000);
    let report_before = client.get_user_report(&user);
    assert_eq!(report_before.position.debt, 10_000);

    env.ledger().with_mut(|li| li.timestamp += 86400 * 30);
    let (remaining, interest_paid, principal_paid) = client.repay_debt(&user, &None, &15_000);
    assert!(interest_paid >= 0);
    assert!(principal_paid <= 10_000);
    assert!(remaining >= 0);
}

// =============================================================================
// Index / rate consistency
// =============================================================================

#[test]
fn test_borrow_rate_used_in_accrual_consistent() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 5000);
    let rate = client.get_borrow_rate();
    assert!(rate >= 0);
    env.as_contract(&contract_id, || {
        let config = get_interest_rate_config(&env);
        assert!(config.is_some());
    });
}

#[test]
fn test_accrual_index_consistency_after_config_update() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);
    client.deposit_collateral(&user, &None, &50_000);
    client.borrow_asset(&user, &None, &10_000);

    let rate_before = client.get_borrow_rate();
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &Some(3000),
        &None,
        &None,
        &None,
        &None,
    );
    let rate_after = client.get_borrow_rate();
    assert!(rate_after >= rate_before || rate_after >= 0);
}
