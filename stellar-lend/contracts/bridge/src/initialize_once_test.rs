#[cfg(test)]
use super::*;
use soroban_sdk::{Bytes, BytesN, Env, Vec};

#[test]
fn test_second_initialize_rejected() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let mut validators = Vec::new(&env);
    validators.push_back(BytesN::from_array(&env, &[1u8; 32]));
    let bridge_id = Bytes::from_slice(&env, b"test_bridge");

    client.initialize(&validators, &bridge_id);

    // Second call with same or different parameters must return AlreadyInitialized
    let res = client.try_initialize(&validators, &bridge_id);
    assert_eq!(res, Err(Ok(BridgeError::AlreadyInitialized)));
}

#[test]
fn test_state_unchanged_after_rejected_reinit() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let mut initial_validators = Vec::new(&env);
    initial_validators.push_back(BytesN::from_array(&env, &[1u8; 32]));
    initial_validators.push_back(BytesN::from_array(&env, &[2u8; 32]));
    let initial_bridge_id = Bytes::from_slice(&env, b"test_bridge");

    client.initialize(&initial_validators, &initial_bridge_id);
    assert_eq!(client.get_epoch(), 0);
    assert_eq!(client.active_validator_count(), 2);

    // Attempt re-initialization with a different validator set and bridge ID
    let mut rogue_validators = Vec::new(&env);
    rogue_validators.push_back(BytesN::from_array(&env, &[9u8; 32]));
    let rogue_bridge_id = Bytes::from_slice(&env, b"rogue_bridge");

    let res = client.try_initialize(&rogue_validators, &rogue_bridge_id);
    assert_eq!(res, Err(Ok(BridgeError::AlreadyInitialized)));

    // Verify epoch and validator count are unchanged
    assert_eq!(client.get_epoch(), 0);
    assert_eq!(client.active_validator_count(), 2);
}

#[test]
fn test_initialize_empty_validator_set_rejected() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let empty_validators = Vec::new(&env);
    let bridge_id = Bytes::from_slice(&env, b"test_bridge");

    let res = client.try_initialize(&empty_validators, &bridge_id);
    assert_eq!(res, Err(Ok(BridgeError::ValidatorSetTooSmall)));
}
