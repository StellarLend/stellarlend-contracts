use crate::debt::{split_reserve_share, DEFAULT_APR_BPS, MAX_RESERVE_FACTOR_BPS};
use crate::rounding_strategy::SECONDS_PER_YEAR;
use crate::{LendingContract, LendingContractClient, LendingError};
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
fn reserve_factor_defaults_to_zero() {
    let (_env, client, _admin, _user) = setup();
    assert_eq!(client.get_reserve_factor_bps(), 0);
    assert_eq!(client.get_total_reserve(), 0);
}

#[test]
fn reserve_factor_bounds_are_enforced() {
    let (_env, client, _admin, _user) = setup();

    client.set_reserve_factor_bps(&0);
    assert_eq!(client.get_reserve_factor_bps(), 0);

    client.set_reserve_factor_bps(&MAX_RESERVE_FACTOR_BPS);
    assert_eq!(client.get_reserve_factor_bps(), MAX_RESERVE_FACTOR_BPS);

    let too_high = MAX_RESERVE_FACTOR_BPS + 1;
    let result = client.try_set_reserve_factor_bps(&too_high);
    assert!(
        matches!(result, Err(Ok(LendingError::InvalidFeeBps))),
        "expected InvalidFeeBps, got {:?}",
        result
    );

    let result = client.try_set_reserve_factor_bps(&-1);
    assert!(
        matches!(result, Err(Ok(LendingError::InvalidFeeBps))),
        "expected InvalidFeeBps, got {:?}",
        result
    );
}

#[test]
fn repay_accrual_routes_reserve_share_and_compounds_remainder() {
    let (env, client, _admin, user) = setup();

    client.set_reserve_factor_bps(&1_000);
    client.borrow(&user, &100_000);
    advance_time(&env, SECONDS_PER_YEAR);

    let remaining = client.repay(&user, &1_000);

    assert_eq!(DEFAULT_APR_BPS, 500);
    assert_eq!(client.get_total_reserve(), 500);
    assert_eq!(remaining, 103_500);

    let position = client.get_debt_position(&user);
    assert_eq!(position.principal, 103_500);
}

#[test]
fn reserve_accumulates_across_multiple_settlements() {
    let (env, client, _admin, user) = setup();

    client.set_reserve_factor_bps(&1_000);
    client.borrow(&user, &100_000);

    advance_time(&env, SECONDS_PER_YEAR);
    client.repay(&user, &1_000);
    assert_eq!(client.get_total_reserve(), 500);

    advance_time(&env, SECONDS_PER_YEAR);
    let remaining = client.borrow(&user, &1_000);

    assert_eq!(client.get_total_reserve(), 1_018);
    assert_eq!(remaining, 109_157);
}

#[test]
fn max_factor_routes_half_of_interest_to_reserve() {
    let (env, client, _admin, user) = setup();

    client.set_reserve_factor_bps(&MAX_RESERVE_FACTOR_BPS);
    client.borrow(&user, &100_000);
    advance_time(&env, SECONDS_PER_YEAR);

    let remaining = client.repay(&user, &1_000);

    assert_eq!(client.get_total_reserve(), 2_500);
    assert_eq!(remaining, 101_500);
}

#[test]
fn tiny_interest_rounds_reserve_share_in_protocol_favor() {
    assert_eq!(split_reserve_share(1, 1_000).unwrap(), 1);
    assert_eq!(split_reserve_share(9, 1_000).unwrap(), 1);
    assert_eq!(split_reserve_share(10, 1_000).unwrap(), 1);
    assert_eq!(split_reserve_share(0, 1_000).unwrap(), 0);
}
