use soroban_sdk::{
    testutils::Ledger, Bytes, BytesN, Env, Vec,
};

use crate::{Bridge, BridgeClient, BridgeDataKey, SourceDomain};
use stellar_lend_common::ttl::PERSISTENT_LIFETIME_THRESHOLD;

#[test]
fn keeper_refreshes_validators_and_consumed_message_markers() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);
    let source = SourceDomain {
        chain_id: 1,
        network_passphrase: Bytes::from_slice(&env, b"network"),
        contract_id: Bytes::from_slice(&env, b"source-bridge"),
    };
    let nonce = 4_u64;
    let source_hash = Bridge::source_domain_hash(&env, &source);
    let message_id = Bridge::inbound_message_id(&env, &source_hash, nonce);
    let remaining = PERSISTENT_LIFETIME_THRESHOLD / 2;

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&BridgeDataKey::Validators, &Vec::<BytesN<32>>::new(&env));
        env.storage()
            .persistent()
            .set(&BridgeDataKey::ConsumedInboundMessage(message_id.clone()), &true);
    });

    for _ in 0..5 {
        let validators_ttl = env.as_contract(&contract_id, || {
            env.storage()
                .persistent()
                .get_ttl(&BridgeDataKey::Validators)
        });
        env.ledger().with_mut(|ledger| {
            ledger.sequence_number += validators_ttl.saturating_sub(remaining)
        });
        client.bump();
        client.bump_consumed_message(&source, &nonce);
    }

    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().get_ttl(&BridgeDataKey::Validators) > remaining);
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&BridgeDataKey::ConsumedInboundMessage(message_id))
                > remaining
        );
        assert!(
            env.storage()
                .persistent()
                .get::<_, bool>(&BridgeDataKey::ConsumedInboundMessage(
                    message_id.clone()
                ))
                .unwrap_or(false)
        );
    });
}

#[test]
fn consuming_inbound_message_keeps_validator_and_replay_state_alive() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);
    let admin = soroban_sdk::Address::generate(&env);
    let mut validators = Vec::new(&env);
    validators.push_back(BytesN::from_array(&env, &[1; 32]));
    client.initialize(&validators, &Bytes::new(&env));
    client.set_admin(&admin);
    let source = SourceDomain {
        chain_id: 2,
        network_passphrase: Bytes::from_slice(&env, b"testnet"),
        contract_id: Bytes::from_slice(&env, b"remote-bridge"),
    };
    client.register_source_domain(&admin, &source);
    let nonce = 0_u64;
    let source_hash = Bridge::source_domain_hash(&env, &source);
    let message_id = client.consume_inbound_message(&source, &nonce);

    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().get_ttl(&BridgeDataKey::Validators) > 0);
        assert!(
            env.storage()
                .persistent()
                .get_ttl(&BridgeDataKey::ConsumedInboundMessage(message_id.clone()))
                > 0
        );
        assert_eq!(
            message_id,
            Bridge::inbound_message_id(&env, &source_hash, nonce)
        );
    });
}
