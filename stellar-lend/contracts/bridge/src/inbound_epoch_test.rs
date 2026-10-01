//! Tests for `Bridge::validate_inbound_epoch` — the active-epoch-only
//! inbound message guard (#1147).
//!
//! `validate_inbound_epoch` must accept only the bridge's currently active
//! epoch, optionally extended by [`crate::INBOUND_EPOCH_TOLERANCE`]. Far-future
//! signed epochs must be rejected so that an attacker cannot pre-collect a
//! message valid under a not-yet-rotated validator set and later replay it
//! once that future epoch actually arrives.
//!
//! # Coverage matrix
//!
//! | Scenario | Outcome |
//! |---|---|
//! | `signed_epoch = active_epoch` (epoch 0) | **Accepted** |
//! | `signed_epoch = active_epoch` after epoch advance | **Accepted** |
//! | `signed_epoch < active_epoch` | **Rejected** — RetiredEpoch |
//! | `signed_epoch = active_epoch + 1` | **Rejected** — InvalidEpoch |
//! | `signed_epoch >> active_epoch` (e.g. `+10⁹`) | **Rejected** — InvalidEpoch |
//! | `signed_epoch = u64::MAX` | **Rejected** — InvalidEpoch (no panic) |
//! | `INBOUND_EPOCH_TOLERANCE == 0` is observably enforced | Strict equality |

use crate::{Bridge, BridgeClient, BridgeError, INBOUND_EPOCH_TOLERANCE};
use soroban_sdk::{Bytes, BytesN, Env, Vec};

/// Helper to set up a bridge contract instance initialized at `target` epoch.
fn bridge_at_epoch(target: u64) -> (Env, BridgeClient<'static>) {
    let env = Env::default();
    let contract_id = env.register(Bridge, ());
    let client = BridgeClient::new(&env, &contract_id);
    let validators: Vec<BytesN<32>> = Vec::new(&env);
    client.initialize(&validators, &Bytes::new(&env));
    if target > 0 {
        env.as_contract(&contract_id, || {
            Bridge::save_epoch(&env, target);
        });
    }
    (env, client)
}

// ── Equality tests: the active epoch is accepted ───────────────────

/// A freshly-constructed bridge sits at epoch 0; `signed_epoch = 0` is
/// the active epoch and must be accepted.
#[test]
fn current_epoch_accepted_at_creation() {
    let (_env, client) = bridge_at_epoch(0);
    assert_eq!(client.get_epoch(), 0);
    assert_eq!(client.try_validate_inbound_epoch(&0u64), Ok(Ok(())));
}

/// After an epoch advance, the bridge sits at epoch 1; `signed_epoch = 1`
/// must be accepted (this is the active epoch).
#[test]
fn current_epoch_accepted_after_rotation() {
    let (_env, client) = bridge_at_epoch(1);
    assert_eq!(client.get_epoch(), 1);
    assert_eq!(client.try_validate_inbound_epoch(&1u64), Ok(Ok(())));
}

/// At a large non-zero epoch, equality is still accepted.
#[test]
fn current_epoch_accepted_at_large_epoch() {
    let (_env, client) = bridge_at_epoch(50);
    assert_eq!(client.get_epoch(), 50);
    assert_eq!(client.try_validate_inbound_epoch(&50u64), Ok(Ok(())));
}

// ── Lower-bound tests: past epochs are rejected ────────────────────

/// After an epoch advance, a `signed_epoch` lower than the current epoch
/// must be rejected with RetiredEpoch.
#[test]
fn past_epoch_is_rejected_after_rotation() {
    let (_env, client) = bridge_at_epoch(1);
    assert_eq!(
        client.try_validate_inbound_epoch(&0u64),
        Err(Ok(BridgeError::RetiredEpoch))
    );
}

/// Past rejection should hold at large epochs.
#[test]
fn past_epoch_is_rejected_at_large_epoch() {
    let (_env, client) = bridge_at_epoch(50);
    for past in [49u64, 25, 1, 0] {
        assert_eq!(
            client.try_validate_inbound_epoch(&past),
            Err(Ok(BridgeError::RetiredEpoch)),
            "past epoch {past} must be rejected with RetiredEpoch"
        );
    }
}

// ── Upper-bound tests: not-yet-active epochs are rejected ──────────

/// `signed_epoch = active_epoch + 1` is the smallest "future" claim and
/// must be rejected — it points at a validator set the bridge has not
/// yet rotated into.
#[test]
fn one_above_current_is_rejected() {
    let (_env, client) = bridge_at_epoch(1);
    assert_eq!(
        client.try_validate_inbound_epoch(&2u64),
        Err(Ok(BridgeError::InvalidEpoch))
    );
}

/// A vastly-future epoch is rejected with InvalidEpoch.
#[test]
fn far_future_is_rejected() {
    let (_env, client) = bridge_at_epoch(1);
    assert_eq!(
        client.try_validate_inbound_epoch(&1_000_000u64),
        Err(Ok(BridgeError::InvalidEpoch))
    );
}

/// At a large epoch the same rejection holds.
#[test]
fn far_future_is_rejected_at_large_epoch() {
    let (_env, client) = bridge_at_epoch(50);
    for too_far in [51u64, 100, 10_000, u64::MAX] {
        assert_eq!(
            client.try_validate_inbound_epoch(&too_far),
            Err(Ok(BridgeError::InvalidEpoch)),
            "too_far {too_far} must be rejected with InvalidEpoch"
        );
    }
}

/// `signed_epoch = u64::MAX` must not panic and must be rejected.
#[test]
fn u64_max_does_not_panic_and_is_rejected() {
    let (_env, client) = bridge_at_epoch(50);
    assert_eq!(
        client.try_validate_inbound_epoch(&u64::MAX),
        Err(Ok(BridgeError::InvalidEpoch))
    );
}

// ── Tolerance-constant observability ───────────────────────────────

/// The constant is observed to be `0` at test time, locking in the
/// strict-equality behaviour that the rest of this file relies on.
#[test]
fn tolerance_constant_observes_zero() {
    assert_eq!(
        INBOUND_EPOCH_TOLERANCE, 0,
        "INBOUND_EPOCH_TOLERANCE must be 0 (strict-epoch equality) to defend \
         against not-yet-active validator-set replay (#1147)."
    );
}

// ── Combined regression: past + current + future at the same epoch ─

/// A single bridge at epoch 5 must reject every other epoch (0..4, 6..u64::MAX)
/// and only accept the active epoch itself.
#[test]
fn only_active_epoch_accepted_at_epoch_5() {
    let (_env, client) = bridge_at_epoch(5);
    assert_eq!(client.get_epoch(), 5);

    // Active epoch is accepted.
    assert_eq!(client.try_validate_inbound_epoch(&5u64), Ok(Ok(())));

    // Past epochs rejected with RetiredEpoch.
    for &past in &[0u64, 1, 2, 3, 4] {
        assert_eq!(
            client.try_validate_inbound_epoch(&past),
            Err(Ok(BridgeError::RetiredEpoch)),
            "past epoch {past} must be rejected with RetiredEpoch"
        );
    }

    // Future epochs rejected with InvalidEpoch.
    for &future in &[6u64, 7, 100, 1_000_000, u64::MAX] {
        assert_eq!(
            client.try_validate_inbound_epoch(&future),
            Err(Ok(BridgeError::InvalidEpoch)),
            "future epoch {future} must be rejected with InvalidEpoch"
        );
    }
}
