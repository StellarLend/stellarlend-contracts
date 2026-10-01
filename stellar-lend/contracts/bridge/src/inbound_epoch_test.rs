//! Tests for `Bridge::validate_inbound_epoch` — the active-epoch-only
//! inbound message guard (#1147).
//!
//! `validate_inbound_epoch` must accept only the bridge's currently active
//! epoch, optionally extended by [`crate::INBOUND_EPOCH_TOLERANCE`]. Far-future
//! signed epochs must be rejected so that an attacker cannot pre-collect a
//! message valid under a not-yet-rotated validator set and later replay it
//! once that future epoch actually arrives.
//!
//! These tests drive the *deployed contract* through its generated client and
//! advance the epoch through the contract's own epoch-maintenance entry point
//! (`set_epoch`, which only ever moves the epoch forward — exactly what a
//! quorum-proof rotation does). This keeps the suite free of Rust-native
//! types (the wasm target compiles this crate `no_std`) while preserving the
//! original coverage matrix.
//!
//! # Coverage matrix
//!
//! | Scenario | Outcome |
//! |---|---|
//! | `signed_epoch = epoch` (zero-mode) | **Accepted** |
//! | `signed_epoch = epoch` after an epoch advance | **Accepted** |
//! | `signed_epoch < epoch` | **Rejected** — retired validator set |
//! | `signed_epoch = epoch + 1` | **Rejected** — not-yet-active |
//! | `signed_epoch >> epoch` (e.g. `+10⁹`) | **Rejected** — not-yet-active |
//! | `signed_epoch = u64::MAX` | **Rejected** — not-yet-active (no panic) |
//! | `INBOUND_EPOCH_TOLERANCE == 0` is observably enforced | Strict equality |

mod inbound_epoch_tests {
    use crate::{Bridge, BridgeClient, INBOUND_EPOCH_TOLERANCE};
    use soroban_sdk::{Bytes, BytesN, Env, Vec};

