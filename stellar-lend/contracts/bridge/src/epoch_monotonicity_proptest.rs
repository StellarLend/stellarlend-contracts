//! epoch_monotonicity_proptest.rs — Property-based tests for bridge epoch invariants.
//!
//! # Proven properties
//!
//! Over every randomly generated rotation sequence the following invariants hold:
//!
//! 1. **Strict monotonicity** — `bridge.epoch` never decreases between any two
//!    observations.
//! 2. **No-skip rule** — every *successful* rotation advances `bridge.epoch` by
//!    exactly one.
//! 3. **No-regress on rejection** — every *rejected* rotation leaves
//!    `bridge.epoch` unchanged.
//! 4. **Final epoch matches success count** — after a sequence of N successful
//!    rotations the final epoch equals the number of successes (since the bridge
//!    starts at epoch 0 and each success adds exactly one).
//!
//! # Fault-injection categories
//!
//! The [`RotationAttempt`] enum encodes the random space of both valid and
//! deliberately invalid inputs:
//!
//! | Variant | Expected outcome |
//! |---------|-----------------|
//! | `Valid` | Rotation accepted; epoch + 1 |
//! | `WrongEpochSame` | Rejected (epoch == current) |
//! | `WrongEpochSkip` | Rejected (epoch == current + 2) |
//! | `WrongEpochStale` | Rejected (epoch == current - 1 or 0) |
//! | `InsufficientQuorum` | Rejected (< threshold signatures) |
//! | `EmptyProofs` | Rejected (no signatures at all) |
//! | `OutsideSigner` | Rejected (signer not in current set) |
//! | `WrongPayloadSig` | Rejected (signature over wrong payload) |

#[cfg(test)]
mod tests {
    use crate::{Bridge, ValidatorSet};
    use bincode;
    use ed25519_dalek::{Keypair, Signature, Signer};
    use proptest::prelude::*;

    // ── Deterministic keypair factory ─────────────────────────────────────────

    /// Build a deterministic [`Keypair`] from a single seed byte.
    ///
    /// The seed is expanded to 32 bytes using a simple mixing function so that
    /// distinct index values always produce distinct keys.  This makes tests
    /// fully reproducible without an OS RNG while keeping each key unique.
    fn det_keypair(index: u8) -> Keypair {
        let mut seed = [0u8; 32];
        seed[0] = index.wrapping_add(1);
        for i in 1..32 {
            seed[i] = index.wrapping_mul(7).wrapping_add(i as u8);
        }
        use ed25519_dalek::SecretKey;
        let secret = SecretKey::from_bytes(&seed).expect("valid secret key");
        let public: ed25519_dalek::PublicKey = (&secret).into();
        let mut combined = [0u8; 64];
        combined[..32].copy_from_slice(&seed);
        combined[32..].copy_from_slice(public.as_bytes());
        Keypair::from_bytes(&combined).expect("valid keypair")
    }

    /// Build `n` deterministic keypairs with indices in `[base, base + n)`.
    ///
    /// Using a non-zero `base` isolates each round's validator set from all
    /// others so there is never an accidental key overlap between sets.
    fn det_keypairs_from(base: u8, n: u8) -> Vec<Keypair> {
        (0..n).map(|i| det_keypair(base.wrapping_add(i))).collect()
    }

    /// Construct a [`ValidatorSet`] from a slice of keypairs.
    fn vs(kps: &[Keypair]) -> ValidatorSet {
        ValidatorSet {
            validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
        }
    }

