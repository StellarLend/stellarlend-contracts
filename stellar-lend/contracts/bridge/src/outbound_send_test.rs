//! Tests for the outbound send entrypoint.
//!
//! Verifies that send() correctly:
//! - Escrows tokens
//! - Enforces the outbound cap
//! - Allocates nonces sequentially per destination
//! - Emits OutboundSentEvent with correct fields
//! - Computes the message hash correctly
//! - Validates inputs (amount, recipient, token allow-list, auth)

use crate::{Bridge, BridgeClient, BridgeError, OutboundSentEvent, OUTBOUND_MSG_DOMAIN};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token::{StellarAssetClient, TokenClient},
    xdr::ToXdr,
    Address, Bytes, BytesN, Env, IntoVal, Symbol,
};

fn setup_bridge(env: &Env) -> (Address, BridgeClient<'_>, Address) {
    let bridge_id = env.register_contract(None, Bridge);
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

fn register_token(env: &Env) -> Address {
    let token_admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(token_admin)
        .address()
}

#[test]
fn happy_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (bridge_addr, client, admin) = setup_bridge(&env);

    // Register and allow token
    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    // Fund sender
    let sender = Address::generate(&env);
    let token_client = TokenClient::new(&env, &token);
    StellarAssetClient::new(&env, &token).mint(&sender, &5000i128);

    // Send 100 tokens to dest_chain 42
    let dest_chain = 42u32;
    let recipient = Bytes::from_slice(&env, b"recipient_address_on_chain_42");
    let amount = 100i128;

    let (nonce, message_hash) =
        client.send(&sender, &dest_chain, &recipient, &token, &amount);

    // Verify nonce
    assert_eq!(nonce, 0, "first nonce should be 0");

    // Verify escrow
    let escrowed = client.get_escrowed(&token);
    assert_eq!(escrowed, amount, "escrowed amount should match");

    // Verify token transfer
    let sender_balance = token_client.balance(&sender);
    assert_eq!(sender_balance, 4900, "sender balance should decrease");
    let bridge_balance = token_client.balance(&bridge_addr);
    assert_eq!(bridge_balance, 100, "bridge should hold escrowed tokens");

    // Verify event
    let mut expected_topics = soroban_sdk::Vec::new(&env);
    expected_topics.push_back(Symbol::new(&env, "outbound").into_val(&env));
    expected_topics.push_back(dest_chain.into_val(&env));
    expected_topics.push_back(nonce.into_val(&env));
    
    let expected_data = OutboundSentEvent {
        dest_chain,
        nonce,
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        amount,
        message_hash: message_hash.clone(),
    };
    
    let expected_events: soroban_sdk::Vec<(Address, soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)> = 
        soroban_sdk::vec![&env, (bridge_addr.clone(), expected_topics, expected_data.into_val(&env))];
    
    assert_eq!(
        env.events().all(),
        expected_events,
        "outbound event should match expected structure"
    );
}

#[test]
fn nonce_sequencing_per_destination() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &10000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    // Send to dest_chain 1
    let (nonce1, _) = client.send(&sender, &1u32, &recipient, &token, &50i128);
    assert_eq!(nonce1, 0);

    // Send to dest_chain 2
    let (nonce2, _) = client.send(&sender, &2u32, &recipient, &token, &50i128);
    assert_eq!(nonce2, 0, "dest_chain 2 should start at nonce 0");

    // Send again to dest_chain 1
    let (nonce3, _) = client.send(&sender, &1u32, &recipient, &token, &50i128);
    assert_eq!(nonce3, 1, "dest_chain 1 nonce should increment");

    // Send again to dest_chain 2
    let (nonce4, _) = client.send(&sender, &2u32, &recipient, &token, &50i128);
    assert_eq!(nonce4, 1, "dest_chain 2 nonce should increment");
}

#[test]
fn cap_boundary_exact_passes() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &10000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    // Cap is 1000, send exactly 1000
    let result = client.try_send(&sender, &1u32, &recipient, &token, &1000i128);
    assert!(result.is_ok(), "should allow sending exactly the cap amount");
}

#[test]
fn cap_boundary_over_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &10000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    // Cap is 1000, try to send 1001
    let result = client.try_send(&sender, &1u32, &recipient, &token, &1001i128);
    assert_eq!(
        result,
        Err(Ok(BridgeError::OutboundCapExceeded)),
        "should reject exceeding cap"
    );
}

#[test]
fn cap_accumulates_across_sends() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &10000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    // Send 500 twice = 1000 total (at cap)
    client.send(&sender, &1u32, &recipient, &token, &500i128);
    client.send(&sender, &1u32, &recipient, &token, &500i128);

    // Try to send 1 more
    let result = client.try_send(&sender, &1u32, &recipient, &token, &1i128);
    assert_eq!(
        result,
        Err(Ok(BridgeError::OutboundCapExceeded)),
        "should reject after cap is exhausted"
    );
}

