//! Tests for governance proposal payload binding (issue #1120).
//!
//! Proves that a payload bound at creation can only be executed with the exact
//! same action: a substituted target, a mutated param, or a replay under a
//! different proposal id are all rejected at verify (execute) time.

use super::{bind_payload, verify_payload, PayloadBindingError, ProposalPayload};
use soroban_sdk::{Bytes, Env};

fn payload(env: &Env, target: &[u8], params: &[u8], proposal_id: u64) -> ProposalPayload {
    ProposalPayload {
        target: Bytes::from_slice(env, target),
        params: Bytes::from_slice(env, params),
        proposal_id,
    }
}

#[test]
fn matching_payload_verifies() {
    let env = Env::default();
    let p = payload(&env, b"set_rate", b"\x00\x00\x00\x05", 1);

    let bound = bind_payload(&env, &p);
    // Reconstructing the identical action at execution time must verify.
    let at_exec = payload(&env, b"set_rate", b"\x00\x00\x00\x05", 1);
    assert_eq!(verify_payload(&env, &bound, &at_exec), Ok(()));
}

#[test]
fn mutated_param_is_rejected() {
    let env = Env::default();
    let bound = bind_payload(&env, &payload(&env, b"set_rate", b"\x00\x00\x00\x05", 1));

    // Same target/id, but the param value was changed at execution time.
    let substituted = payload(&env, b"set_rate", b"\x00\x00\x00\x06", 1);
    assert_eq!(
        verify_payload(&env, &bound, &substituted),
        Err(PayloadBindingError::PayloadMismatch)
    );
}

#[test]
fn substituted_target_is_rejected() {
    let env = Env::default();
    let bound = bind_payload(&env, &payload(&env, b"set_rate", b"\x00\x00\x00\x05", 1));

    // Voters approved `set_rate`; executor tries to run `drain_treasury`.
    let substituted = payload(&env, b"drain_treasury", b"\x00\x00\x00\x05", 1);
    assert_eq!(
        verify_payload(&env, &bound, &substituted),
        Err(PayloadBindingError::PayloadMismatch)
    );
}

#[test]
fn replay_across_proposal_ids_is_rejected() {
    let env = Env::default();
    // The exact same action, bound under proposal id 1.
    let bound = bind_payload(&env, &payload(&env, b"set_rate", b"\x00\x00\x00\x05", 1));

    // Replaying the approved action under a different proposal id must fail,
    // because proposal_id is folded into the hash.
    let replayed = payload(&env, b"set_rate", b"\x00\x00\x00\x05", 2);
    assert_eq!(
        verify_payload(&env, &bound, &replayed),
        Err(PayloadBindingError::PayloadMismatch)
    );
}

#[test]
fn hash_is_deterministic() {
    let env = Env::default();
    let a = bind_payload(&env, &payload(&env, b"set_rate", b"\x01\x02", 7));
    let b = bind_payload(&env, &payload(&env, b"set_rate", b"\x01\x02", 7));
    assert_eq!(a, b);
}

#[test]
fn length_prefixing_prevents_field_aliasing() {
    let env = Env::default();
    // Without length-prefixing these two distinct actions would share a
    // preimage (target+params concatenate to the same bytes). The 4-byte length
    // prefix on each field must make their hashes differ.
    let h1 = bind_payload(&env, &payload(&env, b"ab", b"", 1));
    let h2 = bind_payload(&env, &payload(&env, b"a", b"b", 1));
    assert_ne!(h1, h2);
}

#[test]
fn distinct_actions_under_same_id_differ() {
    let env = Env::default();
    let h1 = bind_payload(&env, &payload(&env, b"set_rate", b"\x00", 1));
    let h2 = bind_payload(&env, &payload(&env, b"set_rate", b"\x01", 1));
    assert_ne!(h1, h2);
}
