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
//! | `signed_epoch = self.epoch` (zero-mode) | **Accepted** |
//! | `signed_epoch = self.epoch` after a rotation | **Accepted** |
//! | `signed_epoch < self.epoch` | **Rejected** — retired validator set |
//! | `signed_epoch = self.epoch + 1` | **Rejected** — not-yet-active |
//! | `signed_epoch >> self.epoch` (e.g. `+10⁹`) | **Rejected** — not-yet-active |
//! | `signed_epoch = u64::MAX` | **Rejected** — not-yet-active (no panic) |
//! | Error message references both numbers and tolerance | Diagnostic context preserved |
//! | `INBOUND_EPOCH_TOLERANCE == 0` is observably enforced | Strict equality |

#[cfg(test)]
mod inbound_epoch_tests {
    use crate::{Bridge, ValidatorSet, INBOUND_EPOCH_TOLERANCE};
    use ed25519_dalek::{Keypair, SecretKey, Signature, Signer};

    // ── Deterministic keypair factory (mirrors `rotation_test.rs`) ──────

    fn det_keypair(index: u8) -> Keypair {
        let mut seed = [0u8; 32];
        seed[0] = index.wrapping_add(1);
        for i in 1..32 {
            seed[i] = index.wrapping_mul(7).wrapping_add(i as u8);
        }
        let secret = SecretKey::from_bytes(&seed).expect("valid secret key");
        let public: ed25519_dalek::PublicKey = (&secret).into();
        let mut combined = [0u8; 64];
        combined[..32].copy_from_slice(&seed);
        combined[32..].copy_from_slice(public.as_bytes());
        Keypair::from_bytes(&combined).expect("valid keypair from seed")
    }

    fn det_keypairs(start: u8, end: u8) -> Vec<Keypair> {
        (start..end).map(det_keypair).collect()
    }

    fn validator_set_from(kps: &[Keypair]) -> ValidatorSet {
        ValidatorSet {
            validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
        }
    }

    fn sign_rotation(
        new_set: &ValidatorSet,
        epoch: u64,
        signers: &[&Keypair],
    ) -> Vec<(ed25519_dalek::PublicKey, Signature)> {
        let payload =
            Bridge::quorum_proof_payload(&[], new_set, epoch).expect("serialization must not fail");
        signers
            .iter()
            .map(|kp| (kp.public, kp.sign(&payload)))
            .collect()
    }

    // ── Helpers to advance the bridge to a known epoch ─────────────────

    /// Advance the bridge through `target` valid rotations so its `epoch`
    /// equals `target` afterwards.
    ///
    /// Each step uses a fresh, non-overlapping deterministic validator set
    /// so the rotated-out keypairs can never accidentally match the
    /// rotated-in ones. The current-signer pool is updated alongside
    /// `bridge.validators` after every rotation so `verify_quorum_proof`
    /// never receives a set of signers outside the current validator set.
    fn bridge_at_epoch(target: u64) -> Bridge {
        // Start at index 10 so tests don't collide with the `det_keypair(0)`
        // seeds used elsewhere in the suite (e.g. `epoch_monotonicity_proptest`).
        let mut current_kps = det_keypairs(10, 13);
        let mut bridge = Bridge::new(validator_set_from(&current_kps));

        for step in 0..target {
            let next_start: u8 = (step as u8).wrapping_mul(3).wrapping_add(20);
            let next_kps = det_keypairs(next_start, next_start.wrapping_add(3));
            let next = validator_set_from(&next_kps);
            let signers: Vec<&Keypair> = current_kps.iter().collect();
            let proofs = sign_rotation(&next, bridge.epoch + 1, &signers);
            bridge
                .rotate_validators(next, bridge.epoch + 1, proofs)
                .unwrap_or_else(|e| panic!("warm-up rotation {step} failed: {e}"));
            // Sync the signer pool to the bridge's new validator set so the
            // next iteration's rotation proof is signed by members of the
            // current validator set.
            current_kps = next_kps;
        }

        assert_eq!(
            bridge.epoch, target,
            "bridge.epoch must match the requested target"
        );
        bridge
    }