#[test]
fn window_rollover_resets_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &50000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    // Send 1000 (at cap) at time 0
    client.send(&sender, &1u32, &recipient, &token, &1000i128);

    // No more capacity
    let result = client.try_send(&sender, &1u32, &recipient, &token, &1i128);
    assert!(result.is_err(), "should be at cap");

    // Advance time past the window (86400 seconds)
    env.ledger().with_mut(|li| {
        li.timestamp = 86401;
    });

    // Should be able to send again
    let result = client.try_send(&sender, &1u32, &recipient, &token, &1000i128);
    assert!(result.is_ok(), "cap should reset after window rollover");
}

#[test]
fn disallowed_token_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, _admin) = setup_bridge(&env);

    let token = register_token(&env);
    // Do NOT allow the token

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    let result = client.try_send(&sender, &1u32, &recipient, &token, &100i128);
    assert_eq!(
        result,
        Err(Ok(BridgeError::TokenNotAllowed)),
        "should reject disallowed token"
    );
}

#[test]
fn zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    let recipient = Bytes::from_slice(&env, b"recipient");

    let result = client.try_send(&sender, &1u32, &recipient, &token, &0i128);
    assert_eq!(
        result,
        Err(Ok(BridgeError::InvalidAmount)),
        "should reject zero amount"
    );
}

#[test]
fn negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    let recipient = Bytes::from_slice(&env, b"recipient");

    let result = client.try_send(&sender, &1u32, &recipient, &token, &-100i128);
    assert_eq!(
        result,
        Err(Ok(BridgeError::InvalidAmount)),
        "should reject negative amount"
    );
}

#[test]
fn empty_recipient_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000i128);

    let recipient = Bytes::new(&env); // empty

    let result = client.try_send(&sender, &1u32, &recipient, &token, &100i128);
    assert_eq!(
        result,
        Err(Ok(BridgeError::RecipientEmpty)),
        "should reject empty recipient"
    );
}

#[test]
fn overlength_recipient_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000i128);

    // Create a recipient that exceeds MAX_RECIPIENT_LEN (128 bytes)
    let mut long_recipient = Bytes::new(&env);
    for _ in 0..129 {
        long_recipient.push_back(0x42);
    }

    let result = client.try_send(&sender, &1u32, &long_recipient, &token, &100i128);
    assert_eq!(
        result,
        Err(Ok(BridgeError::RecipientTooLong)),
        "should reject overlength recipient"
    );
}

#[test]
fn message_hash_is_deterministic() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000i128);

    let dest_chain = 42u32;
    let recipient = Bytes::from_slice(&env, b"recipient");
    let amount = 100i128;

    let (nonce, message_hash) = client.send(&sender, &dest_chain, &recipient, &token, &amount);

    // Recompute the hash manually
    let bridge_id = Bytes::from_slice(&env, b"test_bridge");
    let mut data = Bytes::new(&env);
    data.extend_from_slice(OUTBOUND_MSG_DOMAIN);
    data.append(&bridge_id);
    data.extend_from_slice(&dest_chain.to_le_bytes());
    data.extend_from_slice(&nonce.to_le_bytes());
    data.append(&sender.to_xdr(&env));
    data.append(&recipient);
    data.append(&token.to_xdr(&env));
    data.extend_from_slice(&amount.to_le_bytes());
    let expected_hash: BytesN<32> = env.crypto().sha256(&data).into();

    assert_eq!(
        message_hash,
        expected_hash,
        "message hash should match independent recomputation"
    );
}

#[test]
fn sender_auth_required() {
    let env = Env::default();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    env.mock_all_auths();
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &1000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    // Clear all auths to test that sender auth is checked
    env.mock_auths(&[]);

    let result = client.try_send(&sender, &1u32, &recipient, &token, &100i128);
    assert!(result.is_err(), "should require sender authorization");
}

#[test]
fn failure_atomicity_cap_check_leaves_nonce_unchanged() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, client, admin) = setup_bridge(&env);

    let token = register_token(&env);
    client.set_allowed_token(&admin, &token, &true);

    let sender = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&sender, &10000i128);

    let recipient = Bytes::from_slice(&env, b"recipient");

    // Check initial nonce
    let nonce_before = client.peek_outbound_nonce(&1u32);

    // Try to exceed cap (should fail)
    let result = client.try_send(&sender, &1u32, &recipient, &token, &2000i128);
    assert!(result.is_err());

    // Verify nonce hasn't changed
    let nonce_after = client.peek_outbound_nonce(&1u32);
    assert_eq!(
        nonce_before, nonce_after,
        "nonce should not change on failed send"
    );

    // Verify no tokens moved
    let escrowed = client.get_escrowed(&token);
    assert_eq!(escrowed, 0, "no tokens should be escrowed on failed send");
}
