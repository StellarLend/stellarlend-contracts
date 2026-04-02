#![cfg(test)]

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_test() -> (Env, HelloContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn test_config_snapshot_returns_some_after_init() {
    let (_env, client, _admin) = setup_test();
    let snapshot = client.get_config_snapshot();
    assert!(snapshot.is_some());
}

#[test]
fn test_config_snapshot_default_values_valid() {
    let (_env, client, _admin) = setup_test();
    let snapshot = client.get_config_snapshot().unwrap();
    assert!(snapshot.min_collateral_ratio > 0);
    assert!(snapshot.liquidation_threshold > 0);
    assert!(snapshot.close_factor > 0);
    assert!(snapshot.liquidation_incentive > 0);
}

#[test]
fn test_config_snapshot_min_collateral_ratio_gte_liquidation_threshold() {
    let (_env, client, _admin) = setup_test();
    let snapshot = client.get_config_snapshot().unwrap();
    assert!(snapshot.min_collateral_ratio >= snapshot.liquidation_threshold);
}

#[test]
fn test_config_snapshot_emergency_paused_false_by_default() {
    let (_env, client, _admin) = setup_test();
    let snapshot = client.get_config_snapshot().unwrap();
    assert!(!snapshot.emergency_paused);
}

#[test]
fn test_config_snapshot_emergency_paused_true_when_paused() {
    let (_env, client, admin) = setup_test();
    client.set_emergency_pause(&admin, &true);
    let snapshot = client.get_config_snapshot().unwrap();
    assert!(snapshot.emergency_paused);
}

#[test]
fn test_config_snapshot_emergency_paused_false_after_unpause() {
    let (_env, client, admin) = setup_test();
    client.set_emergency_pause(&admin, &true);
    client.set_emergency_pause(&admin, &false);
    let snapshot = client.get_config_snapshot().unwrap();
    assert!(!snapshot.emergency_paused);
}

#[test]
fn test_config_snapshot_borrow_rate_non_negative() {
    let (_env, client, _admin) = setup_test();
    let snapshot = client.get_config_snapshot().unwrap();
    assert!(snapshot.base_borrow_rate >= 0);
}

#[test]
fn test_config_snapshot_snapshot_time_set() {
    let (env, client, _admin) = setup_test();
    let snapshot = client.get_config_snapshot().unwrap();
    assert_eq!(snapshot.snapshot_time, env.ledger().timestamp());
}

#[test]
fn test_config_snapshot_none_before_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let snapshot = client.get_config_snapshot();
    assert!(snapshot.is_none());
}

#[test]
fn test_config_snapshot_stable_across_calls() {
    let (_env, client, _admin) = setup_test();
    let s1 = client.get_config_snapshot().unwrap();
    let s2 = client.get_config_snapshot().unwrap();
    assert_eq!(s1.min_collateral_ratio, s2.min_collateral_ratio);
    assert_eq!(s1.liquidation_threshold, s2.liquidation_threshold);
    assert_eq!(s1.close_factor, s2.close_factor);
    assert_eq!(s1.liquidation_incentive, s2.liquidation_incentive);
    assert_eq!(s1.emergency_paused, s2.emergency_paused);
}

#[test]
fn test_config_snapshot_no_auth_required() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let snapshot = client.get_config_snapshot();
    assert!(snapshot.is_some());
}

#[test]
fn test_config_snapshot_read_only_guarantees_and_isolation() {
    let (_env, client, admin) = setup_test();

    // 1. Snapshot initial state
    let snapshot_before = client.get_config_snapshot().unwrap();

    // 2. Perform a state-mutating operation via an authorized endpoint
    client.set_emergency_pause(&admin, &true);

    // 3. Snapshot state again
    let snapshot_after = client.get_config_snapshot().unwrap();

    // 4. Prove that exactly the paused state changed, while other fields remain strictly consistent
    assert_eq!(snapshot_before.emergency_paused, false);
    assert_eq!(snapshot_after.emergency_paused, true);
    assert_eq!(
        snapshot_before.min_collateral_ratio,
        snapshot_after.min_collateral_ratio
    );
    assert_eq!(
        snapshot_before.liquidation_threshold,
        snapshot_after.liquidation_threshold
    );
    assert_eq!(snapshot_before.close_factor, snapshot_after.close_factor);
    assert_eq!(
        snapshot_before.liquidation_incentive,
        snapshot_after.liquidation_incentive
    );

    // 5. Unpause and verify state reversion reflects correctly
    client.set_emergency_pause(&admin, &false);
    let snapshot_final = client.get_config_snapshot().unwrap();
    assert_eq!(snapshot_final.emergency_paused, false);
}
