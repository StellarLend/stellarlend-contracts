use crate::rounding_strategy::SECONDS_PER_YEAR;
use crate::{LendingContract, LendingContractClient, LendingError, HEALTH_FACTOR_SCALE};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

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

fn advance_time(env: &Env, seconds: u64) {
    let mut ledger = env.ledger().get();
    ledger.timestamp = ledger.timestamp.saturating_add(seconds);
    ledger.sequence_number = ledger.sequence_number.saturating_add(1);
    env.ledger().set(ledger);
}

#[test]
fn borrow_with_zero_collateral_is_rejected() {
    let (_env, client, _admin, user) = setup();

    let result = client.try_borrow(&user, &1);

    assert!(
        matches!(result, Err(Ok(LendingError::InsufficientCollateral))),
        "expected InsufficientCollateral, got {:?}",
        result
    );
    assert_eq!(client.get_debt_position(&user).principal, 0);
    assert_eq!(client.get_protocol_metrics().total_borrow, 0);
}

#[test]
fn borrow_exactly_at_health_factor_threshold_is_allowed() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &125);

    let debt = client.borrow(&user, &100);

    assert_eq!(debt, 100);
    assert_eq!(client.get_health_factor(&user), HEALTH_FACTOR_SCALE);
}

#[test]
fn borrow_below_health_factor_threshold_is_rejected_without_mutation() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &124);

    let result = client.try_borrow(&user, &100);

    assert!(
        matches!(result, Err(Ok(LendingError::InsufficientCollateral))),
        "expected InsufficientCollateral, got {:?}",
        result
    );
    assert_eq!(client.get_debt_position(&user).principal, 0);
    assert_eq!(client.get_protocol_metrics().total_borrow, 0);
}

#[test]
fn borrow_that_crosses_debt_ceiling_by_one_is_rejected() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &1_000);
    client.set_debt_ceiling(&99);

    let result = client.try_borrow(&user, &100);

    assert!(
        matches!(result, Err(Ok(LendingError::DebtCeilingExceeded))),
        "expected DebtCeilingExceeded, got {:?}",
        result
    );
    assert_eq!(client.get_debt_position(&user).principal, 0);
    assert_eq!(client.get_protocol_metrics().total_borrow, 0);
}

#[test]
fn second_borrow_uses_accrued_debt_for_health_factor() {
    let (env, client, _admin, user) = setup();
    client.deposit(&user, &137_500);
    client.borrow(&user, &100_000);

    advance_time(&env, SECONDS_PER_YEAR);

    let result = client.try_borrow(&user, &5_001);
    assert!(
        matches!(result, Err(Ok(LendingError::InsufficientCollateral))),
        "expected InsufficientCollateral, got {:?}",
        result
    );

    let debt = client.borrow(&user, &5_000);
    assert_eq!(debt, 110_000);
    assert_eq!(client.get_protocol_metrics().total_borrow, 110_000);
}
