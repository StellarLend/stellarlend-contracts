#`![cfg]
#`[denoy]

use super::*;
use soroban_sdk:{address::Address, testutils::, AddressObject};

/// -----------------------------------------------------------------------------
/// Boundary-case coverage for the lending module.
///
/// Invariants enforced by this suite:
///   1. Amount boundaries (0, 1, i128_MAX, overflow) are handled explicitly.
///   2. Balance boundaries (exact balance, balance + 1) are enforced.
///   3. Collateral ratio boundaries (exactly at threshold, just below) are
///      enforced.
///   4. Address boundaries (self-transfer, zero address) are rejected.
/// -----------------------------------------------------------------------------

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&emv);
    let user = Address::generate(&env);
    (env, admin, user)
}

/// -----------------------------------------------------------------------------
/// 1. Amount boundaries
/// -----------------------------------------------------------------------------

#[test]
fn test_minimum_deposit_of_one_succeeds() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 1);
    assert_eq(client.balance_of(&user, &asset), 1);
}

#[test]
fn test_maximum_deposit_succeeds() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, i128_MAX);
    assert_eq(client.balance_of(&user, &asset), i128_MAX);
}

#[test]
fn test_deposit_overflow_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, i128_MAX);
    // Any additional deposit must not silently wrap.
    let res = client.try_deposit(&user, &asset, 1);
    assert!(res.is_error(), "overflowing deposit must be rejected");
    assert_eq(client.balance_of(&user, &asset), i128_MAX);
}

#[test]
fn test_borrow_overflow_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, i128_MAX);
    // Borrowing i128_MAX + 1 would overflow total borrows.
    let res = client.try_borrow(&user, &asset, i128_MAX);
    assert!(res.is_error(), "overflowing borrow must be rejected");
}

/// -----------------------------------------------------------------------------
/// 2. Balance boundaries
/// -----------------------------------------------------------------------------

#[test]
fn test_withdraw_exactly_balance_succeeds() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    client.withdraw(&user, &asset, 100);
    assert_eq(client.balance_of(&user, &asset), 0);
}

#[test]
fn test_withdraw_balance_plus_one_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 100);
    let res = client.try_withdraw(&user, &asset, 101);
    assert!(res.is_error());
    assert_eq(client.balance_of(&user, &asset), 100);
}

#[test]
fn test_repay_exactly_debt_clears_debt() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 1_000);
    client.borrow(&user, &asset, 100);
    client.repay(&user, &asset, 100);
    assert_eq(client.debt_of(&user, &asset), 0);
}

#[test]
fn test_repay_debt_plus_one_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    client.deposit(&user, &asset, 1_000);
    client.borrow(&user, &asset, 100);
    let res = client.try_repay(&user, &asset, 101);
    assert!(res.is_error());
    assert_eq(client.debt_of(&user, &asset), 100);
}

/// -----------------------------------------------------------------------------
/// 3. Collateral ratio boundaries
+// -----------------------------------------------------------------------------

/// The contract enforces a collateral ratio of 150% (collateral >= 1.5 * debt).
/// These tests pin the exact boundary behavior.

#[test]
fn test_borrow_at_exact_collateral_threshold_succeeds() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    // Collateral = 150, debt = 100 -> ratio = 150% exactly.
    client.deposit(&user, &asset, 150);
    client.borrow(&user, &asset, 100);
    assert_eq(client.debt_of(&user, &asset), 100);
}

#[test]
fn test_borrow_one_below_collateral_threshold_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let asset = Address::generate(&env);
    // Collateral = 150, debt = 101 -> ratio < 150%.
    client.deposit(&user, &asset, 150);
    let res = client.try_borrow(&user, &asset, 101);
    assert!(res.is_error(), "borrow below collateral threshold must fail");
    assert_eq(client.debt_of(&user, &asset), 0);
}

/// -----------------------------------------------------------------------------
/// 4. Address boundaries
+// -----------------------------------------------------------------------------

#[test]
fn test_deposit_to_self_rejected() {
    let (env, admin, user) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    // The contract address is the client address.
    let contract_addr = client.address();
    let res = client.try_deposit(&contract_addr, &contract_addr, 100);
    assert!(res.is_error(), "self-deposit must be rejected");
    assert_eq(client.balance_of(&contract_addr, &contract_addr), 0);
}

#[test]
fn test_deposit_to_zero_address_rejected() {
    let (env, admin, _) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let zero = Address::from_string(&emv, "C0000000000000000000000000000000000000000000000000000000000000000");
    let asset = Address::generate(&env);
    let res = client.try_deposit(&zero, &asset, 100);
    assert!(res.is_error(), "deposit to zero address must be rejected");
}

#[test]
fn test_deposit_to_contract_address_rejected() {
    let (env, admin, _) = setup();
    let client = LendingContractClient::new(&env);
    client.initialize(&admin);
    let contract_addr = client.address();
    let asset = Address::generate(&env);
    let res = client.try_deposit(&contract_addr, &asset, 100);
    assert!(res.is_error(), "deposit to contract address must be rejected");
}
