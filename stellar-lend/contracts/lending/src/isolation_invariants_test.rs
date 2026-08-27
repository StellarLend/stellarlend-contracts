//! Sequence and boundary tests for isolation ceilings and reserve-style
//! capacity accounting (GrantFox issue #1899).
//!
//! The contract keeps one running `IsolationDebt(asset)` value for each
//! isolated collateral asset. These tests prove that the value follows the
//! actual stored principal through a long sequence of borrows and repays,
//! cannot be bypassed by administrative ceiling changes, and cannot be
//! corrupted by a failed release.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

fn asset(env: &Env) -> Address {
    Address::generate(env)
}

fn enable_isolation(client: &LendingContractClient, token: &Address, ceiling: i128) {
    client.set_asset_isolation(token, &true, &ceiling);
}

#[test]
fn ceiling_update_cannot_drop_below_outstanding_isolated_debt() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);
    client.borrow_against_collateral(&user, &700, &token);

    assert_eq!(
        client.try_set_asset_isolation(&token, &true, &699),
        Err(Ok(LendingError::IsolationCeilingBelowDebt))
    );
    assert_eq!(client.get_isolation_debt(&token), 700);
    assert_eq!(
        client
            .get_asset_isolation(&token)
            .unwrap()
            .isolation_debt_ceiling,
        1_000
    );
}

#[test]
fn ceiling_update_at_current_debt_is_valid_and_leaves_no_capacity() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);
    client.borrow_against_collateral(&user, &1_000, &token);

    client.set_asset_isolation(&token, &true, &1_000);
    assert_eq!(
        client.try_borrow_against_collateral(&user, &1, &token),
        Err(Ok(LendingError::IsolationCeilingExceeded))
    );
}

#[test]
fn disabling_isolation_preserves_debt_but_explicitly_removes_the_ceiling() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 100);
    client.borrow_against_collateral(&user, &100, &token);

    client.set_asset_isolation(&token, &false, &0);
    assert_eq!(client.get_isolation_debt(&token), 100);
    assert!(!client.get_asset_isolation(&token).unwrap().isolated);
    assert_eq!(client.borrow_against_collateral(&user, &500, &token), 600);
    // Disabled assets do not accumulate a tracker for newly borrowed debt.
    assert_eq!(client.get_isolation_debt(&token), 100);
}

#[test]
fn repayments_release_tracker_retained_when_isolation_is_disabled() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);
    client.borrow_against_collateral(&user, &600, &token);

    client.set_asset_isolation(&token, &false, &0);
    client.repay_against_collateral(&user, &250, &token);
    assert_eq!(client.get_isolation_debt(&token), 350);
}

#[test]
fn negative_ceiling_is_rejected_even_when_isolation_is_disabled() {
    let (env, client, _admin, _user) = setup();
    let token = asset(&env);

    assert_eq!(
        client.try_set_asset_isolation(&token, &false, &-1),
        Err(Ok(LendingError::InvalidIsolationCeiling))
    );
}

#[test]
fn zero_ceiling_is_only_valid_for_disabling_isolation() {
    let (env, client, _admin, _user) = setup();
    let token = asset(&env);

    assert_eq!(
        client.try_set_asset_isolation(&token, &true, &0),
        Err(Ok(LendingError::InvalidIsolationCeiling))
    );
    client.set_asset_isolation(&token, &false, &0);
    assert_eq!(
        client
            .get_asset_isolation(&token)
            .unwrap()
            .isolation_debt_ceiling,
        0
    );
}

#[test]
fn partial_repayments_release_exact_capacity_for_repeated_borrow_cycles() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 10_000);

    let operations = [
        ("borrow", 2_500i128),
        ("borrow", 1_750),
        ("repay", 1_000),
        ("borrow", 1_000),
        ("repay", 2_000),
        ("borrow", 3_000),
        ("repay", 750),
    ];
    let mut expected = 0i128;
    for (kind, amount) in operations {
        if kind == "borrow" {
            client.borrow_against_collateral(&user, &amount, &token);
            expected += amount;
        } else {
            client.repay_against_collateral(&user, &amount, &token);
            expected -= amount;
        }
        assert_eq!(
            client.get_isolation_debt(&token),
            expected,
            "tracker must match the position after {kind} {amount}"
        );
    }
}

#[test]
fn exact_capacity_can_be_reused_after_full_repayment() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 2_000);

    client.borrow_against_collateral(&user, &2_000, &token);
    assert_eq!(client.get_isolation_debt(&token), 2_000);
    client.repay_against_collateral(&user, &2_000, &token);
    assert_eq!(client.get_isolation_debt(&token), 0);
    client.borrow_against_collateral(&user, &2_000, &token);
    assert_eq!(client.get_isolation_debt(&token), 2_000);
}

