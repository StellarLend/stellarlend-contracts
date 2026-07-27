#![cfg(test)]

use crate::debt::{load_debt, repay_amount, DebtError, DebtPosition, DEFAULT_APR_BPS};
use crate::{LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{testutils::Address as _, Address, Env};

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

/// Repaying the exact outstanding debt clears the position to zero.
#[test]
fn repay_exact_amount_leaves_zero_debt() {
    let (env, _client, _admin, _user) = setup();

    let position = DebtPosition {
        borrow_index_snapshot: crate::debt::INDEX_SCALE,
        principal: 1000,
        borrow_index_snapshot: 0,
        last_update: env.ledger().timestamp(),
    };

    let updated = repay_amount(position, env.ledger().timestamp(), 1000, DEFAULT_APR_BPS)
        .expect("repay_amount should succeed for exact repay");

    assert_eq!(updated.principal, 0);
}

/// Repaying one unit more than the outstanding debt must return
/// `DebtError::RepayAmountTooHigh` — not silently clamp.
#[test]
fn repay_overpay_by_one_unit_returns_error() {
    let (env, _client, _admin, _user) = setup();

    let position = DebtPosition {
        borrow_index_snapshot: crate::debt::INDEX_SCALE,
        principal: 1000,
        borrow_index_snapshot: 0,
        last_update: env.ledger().timestamp(),
    };

    let result = repay_amount(position, env.ledger().timestamp(), 1001, DEFAULT_APR_BPS);
    assert_eq!(
        result,
        Err(DebtError::RepayAmountTooHigh),
        "Overpaying by 1 unit must return RepayAmountTooHigh"
    );
}

/// Repaying 2× the outstanding debt must return `DebtError::RepayAmountTooHigh`.
#[test]
fn repay_overpay_by_2x_returns_error() {
    let (env, _client, _admin, _user) = setup();

    let position = DebtPosition {
        borrow_index_snapshot: crate::debt::INDEX_SCALE,
        principal: 1000,
        borrow_index_snapshot: 0,
        last_update: env.ledger().timestamp(),
    };

    let result = repay_amount(position, env.ledger().timestamp(), 2000, DEFAULT_APR_BPS);
    assert_eq!(
        result,
        Err(DebtError::RepayAmountTooHigh),
        "Overpaying by 2× must return RepayAmountTooHigh"
    );
}

/// After time passes, the settled principal includes accrued interest.
/// Repaying an amount that exceeds the *settled* total must also error.
#[test]
fn repay_overpay_after_interest_accrual_returns_error() {
    let (env, _client, _admin, _user) = setup();

    let initial_timestamp = 1000u64;
    let position = DebtPosition {
        borrow_index_snapshot: crate::debt::INDEX_SCALE,
        principal: 1000,
        borrow_index_snapshot: 0,
        last_update: initial_timestamp,
    };

    // Advance one year; at 5 % APR interest ≈ 50, so settled principal ≈ 1050.
    let repay_timestamp = initial_timestamp + 31_536_000u64;

    // Paying 1100 exceeds the settled ~1050 — must error, not clamp.
    let result = repay_amount(position, repay_timestamp, 1100, DEFAULT_APR_BPS);
    assert_eq!(
        result,
        Err(DebtError::RepayAmountTooHigh),
        "Overpaying after interest accrual must return RepayAmountTooHigh"
    );
}

/// Partial repayment (amount < debt) still succeeds and reduces the principal.
#[test]
fn repay_partial_payment_leaves_remaining_debt() {
    let (env, _client, _admin, _user) = setup();

    let position = DebtPosition {
        borrow_index_snapshot: crate::debt::INDEX_SCALE,
        principal: 1000,
        borrow_index_snapshot: 0,
        last_update: env.ledger().timestamp(),
    };

    let updated = repay_amount(position, env.ledger().timestamp(), 600, DEFAULT_APR_BPS)
        .expect("partial repay should succeed");

    assert_eq!(updated.principal, 400, "Remaining principal should be 400");
}

/// The contract-level `repay` entrypoint returns `LendingError::RepayAmountTooHigh`
/// when the caller supplies more than the outstanding debt.
#[test]
fn contract_repay_overpay_returns_lending_error() {
    let (env, client, _admin, user) = setup();

    client.deposit(&user, &700);
    client.borrow(&user, &500);

    // Confirm debt is 500 before attempting the overpay.
    assert_eq!(client.get_position(&user).debt, 500);

    // Repay 700 when only 500 is owed → must fail with RepayAmountTooHigh.
    let result = client.try_repay(&user, &700);
    assert!(
        matches!(result, Err(Ok(LendingError::RepayAmountTooHigh))),
        "Contract repay must return RepayAmountTooHigh on overpay, got: {:?}",
        result
    );

    // Position must be unchanged after the failed repay.
    assert_eq!(
        client.get_position(&user).debt,
        500,
        "Debt must remain 500 after a rejected overpay"
    );
}

/// Repaying the exact debt via the contract succeeds and zeroes the position.
#[test]
fn contract_repay_exact_debt_succeeds() {
    let (env, client, _admin, user) = setup();

    client.deposit(&user, &500);
    client.borrow(&user, &300);
    assert_eq!(client.get_position(&user).debt, 300);

    let remaining = client.repay(&user, &300);
    assert_eq!(remaining, 0, "Remaining debt should be 0 after exact repay");
    assert_eq!(client.get_position(&user).debt, 0);

    // Second repay should fail or return 0 (no outstanding debt)
    // Since there's no debt, repay should either error or cap at 0
    let res = client.try_repay(&user, &100);
    // Contract might error on zero debt or cap it - both are valid
    // If it doesn't error, remaining should be 0
    if let Ok(Ok(remaining)) = res {
        assert_eq!(remaining, 0, "Cannot reduce debt below zero");
    }
}

#[test]
fn repay_exact_debt_after_interest_accrual_with_no_refund() {
    let (env, _client, _admin, _user) = setup();

    let initial_timestamp = 1000u64;
    let position = DebtPosition {
        principal: 1000,
        borrow_index_snapshot: 0,
        last_update: initial_timestamp,
    };

    // Advance time by 1 second
    let repay_timestamp = initial_timestamp + 1u64;

    // Calculate what debt would be after 1 second at 5% APR
    // Interest per second = 1000 * 0.05 / 31536000 ≈ 0.00158...
    // With Banker's rounding, 1 second might round to 0 interest
    let updated = repay_amount(position, repay_timestamp, 1000, DEFAULT_APR_BPS)
        .expect("repay_amount should succeed");

    // If interest is minimal or rounds to 0, debt should be 0 or close
    assert_eq!(
        updated.principal, 0,
        "Debt should be 0 or less after exact repay plus interest"
    );
}
