//! Shared test utilities for bridge contract tests.

use crate::{Bridge, BridgeClient};
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env};

/// Deploy and initialize a `Bridge` contract with a single validator,
/// an admin address, and a default outbound cap of 1000 units per 86400s.
pub fn setup_bridge(env: &Env) -> (Address, BridgeClient<'_>, Address) {
    let bridge_id = env.register(Bridge, ());
    let client = BridgeClient::new(env, &bridge_id);

    let admin = Address::generate(env);
    let validators = soroban_sdk::vec![env, BytesN::from_array(env, &[1u8; 32])];
    let bridge_id_bytes = Bytes::from_slice(env, b"test_bridge");

    client.initialize(&validators, &bridge_id_bytes);
    client.set_admin(&admin);

    // Configure outbound cap: 1000 units per 86400 seconds
    client.set_outbound_cap(&1000i128, &86400u64, &0u64);

    (bridge_id, client, admin)
}

/// Register a fresh Stellar Asset Contract token for testing.
pub fn register_token(env: &Env) -> Address {
    let token_admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(token_admin)
        .address()
}
