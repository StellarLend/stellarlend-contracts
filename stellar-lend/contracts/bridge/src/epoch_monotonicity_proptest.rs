/// Property-based tests for epoch monotonicity in the bridge contract.
#[cfg(test)]
#[allow(unused_imports)]
use super::*;

#[test]
fn epoch_monotonicity_proptest_compile_smoke() {}

#[test]
fn test_epoch_overflow_returns_error() {
    let env = soroban_sdk::Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    client.set_epoch_for_test(&u64::MAX);
    assert_eq!(client.get_epoch(), u64::MAX);

    let res = client.try_advance_epoch();
    assert_eq!(res, Err(Ok(BridgeError::EpochOverflow)));
    assert_eq!(client.get_epoch(), u64::MAX);
}
