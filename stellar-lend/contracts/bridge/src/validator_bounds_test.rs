#![cfg(test)]

//! Tests for validator-set size bounds and duplicate-key rejection in
//! `Bridge::rotate_validators`.
//!
//! # Coverage matrix
//!
//! | Scenario | Expected outcome |
//! |---|---|
//! | `new_set` below `MIN_VALIDATORS` (empty) | **Rejected** — `ValidatorSetTooSmall` |
//! | `new_set` below `MIN_VALIDATORS` (1 validator) | **Rejected** — `ValidatorSetTooSmall` |
//! | `new_set` below `MIN_VALIDATORS` (2 validators) | **Rejected** — `ValidatorSetTooSmall` |
//! | `new_set` exactly `MIN_VALIDATORS` | **Accepted** (if quorum met) |
//! | `new_set` exactly `MAX_VALIDATORS` | **Accepted** (if quorum met) |
//! | `new_set` above `MAX_VALIDATORS` | **Rejected** — `ValidatorSetTooLarge` |
//! | `new_set` with duplicate keys | **Rejected** — `DuplicateValidatorKey` |
//! | `new_set` valid mid-size rotation | **Accepted** (if quorum met) |

use crate::{Bridge, BridgeError, ValidatorSet, MIN_VALIDATORS, MAX_VALIDATORS};
use ed25519_dalek::{Keypair, Signature, Signer};

// ---------------------------------------------------------------------------
// Deterministic test helpers
// ---------------------------------------------------------------------------

/// Build a deterministic `Keypair` from a single-byte `index` seed.
///
/// Uses the same algorithm as `rotation_test::det_keypair` so that tests
/// are reproducible without relying on `OsRng`.
fn det_keypair(index: u8) -> Keypair {
    let mut seed = [0u8; 32];
    seed[0] = index.wrapping_add(1);
    for i in 1..32 {
        seed[i] = index.wrapping_mul(7).wrapping_add(i as u8);
    }
    Keypair::from_bytes(&{
        use ed25519_dalek::SecretKey;
        let secret = SecretKey::from_bytes(&seed).expect("valid secret key");
        let public: ed25519_dalek::PublicKey = (&secret).into();
        let mut combined = [0u8; 64];
        combined[..32].copy_from_slice(&seed);
        combined[32..].copy_from_slice(public.as_bytes());
        combined
    })
    .expect("valid keypair from seed")
}

/// Build `n` deterministic keypairs with indices 0..n.
fn det_keypairs(n: u8) -> Vec<Keypair> {
    (0..n).map(det_keypair).collect()
}

/// Construct a `ValidatorSet` from a slice of keypairs.
fn validator_set_from(kps: &[Keypair]) -> ValidatorSet {
    ValidatorSet {
        validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
    }
}

/// Sign the canonical rotation payload with a subset of keypairs.
/// Returns the proof vector expected by `rotate_validators`.
fn sign_rotation(
    new_set: &ValidatorSet,
    epoch: u64,
    signers: &[&Keypair],
) -> Vec<(ed25519_dalek::PublicKey, Signature)> {
    let payload = Bridge::quorum_proof_payload(&[], new_set, epoch)
        .expect("serialization must not fail");
    signers
        .iter()
        .map(|kp| {
            let sig = kp.sign(&payload);
            (kp.public, sig)
        })
        .collect()
}

/// Build a bridge with `n` validators and return `(bridge, keypairs)`.
///
/// The returned keypairs correspond to the initial validator set and can be
/// used to sign rotation proofs for subsequent tests.
fn setup_bridge(n: u8) -> (Bridge, Vec<Keypair>) {
    let kps = det_keypairs(n);
    let initial = validator_set_from(&kps);
    let bridge = Bridge::new(initial);
    (bridge, kps)
}

// ---------------------------------------------------------------------------
// Happy path — valid rotation within bounds
// ---------------------------------------------------------------------------

#[test]
fn test_valid_mid_size_rotation_accepted() {
    let (mut bridge, current_kps) = setup_bridge(5);

    // Rotate to a new set of 4 validators (within bounds).
    let new_kps = det_keypairs(4);
    let new_set = validator_set_from(&new_kps);

    let epoch = 1u64;
    // threshold for 5 validators = (5*2)/3+1 = 4
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&new_set, epoch, &signers);

    bridge
        .rotate_validators(new_set, epoch, proofs)
        .expect("valid mid-size rotation should succeed");
    assert_eq!(bridge.epoch, 1);
}

// ---------------------------------------------------------------------------
// Below MIN_VALIDATORS
// ---------------------------------------------------------------------------

#[test]
fn test_empty_validator_set_rejected() {
    let (mut bridge, current_kps) = setup_bridge(5);

    let empty_set = ValidatorSet {
        validators: vec![],
    };

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&empty_set, epoch, &signers);

    let result = bridge.rotate_validators(empty_set, epoch, proofs);
    assert!(
        result.is_err(),
        "empty validator set must be rejected"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("ValidatorSetTooSmall"),
        "error should mention ValidatorSetTooSmall, got: {msg}"
    );
}

#[test]
fn test_single_validator_set_rejected() {
    let (mut bridge, current_kps) = setup_bridge(5);

    let new_kps = det_keypairs(1);
    let new_set = validator_set_from(&new_kps);

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&new_set, epoch, &signers);

    let result = bridge.rotate_validators(new_set, epoch, proofs);
    assert!(
        result.is_err(),
        "single-validator set must be rejected"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("ValidatorSetTooSmall"),
        "error should mention ValidatorSetTooSmall, got: {msg}"
    );
}

