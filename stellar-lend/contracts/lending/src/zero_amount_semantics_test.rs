/// Adversarial tests for zero and negative amount rejection across all four
/// core lending entrypoints: deposit, withdraw, borrow, repay.
///
/// Each test verifies:
///   1. The call returns `Err(LendingError::InvalidAmount)` — not a panic.
///   2. Storage is not mutated (balances and debt are unchanged after rejection).
///
/// See docs/ZERO_AMOUNT_SEMANTICS.md for the design invariants these enforce.
use crate::{LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, user)
}

// ---------------------------------------------------------------------------
// deposit
// ---------------------------------------------------------------------------

#[test]
fn deposit_zero_returns_invalid_amount() {
    let (_env, client, user) = setup();
    let res = client.try_deposit(&user, &0);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn deposit_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    let res = client.try_deposit(&user, &-1);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn deposit_large_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    let res = client.try_deposit(&user, &i128::MIN);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn deposit_zero_does_not_mutate_collateral() {
    let (_env, client, user) = setup();
    // Establish a known starting balance
    client.deposit(&user, &500);
    let before = client.get_position(&user).collateral;

    let _ = client.try_deposit(&user, &0);

    let after = client.get_position(&user).collateral;
    assert_eq!(before, after, "collateral must not change on zero deposit");
}

#[test]
fn deposit_negative_does_not_mutate_collateral() {
    let (_env, client, user) = setup();
    client.deposit(&user, &500);
    let before = client.get_position(&user).collateral;

    let _ = client.try_deposit(&user, &-100);

    let after = client.get_position(&user).collateral;
    assert_eq!(before, after, "collateral must not change on negative deposit");
}

// ---------------------------------------------------------------------------
// withdraw
// ---------------------------------------------------------------------------

#[test]
fn withdraw_zero_returns_invalid_amount() {
    let (_env, client, user) = setup();
    client.deposit(&user, &200);
    let res = client.try_withdraw(&user, &0);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn withdraw_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    client.deposit(&user, &200);
    let res = client.try_withdraw(&user, &-50);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn withdraw_large_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    client.deposit(&user, &200);
    let res = client.try_withdraw(&user, &i128::MIN);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn withdraw_zero_does_not_mutate_collateral() {
    let (_env, client, user) = setup();
    client.deposit(&user, &200);
    let before = client.get_position(&user).collateral;

    let _ = client.try_withdraw(&user, &0);

    let after = client.get_position(&user).collateral;
    assert_eq!(before, after, "collateral must not change on zero withdraw");
}

#[test]
fn withdraw_negative_does_not_mutate_collateral() {
    let (_env, client, user) = setup();
    client.deposit(&user, &200);
    let before = client.get_position(&user).collateral;

    let _ = client.try_withdraw(&user, &-75);

    let after = client.get_position(&user).collateral;
    assert_eq!(before, after, "collateral must not change on negative withdraw");
}

// ---------------------------------------------------------------------------
// borrow
// ---------------------------------------------------------------------------

#[test]
fn borrow_zero_returns_invalid_amount() {
    let (_env, client, user) = setup();
    let res = client.try_borrow(&user, &0);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn borrow_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    let res = client.try_borrow(&user, &-1);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn borrow_large_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    let res = client.try_borrow(&user, &i128::MIN);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn borrow_zero_does_not_mutate_debt() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);
    let before = client.get_position(&user).debt;

    let _ = client.try_borrow(&user, &0);

    let after = client.get_position(&user).debt;
    assert_eq!(before, after, "debt must not change on zero borrow");
}

#[test]
fn borrow_negative_does_not_mutate_debt() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);
    let before = client.get_position(&user).debt;

    let _ = client.try_borrow(&user, &-50);

    let after = client.get_position(&user).debt;
    assert_eq!(before, after, "debt must not change on negative borrow");
}

/// A negative borrow must return InvalidAmount, not BelowMinimumBorrow,
/// even when a minimum borrow threshold is set.
#[test]
fn borrow_negative_with_min_borrow_set_returns_invalid_amount_not_below_minimum() {
    let (_env, client, user) = setup();
    // set a minimum borrow of 50 — negative amounts must still hit InvalidAmount
    client.set_min_borrow(&50);
    let res = client.try_borrow(&user, &-1);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

/// Zero borrow must return InvalidAmount even when min_borrow is also zero,
/// ensuring the zero guard is independent of the minimum-borrow check.
#[test]
fn borrow_zero_with_min_borrow_zero_returns_invalid_amount() {
    let (_env, client, user) = setup();
    // min_borrow defaults to 0; a zero amount must still be rejected
    let res = client.try_borrow(&user, &0);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// repay
// ---------------------------------------------------------------------------

#[test]
fn repay_zero_returns_invalid_amount() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);
    let res = client.try_repay(&user, &0);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn repay_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);
    let res = client.try_repay(&user, &-1);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn repay_large_negative_returns_invalid_amount() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);
    let res = client.try_repay(&user, &i128::MIN);
    assert_eq!(res, Ok(Err(LendingError::InvalidAmount)));
}

#[test]
fn repay_zero_does_not_mutate_debt() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);
    let before = client.get_position(&user).debt;

    let _ = client.try_repay(&user, &0);

    let after = client.get_position(&user).debt;
    assert_eq!(before, after, "debt must not change on zero repay");
}

#[test]
fn repay_negative_does_not_mutate_debt() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);
    let before = client.get_position(&user).debt;

    let _ = client.try_repay(&user, &-30);

    let after = client.get_position(&user).debt;
    assert_eq!(before, after, "debt must not change on negative repay");
}

#[test]
fn repay_exact_reduces_debt_to_zero() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);

    let remaining = client.repay(&user, &100).unwrap();

    assert_eq!(remaining, 0);
    assert_eq!(client.get_position(&user).debt, 0);
}

#[test]
fn repay_overpay_clamps_debt_to_zero() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);

    let remaining = client.repay(&user, &150).unwrap();

    assert_eq!(remaining, 0);
    assert_eq!(client.get_position(&user).debt, 0);
}

#[test]
fn repay_with_no_debt_returns_zero() {
    let (_env, client, user) = setup();

    let remaining = client.repay(&user, &50).unwrap();

    assert_eq!(remaining, 0);
    assert_eq!(client.get_position(&user).debt, 0);
}

// ---------------------------------------------------------------------------
// get_position is read-only and must never be affected by guard changes
// ---------------------------------------------------------------------------

#[test]
fn get_position_unaffected_after_all_rejected_calls() {
    let (_env, client, user) = setup();
    client.deposit(&user, &1000);
    client.borrow(&user, &200);

    // Attempt all four entrypoints with invalid amounts
    let _ = client.try_deposit(&user, &0);
    let _ = client.try_deposit(&user, &-1);
    let _ = client.try_withdraw(&user, &0);
    let _ = client.try_withdraw(&user, &-1);
    let _ = client.try_borrow(&user, &0);
    let _ = client.try_borrow(&user, &-1);
    let _ = client.try_repay(&user, &0);
    let _ = client.try_repay(&user, &-1);

    let pos = client.get_position(&user);
    assert_eq!(pos.collateral, 1000, "collateral corrupted by rejected calls");
    assert_eq!(pos.debt, 200, "debt corrupted by rejected calls");
}
