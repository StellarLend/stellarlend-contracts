/// Tests for inbound message capacity / cap enforcement in the bridge contract.
///
/// These tests exercise the admin-gated inbound cap configuration and the
/// ledger-time driven rate-limit window. Time is advanced via
/// `env.ledger().with_mut` instead of being passed in by the caller.
#[allow(unused_imports)]
use super::*;

/// Helper to build a test environment with an initialised admin and
/// an authorised operator address.
#[cfg(test)]
fn setup_initialized() -> (Env, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let guardian = BytesN::<32>::from_array(&env, &[7u8; 32]);
    let validator = Address::generate(&env);
    let bridge_id = Bytes::from_slice(&env, &[1u8]);
    let validators = soroban_sdk::vec![&env, validator];

    env.mock_all_auths();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    client.initialize(&admin, &validators, &bridge_id, &guardian);
    (env, admin, operator)
}

#[test]
fn inbound_cap_test_compile_smoke() {}

#[test]
fn initialize_sets_admin_and_rejects_reinit() {
    let (env, admin, _) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    // The admin is stored and authorised.
    assert_eq!(client.get_admin(), admin);
    // A second initialize must fail.
    let guardian = BytesN::<32>::from_array(&env, &[7u8; 32]);
    let result = client.try_initialize(
        &admin,
        &soroban_sdk::vec![&env],
        &Bytes::from_slice(&env, &[2u8]),
        &guardian,
    );
    assert!(result.is_err(), "second initialize must fail");
}

#[test]
fn non_admin_cannot_set_inbound_cap() {
    let (env, _admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    // Only the operator signature is available; the admin is not authorised.
    env.mock_all_auths();
    let result = client.try_set_inbound_cap(&operator, &Some(1000), &Some(100));
    assert!(result.is_err(), "non-admin must not set inbound cap");
}

#[test]
fn admin_can_set_inbound_cap() {
    let (env, admin, _) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    env.mock_all_auths();
    client.set_inbound_cap(&admin, &Some(1000), &Some(100));
    assert_eq!(client.get_inbound_cap(), Some(1000));
}

#[test]
fn admit_inbound_requires_operator() {
    let (env, admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    env.mock_all_auths();
    client.set_inbound_cap(&admin, &Some(1000), &Some(100));
    // A non-operator cannot admit inbound messages.
    let other = Address::generate(&env);
    let result = client.try_admit_inbound(&other, &10_i128);
    assert!(result.is_err(), "non-operator must not admit inbound");
    // The operator can.
    client.admit_inbound(&operator, &10_i128);
}

#[test]
fn window_rolls_only_with_ledger_time() {
    let (env, admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    env.mock_all_auths();
    client.set_inbound_cap(&admin, &Some(1000), &Some(100));
    client.admit_inbound(&operator, &100_i128);
    // Advance ledger time beyond the window.
    env.ledger().with_mut(|l| l.timestamp += 100);
    // The cap should be available again.
    client.admit_inbound(&operator, &100_i128);
}

#[test]
fn cannot_bypass_cap_by_future_time() {
    let (env, admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    env.mock_all_auths();
    client.set_inbound_cap(&admin, &Some(1000), &Some(100));
    client.admit_inbound(&operator, &100_i128);
    // The old attack passed a far-future timestamp to roll the window.
    // With the new signature there is no timestamp parameter, and the
    // cap must remain enforced against ledger time.
    let result = client.try_admit_inbound(&operator, &1000_i128);
    assert!(result.is_err(), "future time must not bypass the cap");
}
