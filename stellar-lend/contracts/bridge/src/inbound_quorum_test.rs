#[cfg(test)]
mod inbound_quorum_tests {
    use crate::*;
    use ed25519_dalek::{Signer, SigningKey};
    use soroban_sdk::{testutils::Address as _, Bytes, BytesN, Env, Vec};

    fn setup_bridge(env: &Env) -> (BridgeClient, Vec<SigningKey>, Vec<BytesN<32>>) {
        env.mock_all_auths();
        let contract_id = env.register(Bridge, ());
        let client = BridgeClient::new(env, &contract_id);

        let keys: std::vec::Vec<SigningKey> = (0..3)
            .map(|i| { let mut seed = [0u8; 32]; seed[0] = i + 1; SigningKey::from_bytes(&seed) })
            .collect();
        let mut validators = Vec::new(env);
        for k in &keys {
            validators.push_back(BytesN::from_array(env, &k.verifying_key().to_bytes()));
        }

        let bridge_id = Bytes::from_slice(env, b"test_inbound_bridge");
        client.initialize(&validators, &bridge_id);

        let sdk_keys: Vec<BytesN<32>> = {
            let mut v = Vec::new(env);
            for k in &keys { v.push_back(BytesN::from_array(env, &k.verifying_key().to_bytes())); }
            v
        };
        (client, keys, sdk_keys)
    }

    fn register_source(client: &BridgeClient, env: &Env) -> SourceDomain {
        let admin = Address::generate(env);
        client.set_admin(&admin);
        let source = SourceDomain { chain_id: 1u32, contract: BytesN::from_array(env, &[0xABu8; 32]) };
        client.register_source_domain(&admin, &source);
        source
    }

    fn sign_inbound(env: &Env, client: &BridgeClient, keys: &[SigningKey], source: &SourceDomain, nonce: u64, epoch: u64) -> Vec<(BytesN<32>, BytesN<64>)> {
        let payload = client.inbound_message_payload(source, &nonce, &epoch);
        let payload_bytes: [u8; 32] = payload.into();
        let mut proofs = Vec::new(env);
        for k in keys {
            let sig = k.sign(&payload_bytes);
            proofs.push_back((
                BytesN::from_array(env, &k.verifying_key().to_bytes()),
                BytesN::from_array(env, &sig.to_bytes()),
            ));
        }
        proofs
    }

    #[test]
    fn test_consume_with_valid_quorum_succeeds() {
        let env = Env::default();
        let (client, keys, _) = setup_bridge(&env);
        let source = register_source(&client, &env);
        let proofs = sign_inbound(&env, &client, &keys, &source, 0, 0);
        let result = client.consume_inbound_message(&source, &0u64, &0u64, &proofs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_consume_without_proofs_rejected() {
        let env = Env::default();
        let (client, _, _) = setup_bridge(&env);
        let source = register_source(&client, &env);
        let empty: Vec<(BytesN<32>, BytesN<64>)> = Vec::new(&env);
        let result = client.try_consume_inbound_message(&source, &0u64, &0u64, &empty);
        assert!(matches!(result, Err(Ok(BridgeError::EmptyProofs))));
    }

    #[test]
    fn test_consume_below_threshold_rejected() {
        let env = Env::default();
        let (client, keys, _) = setup_bridge(&env);
        let source = register_source(&client, &env);
        // Only 1 out of 3 validators signs — below the 2/3 threshold.
        let proofs = sign_inbound(&env, &client, &keys[..1], &source, 0, 0);
        let result = client.try_consume_inbound_message(&source, &0u64, &0u64, &proofs);
        assert!(matches!(result, Err(Ok(BridgeError::InsufficientQuorum))));
    }

    #[test]
    fn test_proof_for_other_nonce_rejected() {
        let env = Env::default();
        let (client, keys, _) = setup_bridge(&env);
        let source = register_source(&client, &env);
        // Sign nonce 1 but try to consume nonce 0 — ed25519_verify will trap.
        let proofs = sign_inbound(&env, &client, &keys, &source, 1, 0);
        let result = client.try_consume_inbound_message(&source, &0u64, &0u64, &proofs);
        assert!(result.is_err());
    }

    #[test]
    fn test_proof_replayed_after_consume_rejected() {
        let env = Env::default();
        let (client, keys, _) = setup_bridge(&env);
        let source = register_source(&client, &env);
        let proofs = sign_inbound(&env, &client, &keys, &source, 0, 0);
        client.consume_inbound_message(&source, &0u64, &0u64, &proofs).unwrap();
        // Replay: same proofs, same nonce — but nonce is now advanced.
        let result = client.try_consume_inbound_message(&source, &0u64, &0u64, &proofs);
        assert!(matches!(result, Err(Ok(BridgeError::MessageAlreadyConsumed))));
    }
}
