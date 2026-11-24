#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// -----------------------------------------------------------------------------
/// Boundary-case coverage for the lending module.
///
/// Invariants enforced by this suite:
///   1. Amount boundaries (0, 1, i128::MAX, overflow) are handled explicitly.
///   2. Balance boundaries (exact balance, balance + 1) are enforced.
///   3. Collateral / LTV boundaries (exactly at threshold, one unit past) are
///      enforced.
/// -----------------------------------------------------------------------------

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

fn set_deposit_cap(env: &Env, contract_id: &Address, cap: i128) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });
}

/// -----------------------------------------------------------------------------
/// 1. Amount boundaries
/// -----------------------------------------------------------------------------

#[test]
fn test_zero_deposit_rejected() {
    let (_env, client, _admin, user) = setup();
    let res = client.try_deposit(&user, &0);
    assert!(res.is_err(), "zero deposit must be rejected");
    assert_eq!(client.get_position(&user).collateral, 0);
}

#[test]
fn test_minimum_deposit_of_one_succeeds() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &1);
    assert_eq!(client.get_position(&user).collateral, 1);
}

#[test]
fn test_maximum_deposit_succeeds() {
    let (env, client, _admin, user) = setup();
    set_deposit_cap(&env, &client.address, i128::MAX);
    client.deposit(&user, &i128::MAX);
    assert_eq!(client.get_position(&user).collateral, i128::MAX);
}

#[test]
fn test_deposit_overflow_rejected() {
    let (env, client, _admin, user) = setup();
    set_deposit_cap(&env, &client.address, i128::MAX);
    client.deposit(&user, &i128::MAX);
    // Any additional deposit must not silently wrap.
    let res = client.try_deposit(&user, &1);
    assert!(res.is_err(), "overflowing deposit must be rejected");
    assert_eq!(client.get_position(&user).collateral, i128::MAX);
}

#[test]
fn test_borrow_overflow_rejected() {
    let (env, client, _admin, user) = setup();
    set_deposit_cap(&env, &client.address, i128::MAX);
    client.deposit(&user, &i128::MAX);
    // Borrowing i128::MAX exceeds LTV / overflows arithmetic.
    let res = client.try_borrow(&user, &i128::MAX);
    assert!(res.is_err(), "overflowing borrow must be rejected");
}

/// -----------------------------------------------------------------------------
/// 2. Balance boundaries
/// -----------------------------------------------------------------------------

#[test]
fn test_withdraw_exactly_balance_succeeds() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &100);
    client.withdraw(&user, &100);
    assert_eq!(client.get_position(&user).collateral, 0);
}

#[test]
fn test_withdraw_balance_plus_one_rejected() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &100);
    let res = client.try_withdraw(&user, &101);
    assert!(res.is_err());
    assert_eq!(client.get_position(&user).collateral, 100);
}

#[test]
fn test_repay_exactly_debt_clears_debt() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &1_000);
    client.borrow(&user, &100);
    client.repay(&user, &100);
    assert_eq!(client.get_position(&user).debt, 0);
}

#[test]
fn test_repay_debt_plus_one_rejected() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &1_000);
    client.borrow(&user, &100);
    let res = client.try_repay(&user, &101);
    assert!(res.is_err());
    assert_eq!(client.get_position(&user).debt, 100);
}

/// -----------------------------------------------------------------------------
/// 3. Collateral / LTV boundaries
/// -----------------------------------------------------------------------------

/// The contract enforces `DEFAULT_MAX_LTV_BPS` (7500 bps = 75%).
/// These tests pin the exact boundary behavior.

#[test]
fn test_borrow_at_exact_collateral_threshold_succeeds() {
    let (_env, client, _admin, user) = setup();
    // Collateral = 200, max borrow at 75% LTV = 150.
    client.deposit(&user, &200);
    client.borrow(&user, &150);
    assert_eq!(client.get_position(&user).debt, 150);
}

#[test]
fn test_borrow_one_above_collateral_threshold_rejected() {
    let (_env, client, _admin, user) = setup();
    // Collateral = 200, debt = 151 exceeds 75% LTV.
    client.deposit(&user, &200);
    let res = client.try_borrow(&user, &151);
    assert!(res.is_err(), "borrow above LTV threshold must fail");
    assert_eq!(client.get_position(&user).debt, 0);
}
