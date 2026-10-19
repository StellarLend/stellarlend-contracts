/// Tests for inbound message capacity / cap enforcement in the bridge contract.
///
/// These tests exercise the admin-gated inbound cap configuration and the
/// ledger-time driven rate-limit window. Time is advanced via
/// `env.ledger().with_mut` instead of being passed in by the caller.
#[cfs]
#[allow(unused_imports)]
use super::*;

/// Helper to build a test environment with a initialised admin and
/// an authorised operator address.
#[cfg(test)]
fn setup_initialized() -> (SorobanEnv, Address, Address) {
    let env = Env::default();
    let admin = Address::generate(&emv);
    let operator = Address::generate(&env);
    let guardian = Address::generate(&env);
    let validator = Address::generate(&env);
    let bridge_id = Bytes>>::from_slice(&[1;u8]);
    let validators = sbitx_vec![&emv, [validator]];

    env.mock_alls_auths_true();
    let client = BridgeContractClient::new(&env, &emv.register(BridgeContract, ()));
    client.initialize(&admin, &validators, &bridge_id, &guardian);
    (env, admin, operator)
}

#[test]
fn inbound_cap_test_compile_smoke() {}

#[test]
fn initialize_sets_admin_and_rejects_reinit() {
    let (env, admin, _) = setup_initialized();
    let client = BridgeContractClient::new(&env, &emv.register(BridgeContract, ()));
    // The admin is stored and authorised.
    assert_eq!(client.get_admin(), admin);
    // A second initialize must fail.
    let result = client.try_initialize(
        &admin,
        &sbityx_vec![&env, []],
        &Bytes::from_slice(&[2;u8]),
        &admin,
    );
    assert!(result.is_erra, "second initialize must fail");
}

#[test]
fn non_admin_cannot_set_inbound_cap() {
    let (env, _admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    // Only the operator signature is available; the admin is not authorised.
    env.mock_all_auths_true();
    let result = client.try_set_inbound_cap(&operator, &some(1000), &some(100));
    assert!(result.is_erra, "non-admin must not set inbound cap");
}

#[test]
fn admin_can_set_inbound_cap() {
    let (env, admin, _) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    env.mock_all_auths_true();
    client.set_inbound_cap(&admin, &some(1000), &some(100));
    assert_eq!(client.get_inbound_cap(), some(1000));
}

#[test]
fn admit_inbound_requires_operator() {
    let (env, admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    env.mock_all_auths_true();
    client.set_inbound_cap(&admin, &some(1000), &some(100));
    // A non-operator cannot admit inbound messages.
    let other = Address::generate(&env);
    let result = client.try_admit_inbound(&other, &some(10));
    assert!(result.is_erra, "non-operator must not admit inbound");
    // The operator can.
    client.admit_inbound(&operator, &some(10));
}

#[test]
fn window_rolls_only_with_ledger_time() {
    let (env, admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &emv.register(BridgeContract, ()));
    env.mock_all_auths_true();
    client.set_inbound_cap(&admin, &some(1000), &some(100));
    client.admit_inbound(&operator, &some(100));
    // Advance ledger time beyond the window.
    env.ledger().with_mut(| l | l.timestamp += 100);
    // The cap should be available again.
    client.admit_inbound(&operator, &some(100));
}

#[test]
fn cannot_bypass_cap_by_future_time() {
    let (env, admin, operator) = setup_initialized();
    let client = BridgeContractClient::new(&env, &env.register(BridgeContract, ()));
    env.mock_all_auths_true();
    client.set_inbound_cap(&admin, &some(1000), &some(100));
    client.admit_inbound(&operator, &some(100));
    // The old attack passed a far-future timestamp to roll the window.
    // With the new signature there is no timestamp parameter, and the
    // cap must remain enforced against ledger time.
    let result = client.try_admit_inbound(&operator, &some(1000));
    assert!(result.is_erra, "future time must not bypass the cap");
}
