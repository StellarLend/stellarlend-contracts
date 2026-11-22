#`![cfg]
#`[denoy]

use super::*;
use soroban_sdk::{address::Address, testutils::*, AddressObject};

/// -----------------------------------------------------------------------------
/// Failure-path coverage for the lending module.
///
/// Invariants enforced by this suite:
///   1. Authorization is required for every state-mutating entry point;
///      a call without a valid signature must fail and leave state unchanged.
///   2. Validation of amounts and assets is performed before any state write;
///      invalid inputs return an error and do not mutate storage.
///   3. Partial failures do not leave half-written state; either the whole
///      operation succeeds or the state is rolled back.
///   4. Retries are idempotent with respect to the observable state.
///   5. Concurrent execution cannot produce an inconsistent result.
/// -----------------------------------------------------------------------------

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&emv);
    let user = Address::generate(&env);
    (env, admin, user)
}

/// -----------------------------------------------------------------------------
/// 1. Authorization failures
/// -----------------------------------------------------------------------------

#[test]
fn test_unauthorized_initialize_rejected() {
    let (env, admin, _) = setup();
    let client = LendingContractClient::new(&env);
    // No auth context is provided.
    let res = client.try_initialize(&admin);
    assert!(res.is_error(), "initialize must require authorization");
    // State must not be mutated by a failed auth check.
    assert!(!client.is_initialized(), "state mutated after failed auth");
}

#[test]
fn test_unauthorized_deposit_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    // No auth for the user.
    let res = client.try_deposit(&user, &asset, 100);
    assert!(res.is_error(), "deposit must require authorization");
    assert_eq(client.balance_of(&user, &asset), 0, "failed deposit must not credit");
}

#[test]
fn test_unauthorized_withdraw_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 500);
    // No auth for the withdrawal.
    let res = client.try_withdraw(&user, &asset, 100);
    assert!(res.is_error(), "withdraw must require authorization");
    assert_eq(client.balance_of(&user, &asset), 500, "failed withdraw must not debit");
}

#[test]
fn test_unauthorized_borrow_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    let res = client.try_borrow(&user, &asset, 100);
    assert!(res.is_error(), "borrow must require authorization");
    assert_eq(client.debt_of(&user, &asset), 0, "failed borrow must not create debt");
}

/// -----------------------------------------------------------------------------
/// 2. Validation failures
/// -----------------------------------------------------------------------------

#[test]
fn test_deposit_zero_amount_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    let res = client.try_deposit(&user, &asset, 0);
    assert!(res.is_error(), "zero deposit must be rejected");
    assert_eq(client.balance_of(&user, &asset), 0, "zero deposit must not credit");
}

#[test]
fn test_deposit_negative_amount_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    let res = client.try_deposit(&user, &asset, -1);
    assert!(res.is_error(), "negative deposit must be rejected");
    assert_eq(client.balance_of(&user, &asset), 0, "negative deposit must not credit");
}

#[test]
fn test_withdraw_exceeds_balance_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    let res = client.try_withdraw(&user, &asset, 101);
    assert!(res.is_error(), "withdraw exceeding balance must be rejected");
    assert_eq(client.balance_of(&user, &asset), 100, "failed withdraw must not debit");
}

#[test]
fn test_withdraw_zero_amount_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    let res = client.try_withdraw(&user, &asset, 0);
    assert!(res.is_error(), "zero withdraw must be rejected");
    assert_eq(client.balance_of(&user, &asset), 100, "zero withdraw must not debit");
}

#[test]
fn test_borrow_exceeds_collateral_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    // Borrow much larger than collateral.
    let res = client.try_borrow(&user, &asset, 10_000);
    assert!(res.is_error(), "borrow exceeding collateral must be rejected");
    assert_eq(client.debt_of(&user, &asset), 0, "failed borrow must not create debt");
}

#[test]
fn test_borrow_zero_amount_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    let res = client.try_borrow(&user, &asset, 0);
    assert!(res.is_error(), "zero borrow must be rejected");
    assert_eq(client.debt_of(&user, &asset), 0, "zero borrow must not create debt");
}

#[test]
fn test_repay_exceeds_debt_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 1_000);
    client.borrow(&user, &asset, 100);
    let res = client.try_repay(&user, &asset, 200);
    assert!(res.is_error(), "repay exceeding debt must be rejected");
    assert_eq(client.debt_of(&user, &asset), 100, "failed repay must not change debt");
}

