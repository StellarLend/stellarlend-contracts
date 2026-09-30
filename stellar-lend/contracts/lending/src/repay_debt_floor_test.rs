//! Tests for the repay debt-floor invariant.
//!
//! `repay` must guarantee:
//!   1. Exact repayment → remaining debt is exactly 0.
//!   2. Overpayment    → `LendingError::RepayAmountTooHigh` is returned; debt unchanged.
//!   3. No prior debt  → calling repay returns `LendingError::InvalidAmount`
//!      (zero-principal settle falls through to InvalidAmount guard).
//!   4. `get_position` and `get_debt_position` never expose a negative debt value.
//!
//! See `docs/REPAY_SEMANTICS.md` for the canonical protocol semantics.

use crate::{LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{testutils::Address as _, Address, Env};

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
// 1. Exact repayment
// ---------------------------------------------------------------------------

/// Repaying the exact outstanding principal (no accrued interest here because
/// both borrow and repay happen in the same ledger) leaves zero remaining debt.
#[test]
fn repay_exact_principal_returns_zero_remaining_debt() {
    let (_env, client, user) = setup();
    client.deposit(&user, &500);
    client.borrow(&user, &200);

    let remaining = client.repay(&user, &200);

    assert_eq!(remaining, 0, "exact repay must return 0 remaining debt");
    assert_eq!(
        client.get_position(&user).debt,
        0,
        "get_position must report 0 after exact repay"
    );
    assert_eq!(
        client.get_debt_position(&user).principal,
        0,
        "get_debt_position must report 0 principal after exact repay"
    );
}

/// Partial repayment leaves the expected non-negative remainder.
#[test]
fn repay_partial_leaves_positive_remainder() {
    let (_env, client, user) = setup();
    client.deposit(&user, &500);
    client.borrow(&user, &300);

    let remaining = client.repay(&user, &100);

    assert_eq!(
        remaining, 200,
        "partial repay must return positive remainder"
    );
    assert!(
        client.get_position(&user).debt >= 0,
        "debt must not be negative after partial repay"
    );
}

// ---------------------------------------------------------------------------
// 2. Overpayment → RepayAmountTooHigh
// ---------------------------------------------------------------------------

/// Paying more than the outstanding principal returns `RepayAmountTooHigh`;
/// the position is left unchanged.
#[test]
fn repay_overpay_returns_repay_amount_too_high() {
    let (_env, client, user) = setup();
    client.deposit(&user, &200);
    client.borrow(&user, &100);

    // Repay 3× the outstanding principal — must error, not clamp.
    let result = client.try_repay(&user, &300);
    assert!(
        matches!(result, Err(Ok(LendingError::RepayAmountTooHigh))),
        "overpay must return RepayAmountTooHigh, got: {:?}",
        result
    );

    // Position must be unchanged.
    assert_eq!(
        client.get_position(&user).debt,
        100,
        "debt must remain 100 after a rejected overpay"
    );
    assert_eq!(
        client.get_debt_position(&user).principal,
        100,
        "raw principal must remain 100 after a rejected overpay"
    );
}

/// Paying i128::MAX when debt is small must also return RepayAmountTooHigh.
#[test]
fn repay_max_amount_when_small_debt_returns_error() {
    let (_env, client, user) = setup();
    client.deposit(&user, &10);
    client.borrow(&user, &1);

    let result = client.try_repay(&user, &i128::MAX);
    assert!(
        matches!(result, Err(Ok(LendingError::RepayAmountTooHigh))),
        "max overpay must return RepayAmountTooHigh, got: {:?}",
        result
    );
    // Debt must remain unchanged.
    assert_eq!(client.get_position(&user).debt, 1);
}

// ---------------------------------------------------------------------------
// 4. View functions never expose negative debt
// ---------------------------------------------------------------------------

/// get_position.debt is always non-negative regardless of accrual edge cases.
#[test]
fn get_position_debt_is_never_negative() {
    let (_env, client, user) = setup();

    // No debt at all
    assert!(
        client.get_position(&user).debt >= 0,
        "debt must be >= 0 with no borrow"
    );

    client.deposit(&user, &700);
    client.borrow(&user, &500);
    assert!(
        client.get_position(&user).debt >= 0,
        "debt must be >= 0 after borrow"
    );

    client.repay(&user, &500);
    assert!(
        client.get_position(&user).debt >= 0,
        "debt must be >= 0 after exact repay"
    );
}

/// get_debt_position.principal is always non-negative.
#[test]
fn get_debt_position_principal_is_never_negative() {
    let (_env, client, user) = setup();

    assert!(
        client.get_debt_position(&user).principal >= 0,
        "principal must be >= 0 with no borrow"
    );

    client.deposit(&user, &200);
    client.borrow(&user, &100);
    // Exact repay — should not go negative.
    client.repay(&user, &100);

    assert!(
        client.get_debt_position(&user).principal >= 0,
        "principal must be >= 0 after exact repay"
    );
}

/// The total-debt protocol counter must never go negative after a series of
/// borrows and exact repayments.
#[test]
fn total_debt_metric_never_negative_after_exact_repay() {
    let (_env, client, user) = setup();
    client.deposit(&user, &200);
    client.borrow(&user, &100);

    // Exact repay — safe.
    client.repay(&user, &100);

    let metrics = client.get_protocol_metrics();
    assert!(
        metrics.total_borrow >= 0,
        "total_borrow metric must not go negative after repay"
    );
}