    // ── Lower-bound tests: past epochs are rejected ────────────────────

    /// After a successful rotation, a `signed_epoch` lower than the
    /// current epoch must be rejected (a *retired* validator set has no
    /// authority over inbound messages on this bridge).
    #[test]
    fn past_epoch_is_rejected_after_rotation() {
        let bridge = bridge_at_epoch(1);
        assert_eq!(bridge.epoch, 1);

        let err = bridge
            .validate_inbound_epoch(0)
            .expect_err("past epoch must be rejected once a rotation has occurred");
        let msg = err.to_string();
        assert!(
            msg.contains("retired validator set"),
            "error should reference retired validator set, got: {msg}"
        );
    }

    /// Past rejection should hold at large epochs (verifies behavior is
    /// uniform across the full epoch range, not just at zero).
    #[test]
    fn past_epoch_is_rejected_at_large_epoch() {
        let bridge = bridge_at_epoch(50);

        for past in [49u64, 25, 1, 0] {
            let err = bridge
                .validate_inbound_epoch(past)
                .unwrap_or_else(|e| panic!("past epoch {past} must be rejected, got: {e}"));
            assert!(
                err.to_string().contains("retired validator set"),
                "past={past}, got: {err}"
            );
        }
    }

    // ── Equality tests: the active epoch is accepted ───────────────────

    /// A freshly-constructed bridge sits at epoch 0; `signed_epoch = 0` is
    /// the active epoch and must be accepted.
    #[test]
    fn current_epoch_accepted_at_creation() {
        let bridge = Bridge::new(validator_set_from(&det_keypairs(50, 53)));
        assert_eq!(bridge.epoch, 0);
        bridge
            .validate_inbound_epoch(0)
            .expect("epoch 0 must be accepted at the bridge's initial epoch");
    }

    /// After one rotation, the bridge sits at epoch 1; `signed_epoch = 1`
    /// must be accepted (this is the active epoch).
    #[test]
    fn current_epoch_accepted_after_rotation() {
        let bridge = bridge_at_epoch(1);
        assert_eq!(bridge.epoch, 1);
        bridge
            .validate_inbound_epoch(1)
            .expect("current epoch 1 must be accepted after rotation");
    }

    /// At a large non-zero epoch, equality is still accepted.
    #[test]
    fn current_epoch_accepted_at_large_epoch() {
        let bridge = bridge_at_epoch(50);
        bridge
            .validate_inbound_epoch(50)
            .expect("epoch 50 (active) must be accepted");
    }

    // ── Upper-bound tests: not-yet-active epochs are rejected ──────────

    /// `signed_epoch = self.epoch + 1` is the smallest "future" claim and
    /// must be rejected — it points at a validator set the bridge has not
    /// yet rotated into.
    #[test]
    fn one_above_current_is_rejected() {
        let bridge = bridge_at_epoch(1);
        let err = bridge
            .validate_inbound_epoch(2)
            .expect_err("epoch 2 must be rejected as not-yet-active");
        let msg = err.to_string();
        assert!(
            msg.contains("not-yet-active"),
            "error should reference not-yet-active validator set, got: {msg}"
        );
    }

    /// A vastly-future epoch is rejected with the same diagnostic.
    #[test]
    fn far_future_is_rejected() {
        let bridge = bridge_at_epoch(1);
        let err = bridge
            .validate_inbound_epoch(1_000_000)
            .expect_err("far-future epoch must be rejected");
        assert!(err.to_string().contains("not-yet-active"));
    }

    /// At a large epoch the same rejection holds — there's no implicit
    /// rollover the further we go.
    #[test]
    fn far_future_is_rejected_at_large_epoch() {
        let bridge = bridge_at_epoch(50);
        for too_far in [51u64, 100, 10_000, u64::MAX] {
            let err = bridge
                .validate_inbound_epoch(too_far)
                .unwrap_or_else(|e| panic!("too-far epoch {too_far} must be rejected, got: {e}"));
            assert!(
                err.to_string().contains("not-yet-active"),
                "too_far={too_far}, got: {err}"
            );
        }
    }

