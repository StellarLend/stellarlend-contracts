#![cfg(test)]

use crate::{
    DataKey, LendingContract, LendingContractClient, LendingError, HEALTH_FACTOR_SCALE,
    LIQUIDATION_THRESHOLD_BPS,
};
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
use soroban_sdk::{Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, id, user)
}

/// Minimum collateral so effective debt `debt` keeps HF >= 1.0.
fn min_collateral_for_debt(debt: i128) -> i128 {
    (debt.saturating_mul(HEALTH_FACTOR_SCALE) + LIQUIDATION_THRESHOLD_BPS - 1)
        / LIQUIDATION_THRESHOLD_BPS
}

fn advance_time(env: &Env, seconds: u64) {
    let mut info: LedgerInfo = env.ledger().get();
    info.timestamp = info.timestamp.saturating_add(seconds);
    info.sequence_number = info.sequence_number.saturating_add(1);
    env.ledger().set(info);
}

#[test]
fn borrow_zero_collateral_rejected() {
    let (_env, client, _admin, user) = setup();
    let res = client.try_borrow(&user, &100);
    assert!(matches!(res, Err(Ok(LendingError::InsufficientCollateral))));
    assert_eq!(client.get_position(&user).debt, 0);
}

#[test]
fn borrow_at_exact_health_factor_threshold_succeeds() {
    let (_env, client, _admin, user) = setup();
    let debt = 80i128;
    client.deposit(&user, &100);
    let res = client.borrow(&user, &debt);
    assert_eq!(res, debt);
    assert_eq!(client.get_health_factor(&user), HEALTH_FACTOR_SCALE);
}

#[test]
fn borrow_one_unit_past_health_factor_threshold_rejected() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &100);
    let res = client.try_borrow(&user, &81);
    assert!(matches!(res, Err(Ok(LendingError::InsufficientCollateral))));
    assert_eq!(client.get_position(&user).debt, 0);
}

#[test]
fn borrow_rejects_when_weighted_collateral_multiplication_overflows() {
    let (env, client, id, user) = setup();
    let overflowing_collateral = i128::MAX / LIQUIDATION_THRESHOLD_BPS + 1;

    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &overflowing_collateral);
    });

    let res = client.try_borrow(&user, &1);
    assert!(matches!(res, Err(Ok(LendingError::Overflow))));
    assert_eq!(client.get_position(&user).debt, 0);
}

#[test]
fn borrow_rejects_when_post_borrow_total_debt_exceeds_ceiling_by_one() {
    let (_env, client, _id, user) = setup();
    client.set_debt_ceiling(&1_000);
    client.deposit(&user, &min_collateral_for_debt(1_001));

    let first = client.borrow(&user, &1_000);
    assert_eq!(first, 1_000);

    let res = client.try_borrow(&user, &1);
    assert!(matches!(res, Err(Ok(LendingError::DebtCeilingExceeded))));
    assert_eq!(client.get_position(&user).debt, 1_000);
}

#[test]
fn borrow_at_exact_debt_ceiling_succeeds() {
    let (_env, client, _admin, user) = setup();
    client.set_debt_ceiling(&500);
    client.deposit(&user, &min_collateral_for_debt(500));

    let res = client.borrow(&user, &500);
    assert_eq!(res, 500);
}

#[test]
fn second_borrow_with_accrued_interest_requires_extra_collateral() {
    let (env, client, _admin, user) = setup();
    client.deposit(&user, &125);
    client.borrow(&user, &100);

    advance_time(&env, SECONDS_PER_YEAR);
    let position = client.get_position(&user);
    assert!(position.debt > 100, "interest should have accrued");

    let res = client.try_borrow(&user, &1);
    assert!(matches!(res, Err(Ok(LendingError::InsufficientCollateral))));

    client.deposit(&user, &25);
    let res = client.try_borrow(&user, &1);
    assert!(matches!(res, Ok(Ok(_))));
}

#[test]
fn healthy_borrow_succeeds_with_sufficient_collateral() {
    let (_env, client, _admin, user) = setup();
    client.deposit(&user, &200);
    let res = client.borrow(&user, &100);
    assert_eq!(res, 100);
    assert!(client.get_health_factor(&user) > HEALTH_FACTOR_SCALE);
}

#[test]
fn rejected_borrow_does_not_mutate_debt_or_total_debt() {
    let (_env, client, _admin, user) = setup();
    client.set_debt_ceiling(&10_000);
    client.deposit(&user, &100);

    let metrics_before = client.get_protocol_metrics();
    let _ = client.try_borrow(&user, &200);

    assert_eq!(client.get_position(&user).debt, 0);
    assert_eq!(
        client.get_protocol_metrics().total_borrow,
        metrics_before.total_borrow
    );
}

const SECONDS_PER_YEAR: u64 = 31_536_000;