    /// Sign the canonical rotation payload `(new_set_bytes, epoch)` with each
    /// keypair in `signers` and return the proof vector expected by
    /// [`Bridge::rotate_validators`].
    fn sign_rotation(
        new_set: &ValidatorSet,
        epoch: u64,
        signers: &[&Keypair],
    ) -> Vec<(ed25519_dalek::PublicKey, Signature)> {
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch))
            .expect("serialization must not fail");
        signers
            .iter()
            .map(|kp| (kp.public, kp.sign(&payload)))
            .collect()
    }

    // ── Rotation-attempt model ────────────────────────────────────────────────

    /// One entry in a randomly generated rotation sequence.
    ///
    /// Each variant encodes a distinct fault-injection category.  The proptest
    /// strategy ([`arb_attempt`]) generates these uniformly so that both valid
    /// and every class of invalid input are exercised in the same sequence.
    #[derive(Debug, Clone)]
    enum RotationAttempt {
        /// A fully valid rotation: correct epoch, quorum met, correct payload.
        Valid,
        /// Same epoch as current — must be rejected.
        WrongEpochSame,
        /// Current + 2 — skips an epoch — must be rejected.
        WrongEpochSkip,
        /// Current - 1 (or 0 when at epoch 0) — stale epoch — must be rejected.
        WrongEpochStale,
        /// Correct epoch but only (threshold - 1) signatures — must be rejected.
        InsufficientQuorum,
        /// Correct epoch, zero signatures — must be rejected.
        EmptyProofs,
        /// One signer is a keypair not present in the current validator set.
        OutsideSigner,
        /// Signatures are over the wrong payload (different epoch number) —
        /// ed25519 verification will fail — must be rejected.
        WrongPayloadSig,
    }

    /// Proptest strategy that produces a [`RotationAttempt`] uniformly at random.
    fn arb_attempt() -> impl Strategy<Value = RotationAttempt> {
        prop_oneof![
            Just(RotationAttempt::Valid),
            Just(RotationAttempt::WrongEpochSame),
            Just(RotationAttempt::WrongEpochSkip),
            Just(RotationAttempt::WrongEpochStale),
            Just(RotationAttempt::InsufficientQuorum),
            Just(RotationAttempt::EmptyProofs),
            Just(RotationAttempt::OutsideSigner),
            Just(RotationAttempt::WrongPayloadSig),
        ]
    }

    // ── Sequence executor ─────────────────────────────────────────────────────

    /// Validator-set slot: a pool of pre-generated keypairs, one per rotation
    /// round.  Index `r` holds the keypairs active *during* round `r`.
    ///
    /// We pre-generate 60 sets (rounds 0..60) so that any sequence up to 60
    /// steps long can look up both the current and the next validator set
    /// without re-deriving keys.  Sets are non-overlapping: set `r` uses base
    /// offset `r * 4` so no two rounds share a keypair.
    const MAX_ROUNDS: usize = 60;
    const SET_SIZE: u8 = 4; // 4 validators → threshold = 3

    /// Pre-build all keypair pools for the test run.
    ///
    /// Returns a `Vec` of length `MAX_ROUNDS + 1`; entry `i` is the keypair
    /// pool for validator set `i`.  Set 0 is the initial (genesis) set.
    fn build_key_pools() -> Vec<Vec<Keypair>> {
        (0..=MAX_ROUNDS)
            .map(|r| det_keypairs_from((r as u8).wrapping_mul(SET_SIZE), SET_SIZE))
            .collect()
    }

    /// Execute one [`RotationAttempt`] against `bridge` and return `true` iff
    /// the rotation succeeded (epoch advanced by one).
    ///
    /// `round` is the zero-based index of this attempt in the sequence.
    /// It is used to select the "next" validator set from `pools` and to derive
    /// an outsider keypair that is never part of any in-use set.
    ///
    /// The function does NOT assert invariants itself — all assertions are
    /// performed by the caller so that failure messages carry full sequence
    /// context.
    fn execute_attempt(
        bridge: &mut Bridge,
        attempt: &RotationAttempt,
        round: usize,
        pools: &[Vec<Keypair>],
    ) -> bool {
        let current_epoch = bridge.epoch;
        // The "next" set is always pools[round + 1].
        let next_set = vs(&pools[round + 1]);
        // The current set keypairs (for signing).
        let current_kps = &pools[round];
        let threshold = bridge.validators.threshold();

        let result = match attempt {
            RotationAttempt::Valid => {
                // Correct epoch, full quorum of current-set signers.
                let signers: Vec<&Keypair> = current_kps[..threshold].iter().collect();
                let proofs = sign_rotation(&next_set, current_epoch + 1, &signers);
                bridge.rotate_validators(next_set, current_epoch + 1, proofs)
            }

            RotationAttempt::WrongEpochSame => {
                // epoch == current — off-by-zero.
                let signers: Vec<&Keypair> = current_kps[..threshold].iter().collect();
                let proofs = sign_rotation(&next_set, current_epoch, &signers);
                bridge.rotate_validators(next_set, current_epoch, proofs)
            }

            RotationAttempt::WrongEpochSkip => {
                // epoch == current + 2 — jumps over one.
                let bad_epoch = current_epoch + 2;
                let signers: Vec<&Keypair> = current_kps[..threshold].iter().collect();
                let proofs = sign_rotation(&next_set, bad_epoch, &signers);
                bridge.rotate_validators(next_set, bad_epoch, proofs)
            }

            RotationAttempt::WrongEpochStale => {
                // epoch < current (saturating to 0).
                let bad_epoch = current_epoch.saturating_sub(1);
                let signers: Vec<&Keypair> = current_kps[..threshold].iter().collect();
                let proofs = sign_rotation(&next_set, bad_epoch, &signers);
                bridge.rotate_validators(next_set, bad_epoch, proofs)
            }

            RotationAttempt::InsufficientQuorum => {
                // threshold - 1 signatures — one short.
                let below = if threshold > 0 { threshold - 1 } else { 0 };
                let signers: Vec<&Keypair> = current_kps[..below].iter().collect();
                let proofs = sign_rotation(&next_set, current_epoch + 1, &signers);
                bridge.rotate_validators(next_set, current_epoch + 1, proofs)
            }

            RotationAttempt::EmptyProofs => {
                bridge.rotate_validators(next_set, current_epoch + 1, vec![])
            }

            RotationAttempt::OutsideSigner => {
                // Construct a quorum that includes one outsider key.
                // Use index 200 + round as the outsider base — far outside
                // any pool index that could appear in pools (max = 60 * 4 = 240;
                // we use wrapping to stay within u8, but the pattern is distinct
                // enough for the sizes used here).
                let outsider = det_keypair(200u8.wrapping_add(round as u8));
                // mix outsider in: (threshold-1) valid + 1 outsider
                let valid_count = if threshold > 1 { threshold - 1 } else { 0 };
                let mut signers: Vec<&Keypair> = current_kps[..valid_count].iter().collect();
                signers.push(&outsider);
                let proofs = sign_rotation(&next_set, current_epoch + 1, &signers);
                bridge.rotate_validators(next_set, current_epoch + 1, proofs)
            }

            RotationAttempt::WrongPayloadSig => {
                // Sign over the wrong epoch (current + 99) so ed25519 verify fails.
                let wrong_epoch = current_epoch.wrapping_add(99);
                let signers: Vec<&Keypair> = current_kps[..threshold].iter().collect();
                // Produce proofs over the wrong payload.
                let bad_proofs = sign_rotation(&next_set, wrong_epoch, &signers);
                // But submit with the correct epoch number, so the epoch check
                // passes and the sig check is reached.
                bridge.rotate_validators(next_set, current_epoch + 1, bad_proofs)
            }
        };

        result.is_ok()
    }

    // ── Property tests ────────────────────────────────────────────────────────

    proptest! {
        /// **Property 1 & 2 & 3 & 4** — full invariant suite over random sequences.
        ///
        /// For every randomly generated sequence of up to 20 rotation attempts:
        ///
        /// - P1 (strict monotonicity): `epoch` never decreases between consecutive
        ///   observations.
        /// - P2 (no-skip rule): every successful rotation advances `epoch` by
        ///   exactly one.
        /// - P3 (no-regress on rejection): every rejected rotation leaves `epoch`
        ///   unchanged.
        /// - P4 (final epoch == success count): `bridge.epoch` equals the number
        ///   of successful rotations at the end of the sequence (since initial
        ///   epoch is 0 and each success adds exactly one).
        #[test]
        fn prop_epoch_monotonic_and_no_skip(
            attempts in proptest::collection::vec(arb_attempt(), 1..=20)
        ) {
            let pools = build_key_pools();
            let initial_set = vs(&pools[0]);
            let mut bridge = Bridge::new(initial_set);

            let mut prev_epoch: u64 = 0;
            let mut success_count: u64 = 0;
            // Track the current round index into pools (only advances on success).
            let mut round: usize = 0;

            for (step, attempt) in attempts.iter().enumerate() {
                let epoch_before = bridge.epoch;

                // Guard: stop if we've exhausted pre-generated pools.
                if round + 1 >= pools.len() {
                    break;
                }

                let succeeded = execute_attempt(&mut bridge, attempt, round, &pools);
                let epoch_after = bridge.epoch;

                // P1: epoch must never decrease.
                prop_assert!(
                    epoch_after >= prev_epoch,
                    "step {} ({:?}): epoch regressed from {} to {}",
                    step,
                    attempt,
                    prev_epoch,
                    epoch_after
                );

                if succeeded {
                    // P2: successful rotation advances epoch by exactly one.
                    prop_assert_eq!(
                        epoch_after,
                        epoch_before + 1,
                        "step {} ({:?}): successful rotation must advance epoch by 1 \
                         (was {}, now {})",
                        step,
                        attempt,
                        epoch_before,
                        epoch_after
                    );
                    success_count += 1;
                    round += 1;
                } else {
                    // P3: rejected rotation must leave epoch unchanged.
                    prop_assert_eq!(
                        epoch_after,
                        epoch_before,
                        "step {} ({:?}): rejected rotation must not change epoch \
                         (was {}, now {})",
                        step,
                        attempt,
                        epoch_before,
                        epoch_after
                    );
                }

                prev_epoch = epoch_after;
            }

            // P4: final epoch equals the total number of successful rotations.
            prop_assert_eq!(
                bridge.epoch,
                success_count,
                "final epoch {} must equal success count {}",
                bridge.epoch,
                success_count
            );
        }

        /// **Large-epoch regression test** — verify invariants still hold when
        /// the bridge has been rotated many times and the epoch number is large.
        ///
        /// Performs up to 30 consecutive valid rotations then fires a random
        /// sequence of 10 attempts and re-checks all four properties.
        #[test]
        fn prop_invariants_hold_at_large_epoch(
            fault_attempts in proptest::collection::vec(arb_attempt(), 1..=10)
        ) {
            let pools = build_key_pools();
            let initial_set = vs(&pools[0]);
            let mut bridge = Bridge::new(initial_set);

            // Phase 1: advance to a large epoch via valid rotations.
            let warm_up_rounds = 30usize;
            for r in 0..warm_up_rounds {
                let next_set = vs(&pools[r + 1]);
                let threshold = bridge.validators.threshold();
                let signers: Vec<&Keypair> = pools[r][..threshold].iter().collect();
                let proofs = sign_rotation(&next_set, bridge.epoch + 1, &signers);
                bridge
                    .rotate_validators(next_set, bridge.epoch + 1, proofs)
                    .expect("warm-up rotation must succeed");
            }

            let large_epoch = bridge.epoch;
            prop_assert_eq!(
                large_epoch, warm_up_rounds as u64,
                "after {} warm-up rotations epoch must be {}",
                warm_up_rounds,
                warm_up_rounds
            );

            // Phase 2: apply random fault attempts and verify invariants.
            let mut prev_epoch = bridge.epoch;
            let mut round = warm_up_rounds;

            for (step, attempt) in fault_attempts.iter().enumerate() {
                if round + 1 >= pools.len() {
                    break;
                }

                let epoch_before = bridge.epoch;
                let succeeded = execute_attempt(&mut bridge, attempt, round, &pools);
                let epoch_after = bridge.epoch;

                // P1
                prop_assert!(
                    epoch_after >= prev_epoch,
                    "large-epoch step {} ({:?}): epoch regressed {}→{}",
                    step,
                    attempt,
                    prev_epoch,
                    epoch_after
                );

                if succeeded {
                    // P2
                    prop_assert_eq!(
                        epoch_after,
                        epoch_before + 1,
                        "large-epoch step {}: success must advance by 1 ({}→{})",
                        step,
                        epoch_before,
                        epoch_after
                    );
                    round += 1;
                } else {
                    // P3
                    prop_assert_eq!(
                        epoch_after,
                        epoch_before,
                        "large-epoch step {}: rejection must not change epoch ({}→{})",
                        step,
                        epoch_before,
                        epoch_after
                    );
                }

                prev_epoch = epoch_after;
            }
        }

        /// **Repeated-epoch attack** — an adversary replays the *same* epoch
        /// number many times; epoch must never advance.
        ///
        /// Generates between 1 and 15 repeated rotation attempts all using
        /// `epoch = 0` (the stale-or-same epoch) and verifies the bridge
        /// remains at epoch 0 throughout.
        #[test]
        fn prop_repeated_same_epoch_never_advances(repeat_count in 1usize..=15) {
            let pools = build_key_pools();
            let initial_set = vs(&pools[0]);
            let mut bridge = Bridge::new(initial_set);

            let threshold = bridge.validators.threshold();

            for _ in 0..repeat_count {
                let next_set = vs(&pools[1]);
                let signers: Vec<&Keypair> = pools[0][..threshold].iter().collect();
                // Always submit epoch 0 — same as current, must always be rejected.
                let proofs = sign_rotation(&next_set, 0, &signers);
                let result = bridge.rotate_validators(next_set, 0, proofs);

                prop_assert!(result.is_err(), "same-epoch attempt must be rejected");
                prop_assert_eq!(
                    bridge.epoch,
                    0u64,
                    "epoch must remain 0 after same-epoch rejection (attempt rejected correctly)"
                );
            }
        }

        /// **Interleaved valid/invalid** — sequences that strictly alternate
        /// between a valid rotation and one of the fault categories.
        ///
        /// Verifies that faults injected between valid rotations do not corrupt
        /// the epoch counter and that the bridge can always resume with the next
        /// valid rotation after a fault.
        #[test]
        fn prop_interleaved_valid_and_fault(
            fault_variants in proptest::collection::vec(arb_attempt(), 2..=10)
        ) {
            let pools = build_key_pools();
            let initial_set = vs(&pools[0]);
            let mut bridge = Bridge::new(initial_set);

            let mut round: usize = 0;
            let mut total_successes: u64 = 0;

            for (step, fault) in fault_variants.iter().enumerate() {
                if round + 2 >= pools.len() {
                    break;
                }

                // ── Step A: inject the fault ──────────────────────────────
                let epoch_before_fault = bridge.epoch;
                let fault_succeeded = execute_attempt(&mut bridge, fault, round, &pools);

                if fault_succeeded {
                    // It was actually valid — count it and advance round.
                    prop_assert_eq!(
                        bridge.epoch,
                        epoch_before_fault + 1,
                        "interleaved step {} fault succeeded: epoch must be +1",
                        step
                    );
                    total_successes += 1;
                    round += 1;
                } else {
                    // Fault correctly rejected — epoch unchanged.
                    prop_assert_eq!(
                        bridge.epoch,
                        epoch_before_fault,
                        "interleaved step {} fault rejected: epoch must be unchanged",
                        step
                    );
                }

                if round + 1 >= pools.len() {
                    break;
                }

                // ── Step B: perform a valid rotation ─────────────────────
                let epoch_before_valid = bridge.epoch;
                let next_set = vs(&pools[round + 1]);
                let threshold = bridge.validators.threshold();
                let signers: Vec<&Keypair> = pools[round][..threshold].iter().collect();
                let proofs = sign_rotation(&next_set, epoch_before_valid + 1, &signers);

                bridge
                    .rotate_validators(next_set, epoch_before_valid + 1, proofs)
                    .unwrap_or_else(|e| {
                        panic!(
                            "interleaved step {step} valid rotation must succeed \
                             (epoch_before={epoch_before_valid}): {e}"
                        )
                    });

                prop_assert_eq!(
                    bridge.epoch,
                    epoch_before_valid + 1,
                    "interleaved step {} valid rotation must advance epoch by 1",
                    step
                );
                total_successes += 1;
                round += 1;
            }

            // Final invariant: epoch == number of successful rotations seen.
            prop_assert_eq!(
                bridge.epoch,
                total_successes,
                "final epoch {} must equal total successes {}",
                bridge.epoch,
                total_successes
            );
        }
    }

    // ── Deterministic regression cases ────────────────────────────────────────
    // These are hand-crafted sequences that are known to exercise corner-cases
    // proptest might not hit in every run.  They are not proptest cases (no
    // random input) but they rely on the same helpers and assert the same
    // invariants.

    /// **Edge case: rejected rotation mid-sequence leaves state intact.**
    ///
    /// Sequence: valid → invalid (wrong epoch) → valid.
    /// After the full sequence the epoch must be 2 (not 1 or 3).
    #[test]
    fn edge_rejected_mid_sequence_leaves_state_intact() {
        let pools = build_key_pools();
        let initial_set = vs(&pools[0]);
        let mut bridge = Bridge::new(initial_set);

        // Step 1: valid rotation (0 → 1)
        {
            let next = vs(&pools[1]);
            let threshold = bridge.validators.threshold();
            let signers: Vec<&Keypair> = pools[0][..threshold].iter().collect();
            let proofs = sign_rotation(&next, 1, &signers);
            bridge.rotate_validators(next, 1, proofs).expect("step 1 must succeed");
        }
        assert_eq!(bridge.epoch, 1, "epoch must be 1 after first valid rotation");

        // Step 2: invalid — skip to epoch 3 (must be rejected)
        {
            let next = vs(&pools[2]);
            let threshold = bridge.validators.threshold();
            let signers: Vec<&Keypair> = pools[1][..threshold].iter().collect();
            let proofs = sign_rotation(&next, 3, &signers);
            let result = bridge.rotate_validators(next, 3, proofs);
            assert!(result.is_err(), "skipped-epoch attempt must be rejected");
            assert_eq!(bridge.epoch, 1, "epoch must still be 1 after rejection");
        }

        // Step 3: valid rotation (1 → 2)
        {
            let next = vs(&pools[2]);
            let threshold = bridge.validators.threshold();
            let signers: Vec<&Keypair> = pools[1][..threshold].iter().collect();
            let proofs = sign_rotation(&next, 2, &signers);
            bridge.rotate_validators(next, 2, proofs).expect("step 3 must succeed");
        }
        assert_eq!(bridge.epoch, 2, "epoch must be 2 after second valid rotation");
    }

    /// **Edge case: large epoch number — epoch does not overflow or regress.**
    ///
    /// Advances the bridge to epoch 50 via valid rotations then asserts the
    /// epoch is exactly 50 and that `validate_inbound_epoch` correctly accepts
    /// epoch 50 and rejects epoch 49.
    #[test]
    fn edge_large_epoch_no_overflow_or_regress() {
        let pools = build_key_pools();
        let initial_set = vs(&pools[0]);
        let mut bridge = Bridge::new(initial_set);

        for r in 0..50 {
            let next = vs(&pools[r + 1]);
            let threshold = bridge.validators.threshold();
            let signers: Vec<&Keypair> = pools[r][..threshold].iter().collect();
            let proofs = sign_rotation(&next, bridge.epoch + 1, &signers);
            bridge
                .rotate_validators(next, bridge.epoch + 1, proofs)
                .unwrap_or_else(|e| panic!("rotation {r} failed: {e}"));
        }

        assert_eq!(bridge.epoch, 50, "epoch must be 50 after 50 valid rotations");
        assert!(bridge.validate_inbound_epoch(50).is_ok(), "epoch 50 must be accepted");
        assert!(bridge.validate_inbound_epoch(49).is_err(), "epoch 49 must be rejected");
    }

    /// **Edge case: epoch 0 stale attempt when already at epoch 0.**
    ///
    /// Confirms that `WrongEpochStale` at the very first rotation (when
    /// `current_epoch == 0` and `saturating_sub(1) == 0`) is still rejected
    /// by the `epoch != self.epoch + 1` gate (0 != 0 + 1 = 1).
    #[test]
    fn edge_stale_at_epoch_zero_rejected() {
        let pools = build_key_pools();
        let initial_set = vs(&pools[0]);
        let mut bridge = Bridge::new(initial_set);

        let next = vs(&pools[1]);
        let threshold = bridge.validators.threshold();
        let signers: Vec<&Keypair> = pools[0][..threshold].iter().collect();
        // epoch 0 when current is 0 → same as WrongEpochSame AND WrongEpochStale
        let proofs = sign_rotation(&next, 0, &signers);
        let result = bridge.rotate_validators(next, 0, proofs);
        assert!(result.is_err(), "epoch 0 at epoch 0 must be rejected");
        assert_eq!(bridge.epoch, 0, "epoch must remain 0");
    }
}