#[test]
fn rejected_borrow_leaves_both_debt_and_isolation_tracker_unchanged() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 500);
    client.borrow_against_collateral(&user, &400, &token);
    let before_position = client.get_debt_position(&user);
    let before_tracker = client.get_isolation_debt(&token);

    assert_eq!(
        client.try_borrow_against_collateral(&user, &101, &token),
        Err(Ok(LendingError::IsolationCeilingExceeded))
    );
    assert_eq!(client.get_debt_position(&user), before_position);
    assert_eq!(client.get_isolation_debt(&token), before_tracker);
}

#[test]
fn isolated_assets_have_independent_capacity_buckets() {
    let (env, client, _admin, user) = setup();
    let first = asset(&env);
    let second = asset(&env);
    enable_isolation(&client, &first, 1_000);
    enable_isolation(&client, &second, 2_000);

    client.borrow_against_collateral(&user, &1_000, &first);
    client.borrow_against_collateral(&user, &2_000, &second);

    assert_eq!(client.get_isolation_debt(&first), 1_000);
    assert_eq!(client.get_isolation_debt(&second), 2_000);
    assert_eq!(
        client.try_borrow_against_collateral(&user, &1, &first),
        Err(Ok(LendingError::IsolationCeilingExceeded))
    );
    assert_eq!(
        client.try_borrow_against_collateral(&user, &1, &second),
        Err(Ok(LendingError::IsolationCeilingExceeded))
    );
}

#[test]
fn non_isolated_borrow_does_not_create_or_mutate_isolation_capacity() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);

    client.borrow_against_collateral(&user, &5_000, &token);

    assert_eq!(client.get_isolation_debt(&token), 0);
    assert!(client.get_asset_isolation(&token).is_none());
}

#[test]
fn public_ceiling_check_rejects_non_positive_borrows() {
    let (env, client, _admin, _user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);

    assert_eq!(
        client.try_check_isolation_ceiling(&token, &0),
        Err(Ok(LendingError::InvalidAmount))
    );
    assert_eq!(
        client.try_check_isolation_ceiling(&token, &-1),
        Err(Ok(LendingError::InvalidAmount))
    );
}

#[test]
fn isolation_release_rejects_tracker_underflow_without_saturating() {
    let (env, client, _admin, _user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);
    let contract_id = client.address.clone();

    // An empty bucket cannot release debt. The checked decrement must fail
    // instead of saturating to zero or creating a negative tracker.
    let result = env.as_contract(&contract_id, || decrement_isolation_debt(&env, &token, 1));
    assert_eq!(result, Err(LendingError::IsolationDebtInvariant));
    assert_eq!(client.get_isolation_debt(&token), 0);
}

#[test]
fn ledger_advance_does_not_change_capacity_without_a_state_transition() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);
    client.borrow_against_collateral(&user, &400, &token);
    let original_ledger = env.ledger().sequence();
    env.ledger().set_sequence_number(original_ledger + 10_000);

    assert_eq!(client.get_isolation_debt(&token), 400);
    assert_eq!(
        client.try_borrow_against_collateral(&user, &601, &token),
        Err(Ok(LendingError::IsolationCeilingExceeded))
    );
}

#[test]
fn ceiling_increase_restores_only_the_newly_added_capacity() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);
    client.borrow_against_collateral(&user, &700, &token);

    client.set_asset_isolation(&token, &true, &1_500);
    client.borrow_against_collateral(&user, &800, &token);
    assert_eq!(client.get_isolation_debt(&token), 1_500);
    assert_eq!(
        client.try_borrow_against_collateral(&user, &1, &token),
        Err(Ok(LendingError::IsolationCeilingExceeded))
    );
}

#[test]
fn failed_ceiling_update_does_not_disable_an_existing_isolated_asset() {
    let (env, client, _admin, user) = setup();
    let token = asset(&env);
    enable_isolation(&client, &token, 1_000);
    client.borrow_against_collateral(&user, &600, &token);

    assert_eq!(
        client.try_set_asset_isolation(&token, &true, &500),
        Err(Ok(LendingError::IsolationCeilingBelowDebt))
    );
    assert!(client.get_asset_isolation(&token).unwrap().isolated);
    assert_eq!(
        client
            .get_asset_isolation(&token)
            .unwrap()
            .isolation_debt_ceiling,
        1_000
    );
}
