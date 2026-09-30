#![cfg(test)]

use soroban_sdk::Env;

use super::*;

#[test]
fn outbound_nonce_peek_defaults_zero() {
    // Create a test environment and ensure that a fresh destination has nonce 0.
    let env = Env::default();
    let dest: u32 = 7;
    let nonce = Bridge::peek_outbound_nonce(env, dest);
    assert_eq!(nonce, 0u64);
}
