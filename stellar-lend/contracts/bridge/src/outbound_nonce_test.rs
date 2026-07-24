use super::*;
use soroban_sdk::Env;

// ── peek_outbound_nonce ──────────────────────────────────────────────────────

/// A fresh destination starts at nonce 0.
#[test]
fn test_peek_fresh_destination_is_zero() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    assert_eq!(client.peek_outbound_nonce(&1u32), 0u64);
    assert_eq!(client.peek_outbound_nonce(&999u32), 0u64);
}

// ── next_outbound_nonce ──────────────────────────────────────────────────────

/// First nonce per fresh destination must be exactly 0.
#[test]
fn test_first_nonce_is_zero() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let nonce = client.next_outbound_nonce(&42u32);
    assert_eq!(nonce, 0u64);
}

/// Nonces are strictly monotonic for the same destination.
#[test]
fn test_monotonic_increase_single_destination() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let dest: u32 = 7;
    let n0 = client.next_outbound_nonce(&dest);
    let n1 = client.next_outbound_nonce(&dest);
    let n2 = client.next_outbound_nonce(&dest);

    assert_eq!(n0, 0u64);
    assert_eq!(n1, 1u64);
    assert_eq!(n2, 2u64);
}

/// peek returns the value that next_outbound_nonce will return next.
#[test]
fn test_peek_reflects_next_value() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let dest: u32 = 3;

    // Before any messages: peek = 0, next returns 0.
    assert_eq!(client.peek_outbound_nonce(&dest), 0u64);
    let n0 = client.next_outbound_nonce(&dest);
    assert_eq!(n0, 0u64);

    // After first message: peek = 1, next returns 1.
    assert_eq!(client.peek_outbound_nonce(&dest), 1u64);
    let n1 = client.next_outbound_nonce(&dest);
    assert_eq!(n1, 1u64);

    // After second message: peek = 2.
    assert_eq!(client.peek_outbound_nonce(&dest), 2u64);
}

/// Sequences for different destinations are independent.
#[test]
fn test_independent_sequences_per_destination() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let dest_a: u32 = 1;
    let dest_b: u32 = 2;

    // Advance dest_a three times.
    client.next_outbound_nonce(&dest_a);
    client.next_outbound_nonce(&dest_a);
    client.next_outbound_nonce(&dest_a);

    // dest_b should still start at 0.
    assert_eq!(client.peek_outbound_nonce(&dest_b), 0u64);
    let nb0 = client.next_outbound_nonce(&dest_b);
    assert_eq!(nb0, 0u64);

    // dest_a should now be at 3.
    assert_eq!(client.peek_outbound_nonce(&dest_a), 3u64);
}

/// peek does not advance the nonce.
#[test]
fn test_peek_does_not_advance_nonce() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let dest: u32 = 10;

    // Multiple peeks should not change anything.
    for _ in 0..5 {
        assert_eq!(client.peek_outbound_nonce(&dest), 0u64);
    }

    // The first real call still returns 0.
    assert_eq!(client.next_outbound_nonce(&dest), 0u64);
}

/// Rollover near u64::MAX is rejected.
#[test]
fn test_nonce_overflow_near_max_is_rejected() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());

    let dest: u32 = 55;

    // Manually seed the nonce map to u64::MAX so the next increment overflows.
    env.as_contract(&contract_id, || {
        let mut nonces: Map<u32, u64> = env
            .storage()
            .persistent()
            .get::<BridgeDataKey, Map<u32, u64>>(&BridgeDataKey::OutboundNonces)
            .unwrap_or_else(|| Map::new(&env));
        nonces.set(dest, u64::MAX);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::OutboundNonces, &nonces);
    });

    // The call must return NonceOverflow.
    let client = BridgeClient::new(&env, &contract_id);
    let result = client.try_next_outbound_nonce(&dest);
    assert!(result.is_err(), "expected NonceOverflow error");
}

/// Many destinations can coexist without interfering with each other.
#[test]
fn test_many_destinations_independent() {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);

    let dests: [u32; 5] = [100, 200, 300, 400, 500];

    // Send 3 messages to each destination.
    for &dest in &dests {
        for expected_nonce in 0u64..3 {
            let n = client.next_outbound_nonce(&dest);
            assert_eq!(n, expected_nonce, "dest={dest} expected nonce {expected_nonce}");
        }
    }

    // Verify peek for each destination is now 3.
    for &dest in &dests {
        assert_eq!(
            client.peek_outbound_nonce(&dest),
            3u64,
            "dest={dest} peek should be 3"
        );
    }
}