#[test]
fn test_two_validator_set_rejected() {
    let (mut bridge, current_kps) = setup_bridge(5);

    let new_kps = det_keypairs(2);
    let new_set = validator_set_from(&new_kps);

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&new_set, epoch, &signers);

    let result = bridge.rotate_validators(new_set, epoch, proofs);
    assert!(
        result.is_err(),
        "two-validator set must be rejected (below MIN_VALIDATORS=3)"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("ValidatorSetTooSmall"),
        "error should mention ValidatorSetTooSmall, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Exactly MIN_VALIDATORS (boundary)
// ---------------------------------------------------------------------------

#[test]
fn test_min_validator_boundary_accepted() {
    let (mut bridge, current_kps) = setup_bridge(5);

    let new_kps = det_keypairs(MIN_VALIDATORS as u8);
    let new_set = validator_set_from(&new_kps);

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&new_set, epoch, &signers);

    bridge
        .rotate_validators(new_set, epoch, proofs)
        .expect("MIN_VALIDATORS rotation should succeed");
    assert_eq!(bridge.epoch, 1);
}

// ---------------------------------------------------------------------------
// Above MAX_VALIDATORS
// ---------------------------------------------------------------------------

#[test]
fn test_oversized_validator_set_rejected() {
    let (mut bridge, current_kps) = setup_bridge(5);

    // Create a set larger than MAX_VALIDATORS.
    let new_kps = det_keypairs((MAX_VALIDATORS + 1) as u8);
    let new_set = validator_set_from(&new_kps);

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&new_set, epoch, &signers);

    let result = bridge.rotate_validators(new_set, epoch, proofs);
    assert!(
        result.is_err(),
        "oversized validator set must be rejected"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("ValidatorSetTooLarge"),
        "error should mention ValidatorSetTooLarge, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Exactly MAX_VALIDATORS (boundary)
// ---------------------------------------------------------------------------

#[test]
fn test_max_validator_boundary_accepted() {
    let (mut bridge, current_kps) = setup_bridge(5);

    let new_kps = det_keypairs(MAX_VALIDATORS as u8);
    let new_set = validator_set_from(&new_kps);

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&new_set, epoch, &signers);

    bridge
        .rotate_validators(new_set, epoch, proofs)
        .expect("MAX_VALIDATORS rotation should succeed");
    assert_eq!(bridge.epoch, 1);
}

// ---------------------------------------------------------------------------
// Duplicate keys
// ---------------------------------------------------------------------------

#[test]
fn test_duplicate_keys_rejected() {
    let (mut bridge, current_kps) = setup_bridge(5);

    // Create a set where the same key appears twice.
    let kp = det_keypair(10);
    let dup_set = ValidatorSet {
        validators: vec![
            kp.public.to_bytes().to_vec(),
            kp.public.to_bytes().to_vec(),
        ],
    };

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&dup_set, epoch, &signers);

    let result = bridge.rotate_validators(dup_set, epoch, proofs);
    assert!(
        result.is_err(),
        "duplicate-key validator set must be rejected"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("DuplicateValidatorKey"),
        "error should mention DuplicateValidatorKey, got: {msg}"
    );
}

/// A set that is within the size bound *only because of dedup* should still
/// be rejected if the raw list contains duplicates.
#[test]
fn test_duplicate_keys_rejected_even_if_dedup_meets_bound() {
    let (mut bridge, current_kps) = setup_bridge(5);

    // 3 unique validators, but 4 entries (one duplicated).
    let kp_a = det_keypair(10);
    let kp_b = det_keypair(11);
    let kp_c = det_keypair(12);
    let dup_set = ValidatorSet {
        validators: vec![
            kp_a.public.to_bytes().to_vec(),
            kp_b.public.to_bytes().to_vec(),
            kp_c.public.to_bytes().to_vec(),
            kp_a.public.to_bytes().to_vec(), // duplicate of first
        ],
    };

    let epoch = 1u64;
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&dup_set, epoch, &signers);

    let result = bridge.rotate_validators(dup_set, epoch, proofs);
    assert!(
        result.is_err(),
        "set with duplicates must be rejected even if dedup meets bound"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("DuplicateValidatorKey"),
        "error should mention DuplicateValidatorKey, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Bridge state unchanged on rejected rotation
// ---------------------------------------------------------------------------

#[test]
fn test_bridge_state_unchanged_on_rejected_rotation() {
    let (mut bridge, current_kps) = setup_bridge(5);
    let epoch_before = bridge.epoch;
    let validators_before = bridge.validators.to_bytes_vec();

    // Attempt rotation with empty set — should be rejected.
    let empty_set = ValidatorSet { validators: vec![] };
    let signers: Vec<&Keypair> = current_kps[..4].iter().collect();
    let proofs = sign_rotation(&empty_set, 1, &signers);
    let _ = bridge.rotate_validators(empty_set, 1, proofs);

    assert_eq!(bridge.epoch, epoch_before, "epoch must not advance");
    assert_eq!(
        bridge.validators.to_bytes_vec(),
        validators_before,
        "validator set must remain unchanged"
    );
}

// ---------------------------------------------------------------------------
// Error code stability
// ---------------------------------------------------------------------------

#[test]
fn test_error_code_values() {
    assert_eq!(BridgeError::InvalidWindowSize as u32, 0);
    // These are derived from the discriminant ordering:
    assert_eq!(BridgeError::ValidatorSetTooSmall as u32, 1);
    assert_eq!(BridgeError::ValidatorSetTooLarge as u32, 2);
    assert_eq!(BridgeError::DuplicateValidatorKey as u32, 3);
}
