/// Tests for bridge validator-pause functionality with domain separation and nonces.
#[cfg(test)]
#[allow(unused_imports)]
use super::*;
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::Address as _, Bytes, BytesN, Env, Vec};


#[test]
fn validator_pause_test_compile_smoke() {}
fn sign_payload(signing_key: &SigningKey, payload_bytes: &BytesN<32>) -> BytesN<64> {
    let msg: [u8; 32] = payload_bytes.into();
    let signature = signing_key.sign(&msg);
    BytesN::from_array(&Env::default(), &signature.to_bytes())
}

fn make_validators(env: &Env) -> Vec<BytesN<32>> {
    let mut validators = Vec::new(env);
    validators.push_back(BytesN::from_array(env, &[1u8; 32]));
    validators.push_back(BytesN::from_array(env, &[2u8; 32]));
    validators.push_back(BytesN::from_array(env, &[3u8; 32]));
    validators
}

#[test]
fn test_guardian_pause_unpause_happy_path() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let mut csprng = rand::rngs::OsRng;
    let guardian_signing_key = SigningKey::generate(&mut csprng);
    let guardian_pk_bytes = guardian_signing_key.verifying_key().to_bytes();
    let guardian_pk = BytesN::from_array(&env, &guardian_pk_bytes);

    let val_bytes = [1u8; 32];
    let validator = BytesN::from_array(&env, &val_bytes);
    let mut validators = Vec::new(&env);
    validators.push_back(validator.clone());
    validators.push_back(BytesN::from_array(&env, &[2u8; 32]));
    validators.push_back(BytesN::from_array(&env, &[3u8; 32]));

    let bridge_id = Bytes::from_slice(&env, b"test_bridge");
    let admin = Address::generate(&env);
    client.initialize(&validators, &bridge_id);
    client.set_admin(&admin);
    client.set_guardian(&admin, &guardian_pk).unwrap();

    assert_eq!(client.get_guardian_nonce(), 0);

    // Pause validator
    let expires_at = u64::MAX;
    let payload = client.guardian_action_payload(&1u32, &validator.clone(), &expires_at);
    let sig = sign_payload(&guardian_signing_key, &payload);

    client.pause_validator(&validator, &sig, &expires_at);
    assert!(client.is_paused(&validator));
    assert_eq!(client.get_guardian_nonce(), 1);

    // Unpause validator
    let payload_unpause = client.guardian_action_payload(&2u32, &validator.clone(), &expires_at);
    let sig_unpause = sign_payload(&guardian_signing_key, &payload_unpause);

    client.unpause_validator(&validator, &sig_unpause, &expires_at);
    assert!(!client.is_paused(&validator));
    assert_eq!(client.get_guardian_nonce(), 2);
}

#[test]
fn test_guardian_replay_and_cross_bridge() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id_a = env.register(Bridge, ());
    let contract_id_b = env.register(Bridge, ());
    let client_a = BridgeClient::new(&env, &contract_id_a);
    let client_b = BridgeClient::new(&env, &contract_id_b);

    let mut csprng = rand::rngs::OsRng;
    let guardian_key = SigningKey::generate(&mut csprng);
    let guardian_pk = BytesN::from_array(&env, &guardian_key.verifying_key().to_bytes());

    let val = BytesN::from_array(&env, &[1u8; 32]);
    let validators = make_validators(&env);

    let admin_a = Address::generate(&env);
    let admin_b = Address::generate(&env);

    client_a.initialize(&validators, &Bytes::from_slice(&env, b"bridge_a"));
    client_a.set_admin(&admin_a);
    client_a.set_guardian(&admin_a, &guardian_pk).unwrap();

    client_b.initialize(&validators, &Bytes::from_slice(&env, b"bridge_b"));
    client_b.set_admin(&admin_b);
    client_b.set_guardian(&admin_b, &guardian_pk).unwrap();

    let expires = u64::MAX;
    let payload_a = client_a.guardian_action_payload(&1u32, &val, &expires);
    let sig_a = sign_payload(&guardian_key, &payload_a);

    // Successful pause on A
    client_a.pause_validator(&val, &sig_a, &expires);
    assert_eq!(client_a.get_guardian_nonce(), 1);

    // Replay on A with same nonce fails
    let err_replay = client_a.try_pause_validator(&val, &sig_a, &expires);
    assert!(err_replay.is_err());

    // Cross-bridge replay on B fails because bridge_id / contract_id differ
    let err_cross = client_b.try_pause_validator(&val, &sig_a, &expires);
    assert!(err_cross.is_err());
}

#[test]
fn test_set_guardian_requires_admin() {
    let env = Env::default();
    // No mock_all_auths — auth must come from the caller.
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let validators = make_validators(&env);
    client.initialize(&validators, &Bytes::from_slice(&env, b"bridge_auth_test"));

    let guardian_pk = BytesN::from_array(&env, &[9u8; 32]);

    // No admin set yet — must fail.
    let no_admin_caller = Address::generate(&env);
    let result = client.try_set_guardian(&no_admin_caller, &guardian_pk);
    assert!(result.is_err());
}

#[test]
fn test_set_guardian_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let validators = make_validators(&env);
    client.initialize(&validators, &Bytes::from_slice(&env, b"bridge_non_admin_test"));

    let admin = Address::generate(&env);
    client.set_admin(&admin);

    let non_admin = Address::generate(&env);
    let guardian_pk = BytesN::from_array(&env, &[9u8; 32]);

    let result = client.try_set_guardian(&non_admin, &guardian_pk);
    // Expect Err(Ok(NotAdmin))
    assert!(
        matches!(result, Err(Ok(BridgeError::NotAdmin))),
        "expected NotAdmin, got {:?}", result
    );
}