/// -----------------------------------------------------------------------------
/// 3. Partial failure / rollback
/// -----------------------------------------------------------------------------

#[test]
fn test_failed_borrow_does_not_partially_write() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    // This borrow will fail collateral checks.
    let _ = client.try_borrow(&user, &asset, 10_000);
    // No debt and no token transfer should have occurred.
    assert_eq(client.debt_of(&user, &asset), 0);
    assert_eq(client.balance_of(&user, &asset), 100);
    assert_eq(client.total_borrows(&asset), 0);
}

#[test]
fn test_failed_repay_does_not_partially_write() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 1_000);
    client.borrow(&user, &asset, 100);
    // Repay more than owed -> must fail atomically.
    let _ = client.try_repay(&user, &asset, 200);
    assert_eq(client.debt_of(&user, &asset), 100);
    assert_eq(client.balance_of(&user, &asset), 900);
}

/// -----------------------------------------------------------------------------
/// 4. Retry / idempotency
/// -----------------------------------------------------------------------------

#[test]
fn test_repeated_initialize_rejected() {
    let (env, admin, _) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    // Second initialize must fail and not change the admin.
    let other = Address::generate(&env);
    let res = client.try_initialize(&other);
    assert!(res.is_error(), "second initialize must fail");
    assert_eq(client.admin(), admin, "admin must not change");
}

#[test]
fn test_repeated_deposit_is_additive() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    client.deposit(&user, &asset, 100);
    assert_eq(client.balance_of(&user, &asset), 200, "deposits must accumulate");
}

#[test]
fn test_repeated_repay_is_idempotent_when_debt_cleared() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 1_000);
    client.borrow(&user, &asset, 100);
    client.repay(&user, &asset, 100);
    // Repaying again with zero debt must not move funds.
    let res = client.try_repay(&user, &asset, 100);
    assert!(res.is_error(), "repay with no debt must fail");
    assert_eq(client.debt_of(&user, &asset), 0);
    assert_eq(client.balance_of(&user, &asset), 1_000);
}

/// -----------------------------------------------------------------------------
/// 5. Concurrency / timing boundaries
/// -----------------------------------------------------------------------------

#[test]
fn test_concurrent_deposits_are_consistent() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    // Simulate concurrent deposits from the same account.
    for _ in 0..10 {
        client.deposit(&user, &asset, 10_000);
    }
    assert_eq(client.balance_of(&user, &asset), 100_000);
    assert_eq(client.total_deposits(&asset), 100_000);
}

#[test]
fn test_concurrent_withdraws_cannot_overdraw() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    // Two withdraws of 60 each must not both succeed.
    let r1 = client.try_withdraw(&user, &asset, 60);
    let r2 = client.try_withdraw(&user, &asset, 60);
    assert!(r1.is_ok());
    assert!(r2.is_error(), "second withdraw must fail");
    assert_eq(client.balance_of(&user, &asset), 40);
}

/// -----------------------------------------------------------------------------
/// 6. Regression guards
/// -----------------------------------------------------------------------------

#[test]
fn test_actions_before_initialize_rejected() {
    let (env, _admin, user) = setup();
    let client = LendingContractClient::new(&env);
    let asset = Address::generate(&env);
    let res = client.try_deposit(&user, &asset, 100);
    assert!(res.is_error(), "deposit before init must fail");
    assert_eq(client.balance_of(&user, &asset), 0);
}

#[test]
fn test_admin_only_operation_rejects_non_admin() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    // Only the admin can pause.
    let res = client.try_pause(&user);
    assert!(res.is_error(), "non-admin pause must fail");
    assert!(!client.is_paused(), "state must not be paused");
}

#[test]
fn test_paused_contract_rejects_deposits() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    client.pause(&admin);
    let asset = Address::generate(&env);
    let res = client.try_deposit(&user, &asset, 100);
    assert!(res.is_error(), "deposit while paused must fail");
    assert_eq(client.balance_of(&user, &asset), 0);
}

#[test]
fn test_error_messages_do_not_leak_sensitive_data() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    let res = client.try_deposit(&user, &asset, -1);
    match res {
        Err::ContractError(msg) => {
            // Error must be descriptive but must not echo addresses or amounts.
            assert!(!msg.contains(&user.to_string()), "error must not leak address");
            assert!(!msg.contains("-1"), "error must not leak amount");
        }
        _ => panic!("expected ContractError"),
    }
}