    /// Deploy a fresh bridge and advance it to `target` through the
    /// contract's own forward-only epoch entry point.
    fn bridge_at_epoch(env: &Env, target: u64) -> BridgeClient<'static> {
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(env, &contract_id);
        let validators: Vec<BytesN<32>> = Vec::new(env);
        client.initialize(&validators, &Bytes::new(env));
        for _ in 0..target {
            client.advance_epoch();
        }
        assert_eq!(client.get_epoch(), target, "bridge epoch must match target");
        client
    }

    // ── Lower-bound tests: past epochs are rejected ────────────────────

    /// After an epoch advance, a `signed_epoch` lower than the current
    /// epoch must be rejected (a *retired* validator set has no authority
    /// over inbound messages on this bridge).
    #[test]
    fn past_epoch_is_rejected_after_rotation() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 1);

        let err = client.try_validate_inbound_epoch(&0u64);
        assert!(
            matches!(err, Err(Ok(crate::BridgeError::RetiredEpoch))),
            "past epoch must be rejected with RetiredEpoch, got: {err:?}"
        );
    }

    /// Past rejection should hold at large epochs (verifies behavior is
    /// uniform across the full epoch range, not just at zero).
    #[test]
    fn past_epoch_is_rejected_at_large_epoch() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 50);

        for past in [49u64, 25, 1, 0] {
            let err = client.try_validate_inbound_epoch(&past);
            assert!(
                matches!(err, Err(Ok(crate::BridgeError::RetiredEpoch))),
                "past={past} must be rejected with RetiredEpoch, got: {err:?}"
            );
        }
    }

    // ── Equality tests: the active epoch is accepted ───────────────────

    /// A freshly-initialised bridge sits at epoch 0; `signed_epoch = 0` is
    /// the active epoch and must be accepted.
    #[test]
    fn current_epoch_accepted_at_creation() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 0);
        assert_eq!(client.get_epoch(), 0);
        let res = client.try_validate_inbound_epoch(&0u64);
        assert!(
            matches!(res, Ok(Ok(()))),
            "epoch 0 must be accepted at the bridge's initial epoch"
        );
    }

    /// After one epoch advance, the bridge sits at epoch 1;
    /// `signed_epoch = 1` must be accepted (this is the active epoch).
    #[test]
    fn current_epoch_accepted_after_rotation() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 1);
        let res = client.try_validate_inbound_epoch(&1u64);
        assert!(
            matches!(res, Ok(Ok(()))),
            "current epoch 1 must be accepted after rotation"
        );
    }

    /// At a large non-zero epoch, equality is still accepted.
    #[test]
    fn current_epoch_accepted_at_large_epoch() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 50);
        let res = client.try_validate_inbound_epoch(&50u64);
        assert!(
            matches!(res, Ok(Ok(()))),
            "epoch 50 (active) must be accepted"
        );
    }

    // ── Upper-bound tests: not-yet-active epochs are rejected ──────────

    /// `signed_epoch = epoch + 1` is the smallest "future" claim and must
    /// be rejected — it points at a validator set the bridge has not yet
    /// rotated into.
    #[test]
    fn one_above_current_is_rejected() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 1);

        let err = client.try_validate_inbound_epoch(&2u64);
        assert!(
            matches!(err, Err(Ok(crate::BridgeError::InvalidEpoch))),
            "epoch 2 must be rejected as not-yet-active, got: {err:?}"
        );
    }

    /// A vastly-future epoch is rejected with the same error.
    #[test]
    fn far_future_is_rejected() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 1);

        let err = client.try_validate_inbound_epoch(&1_000_000u64);
        assert!(
            matches!(err, Err(Ok(crate::BridgeError::InvalidEpoch))),
            "far-future epoch must be rejected, got: {err:?}"
        );
    }

    /// At a large epoch the same rejection holds — there's no implicit
    /// rollover the further we go.
    #[test]
    fn far_future_is_rejected_at_large_epoch() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 50);

        for too_far in [51u64, 100, 10_000, u64::MAX] {
            let err = client.try_validate_inbound_epoch(&too_far);
            assert!(
                matches!(err, Err(Ok(crate::BridgeError::InvalidEpoch))),
                "too_far={too_far} must be rejected as not-yet-active, got: {err:?}"
            );
        }
    }

    /// `signed_epoch = u64::MAX` must not panic and must be rejected.
    /// This exercises the `saturating_add` overflow path safely.
    #[test]
    fn u64_max_does_not_panic_and_is_rejected() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 50);

        let err = client.try_validate_inbound_epoch(&u64::MAX);
        assert!(
            matches!(err, Err(Ok(crate::BridgeError::InvalidEpoch))),
            "u64::MAX must be rejected, got: {err:?}"
        );
        // Control flow returning here indicates no panic.
    }

    // ── Tolerance-constant observability ───────────────────────────────

    /// The constant is observed to be `0` at test time, locking in the
    /// strict-equality behaviour that the rest of this file relies on.
    /// Changing the constant without re-justifying each test is a
    /// security-sensitive regression.
    #[test]
    fn tolerance_constant_observes_zero() {
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

    // ── Combined regression: past + current + future at the same epoch ─

    /// A single bridge at epoch 5 must reject every other epoch (0, 4, 6,
    /// far-future, u64::MAX) and only accept the active epoch itself.
    #[test]
    fn only_active_epoch_accepted_at_epoch_5() {
        let env = Env::default();
        let client = bridge_at_epoch(&env, 5);
        assert_eq!(client.get_epoch(), 5);

        // Active epoch is accepted.
        let res = client.try_validate_inbound_epoch(&5u64);
        assert!(matches!(res, Ok(Ok(()))), "active epoch 5 must be accepted");

        // Everything else is rejected with the correct error category.
        for &bad in &[0u64, 1, 2, 3, 4, 6, 7, 100, 1_000_000, u64::MAX] {
            let err = client.try_validate_inbound_epoch(&bad);
            let is_past = bad < 5;
            let expected = if is_past {
                crate::BridgeError::RetiredEpoch
            } else {
                crate::BridgeError::InvalidEpoch
            };
            assert!(
                matches!(err, Err(Ok(e)) if e == expected),
                "bad={bad}: wrong error category, got: {err:?}"
            );
        }
    }
}
