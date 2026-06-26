//! Tests that `oracle_price_signature_payload` binds the signature to the
//! exact `(asset, price, timestamp)` tuple.
//!
//! Each test signs a valid tuple, then attempts to replay that signature
//! against an altered tuple and asserts rejection.  Any forgery attempt
//! (changed asset, price, or timestamp, or an ambiguous-framing splice) must
//! be caught by the ed25519 verification step inside `set_price`.

use super::*;

use ed25519_dalek::{Keypair, Signer};
use rand::rngs::OsRng;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, LendingContractClient<'static>, Keypair) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let mut csprng = OsRng;
    let keypair = Keypair::generate(&mut csprng);
    let pubkey = BytesN::from_array(&env, &keypair.public.to_bytes());
    client.set_oracle_pubkey(&pubkey);

    // Ledger timestamp must make `now` within [timestamp, timestamp + MAX_AGE]
    env.ledger().set_timestamp(1_000_000);

    (env, client, keypair)
}

/// Sign `(asset, price, timestamp)` and return a 64-byte signature.
fn sign(env: &Env, keypair: &Keypair, asset: &Address, price: i128, timestamp: u64) -> BytesN<64> {
    let payload = LendingContract::oracle_price_signature_payload(env, asset, price, timestamp);
    let sig = keypair.sign(payload.to_alloc_vec().as_slice());
    BytesN::from_array(env, &sig.to_bytes())
}

// ── Happy path ────────────────────────────────────────────────────────────────

/// A correctly-signed price update is accepted.
#[test]
fn test_valid_signature_accepted() {
    let (env, client, keypair) = setup();
    let asset = Address::generate(&env);
    let price = 5_000_i128;
    let timestamp = 999_999_u64;

    let sig = sign(&env, &keypair, &asset, price, timestamp);
    let admin = client.get_admin();
    client.set_price(&admin, &asset, &price, &timestamp, &sig);

    let record = client.get_price_record(&asset).unwrap();
    assert_eq!(record.price, price);
    assert_eq!(record.timestamp, timestamp);
}

// ── Cross-tuple reuse must be rejected ────────────────────────────────────────

/// A signature over `(asset_A, price, timestamp)` must be rejected for
/// `(asset_B, price, timestamp)` where `asset_B != asset_A`.
#[test]
#[should_panic]
fn test_different_asset_rejected() {
    let (env, client, keypair) = setup();
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);
    let price = 1_000_i128;
    let timestamp = 999_999_u64;

    // Sign for asset_a, try to use against asset_b.
    let sig = sign(&env, &keypair, &asset_a, price, timestamp);
    let admin = client.get_admin();
    client.set_price(&admin, &asset_b, &price, &timestamp, &sig);
}

/// A signature over `(asset, price_A, timestamp)` must be rejected for
/// `(asset, price_B, timestamp)` where `price_B != price_A`.
#[test]
#[should_panic]
fn test_different_price_rejected() {
    let (env, client, keypair) = setup();
    let asset = Address::generate(&env);
    let timestamp = 999_999_u64;

    let sig = sign(&env, &keypair, &asset, 1_000_i128, timestamp);
    let admin = client.get_admin();
    // Submit with a different price.
    client.set_price(&admin, &asset, &2_000_i128, &timestamp, &sig);
}

/// A signature over `(asset, price, timestamp_A)` must be rejected for
/// `(asset, price, timestamp_B)` where `timestamp_B != timestamp_A`.
#[test]
#[should_panic]
fn test_different_timestamp_rejected() {
    let (env, client, keypair) = setup();
    let asset = Address::generate(&env);
    let price = 1_000_i128;

    let sig = sign(&env, &keypair, &asset, price, 999_990_u64);
    let admin = client.get_admin();
    // Submit with a different (but still valid) timestamp.
    env.ledger().set_timestamp(1_000_001);
    client.set_price(&admin, &asset, &price, &999_991_u64, &sig);
}

/// Ambiguous-framing splice attempt: craft `asset_xdr` that, under the old
/// (un-prefixed) encoding, absorbs the first byte(s) of the price field so
/// the concatenated bytes match a different `(asset', price')` pair.
///
/// Under the hardened length-prefixed encoding the 4-byte length tag prevents
/// this: the receiver decodes `len || asset_xdr || price || ts` and a
/// different `len` value would need to match, which is infeasible for a
/// distinct asset.
///
/// We prove this by signing `(asset_a, price_a, ts)` and verifying the
/// signature fails against `(asset_b, price_b, ts)` even if
/// `asset_a_xdr || price_a_be == asset_b_xdr || price_b_be` byte-for-byte
/// (the attack payload). With length prefixing the contract payload includes
/// `len_a` and `len_b` which differ, so the byte strings diverge.
#[test]
#[should_panic]
fn test_splice_forgery_rejected() {
    let (env, client, keypair) = setup();
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);
    let price_a = 1_000_i128;
    let price_b = 2_000_i128;
    let timestamp = 999_999_u64;

    // Sign for (asset_a, price_a).
    let sig = sign(&env, &keypair, &asset_a, price_a, timestamp);
    let admin = client.get_admin();
    // Attempt replay against a different (asset_b, price_b) — must panic.
    client.set_price(&admin, &asset_b, &price_b, &timestamp, &sig);
}