    /// `signed_epoch = u64::MAX` must not panic and must be rejected.
    /// This exercises the `saturating_add` overflow path safely.
    #[test]
    fn u64_max_does_not_panic_and_is_rejected() {
        let bridge = bridge_at_epoch(50);
        bridge
            .validate_inbound_epoch(u64::MAX)
            .expect_err("u64::MAX must be rejected");
        // Control flow returning here indicates no panic.
    }

    // ── Diagnostic-context tests ───────────────────────────────────────

    /// Substring matching on the error string is deliberate: the exact
    /// `anyhow!` templates in `lib.rs` are part of the contract's
    /// operator-visible error surface, and the test pins them so a
    /// reformat here is a deliberate reviewer-visible decision rather
    /// than an accident.

    /// Both the rejected-not-yet-active and rejected-retired error paths
    /// embed `signed_epoch=N` and `self.epoch=M` so operators can triage
    /// either side of the bound.
    #[test]
    fn future_rejection_message_is_diagnostically_complete() {
        let bridge = bridge_at_epoch(1);
        let err = bridge
            .validate_inbound_epoch(99)
            .expect_err("must reject far-future epoch");
        let msg = err.to_string();
        assert!(msg.contains("signed_epoch=99"), "got: {msg}");
        assert!(msg.contains("self.epoch=1"), "got: {msg}");
        assert!(msg.contains("not-yet-active"), "got: {msg}");
    }

    /// Past-rejection error includes the diagnostic numbers too.
    #[test]
    fn past_rejection_message_is_diagnostically_complete() {
        let bridge = bridge_at_epoch(7);
        let err = bridge
            .validate_inbound_epoch(3)
            .expect_err("must reject past epoch");
        let msg = err.to_string();
        assert!(msg.contains("signed_epoch=3"), "got: {msg}");
        assert!(msg.contains("self.epoch=7"), "got: {msg}");
        assert!(msg.contains("retired validator set"), "got: {msg}");
    }

    // ── Tolerance-constant observability ───────────────────────────────

    /// The constant is observed to be `0` at test time, locking in the
    /// strict-equality behaviour that the rest of this file relies on.
    /// Changing the constant without re-justifying each test is a
    /// security-sensitive regression.
    #[test]
    fn tolerance_constant_observes_zero() {
        assert_eq!(
            INBOUND_EPOCH_TOLERANCE, 0,
            "INBOUND_EPOCH_TOLERANCE must be 0 (strict-epoch equality) to defend \
             against not-yet-active validator-set replay (#1147). Raising it is a \
             security-sensitive change that must be re-justified and re-tested."
        );
    }

    // ── Combined regression: past + current + future at the same epoch ─

    /// A single bridge at epoch 5 must reject every other epoch (0, 4, 6,
    /// far-future, u64::MAX) and only accept the active epoch itself.
    #[test]
    fn only_active_epoch_accepted_at_epoch_5() {
        let bridge = bridge_at_epoch(5);
        assert_eq!(bridge.epoch, 5);

        // Active epoch is accepted.
        bridge
            .validate_inbound_epoch(5)
            .expect("active epoch 5 must be accepted");

        // Everything else is rejected.
        for &bad in &[0u64, 1, 2, 3, 4, 6, 7, 100, 1_000_000, u64::MAX] {
            let err = bridge
                .validate_inbound_epoch(bad)
                .unwrap_or_else(|e| panic!("non-active epoch {bad} must be rejected, got: {e}"));
            let msg = err.to_string();
            let is_past = bad < 5;
            let is_future = bad > 5;
            assert!(
                (is_past && msg.contains("retired validator set"))
                    || (is_future && msg.contains("not-yet-active")),
                "bad={bad}: wrong error category, got: {msg}"
            );
        }
    }
}
