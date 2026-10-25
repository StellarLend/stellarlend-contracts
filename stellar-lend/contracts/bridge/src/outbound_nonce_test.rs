use crate::{Bridge, BridgeClient};
use soroban_sdk::Env;

#[test]
fn outbound_nonce_peek_defaults_zero() {
    // Create a test environment and ensure that a fresh destination has nonce 0.
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);
    let dest: u32 = 7;
    let nonce = client.peek_outbound_nonce(&dest);
    assert_eq!(nonce, 0u64);
}
