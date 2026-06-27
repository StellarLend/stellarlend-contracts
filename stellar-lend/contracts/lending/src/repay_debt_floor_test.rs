//! Tests for the repay debt-floor invariant.
//!
//! `repay` must guarantee:
//!   1. Exact repayment → remaining debt is exactly 0.
//!   2. Overpayment    → remaining debt is clamped to 0, never negative.
//!   3. No prior debt  → calling repay returns 0 and does not create negative debt.
//!   4. `get_position` and `get_debt_position` never expose a negative debt value.
//!
//! See `docs/ZERO_AMOUNT_SEMANTICS.md` for the canonical protocol semantics.

use crate::{LendingContract, LendingContractClient};
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
// 2. Overpayment clamped to zero
// ---------------------------------------------------------------------------

/// Paying more than the outstanding principal clamps debt to zero; the return
/// value must be 0, never negative.
#[test]
fn repay_overpay_clamps_to_zero_not_negative() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);

    // Repay 3× the outstanding principal
    let remaining = client.repay(&user, &300);

    assert_eq!(remaining, 0, "overpay must return 0, not a negative value");
    assert_eq!(
        client.get_position(&user).debt,
        0,
        "get_position must report 0 after overpay"
    );
    assert_eq!(
        client.get_debt_position(&user).principal,
        0,
        "raw debt principal must be 0 after overpay clamp"
    );
}

/// Paying i128::MAX when debt is small must also clamp cleanly to zero.
#[test]
fn repay_max_amount_when_small_debt_clamps_to_zero() {
    let (_env, client, user) = setup();
    client.borrow(&user, &1);

    let remaining = client.repay(&user, &i128::MAX);

    assert_eq!(remaining, 0, "max overpay must return 0 remaining debt");
    assert_eq!(client.get_position(&user).debt, 0);
}

// ---------------------------------------------------------------------------
// 3. Repay with no prior debt
// ---------------------------------------------------------------------------

/// A user who has never borrowed still gets 0 back from repay — no credit
/// balance (negative debt) must ever be created.
#[test]
fn repay_when_no_debt_exists_returns_zero() {
    let (_env, client, user) = setup();

    let remaining = client.repay(&user, &50);

    assert_eq!(remaining, 0, "repay with no debt must return 0");
    assert_eq!(
        client.get_position(&user).debt,
        0,
        "get_position must report 0 for a user who never borrowed"
    );
    assert_eq!(
        client.get_debt_position(&user).principal,
        0,
        "principal must remain 0 when no debt was ever created"
    );
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

    client.borrow(&user, &100);
    client.repay(&user, &999); // overpay

    assert!(
        client.get_debt_position(&user).principal >= 0,
        "principal must be >= 0 after overpay"
    );
}

/// The total-debt protocol counter must never go negative after a series of
/// borrows and overpayments.
#[test]
fn total_debt_metric_never_negative_after_overpay() {
    let (_env, client, user) = setup();
    client.borrow(&user, &100);

    // Overpay
    client.repay(&user, &9999);

    let metrics = client.get_protocol_metrics();
    assert!(
        metrics.total_borrow >= 0,
        "total_borrow metric must not go negative after overpay"
    );
}
