//! Tests for Bridge::rotate_validators epoch monotonicity and quorum-proof
//! rejection paths.
//!
//! Coverage targets:
//!   - Non-incrementing epoch rejection (same epoch, stale epoch, jump >1)
//!   - Exactly-threshold acceptance
//!   - Below-threshold rejection (threshold - 1 proofs)
//!   - Duplicate signer in proof set (counted only once)
//!   - Signer not in current validator set
//!   - Empty proof list
//!   - Replay of inbound message signed under a rotated-out validator set
//!   - Sequential multi-rotation correctness

#[cfg(test)]
mod rotation_tests {
    use crate::{Bridge, ValidatorSet};
    use bincode;
    use ed25519_dalek::{Keypair, Signature, Signer};

    // ---------------------------------------------------------------------------
    // Deterministic test helpers
    // ---------------------------------------------------------------------------

    /// Build a deterministic `Keypair` seeded from a fixed 32-byte seed derived
    /// from `index`. Uses ed25519-dalek v1 `from_bytes` with a manually crafted
    /// seed so that tests are 100 % reproducible without `OsRng`.
    fn det_keypair(index: u8) -> Keypair {
        // 32-byte seed: first byte encodes the index, rest are a fixed pattern.
        let mut seed = [0u8; 32];
        seed[0] = index.wrapping_add(1); // avoid all-zero seed
        for i in 1..32 {
            seed[i] = index.wrapping_mul(7).wrapping_add(i as u8);
        }
        Keypair::from_bytes(&{
            // ed25519-dalek v1 Keypair::from_bytes expects [secret(32) || public(32)]
            // We derive the public key by constructing a SecretKey first.
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

    /// Build `n` deterministic keypairs (indices 0..n).
    fn det_keypairs(n: u8) -> Vec<Keypair> {
        (0..n).map(det_keypair).collect()
    }

    /// Construct a `ValidatorSet` from a slice of keypairs.
    fn validator_set_from(kps: &[Keypair]) -> ValidatorSet {
        ValidatorSet {
            validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
        }
    }

    /// Sign the rotation payload `(new_set_bytes, epoch)` with a subset of
    /// keypairs and return the proof vec expected by `rotate_validators`.
    fn sign_rotation(
        new_set: &ValidatorSet,
        epoch: u64,
        signers: &[&Keypair],
    ) -> Vec<(ed25519_dalek::PublicKey, Signature)> {
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch))
            .expect("serialization must not fail");
        signers
            .iter()
            .map(|kp| {
                let sig = kp.sign(&payload);
                (kp.public, sig)
            })
            .collect()
    }

    // ---------------------------------------------------------------------------
    // Epoch monotonicity tests
    // ---------------------------------------------------------------------------

    /// Rotating with the *same* epoch (not incrementing) must be rejected.
    #[test]
    fn test_reject_same_epoch() {
        let kps = det_keypairs(3);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        // start indices after existing set to avoid overlap
        let new_kps: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        // epoch == current (0) — must fail
        let epoch = 0u64;
        let signers: Vec<&Keypair> = kps.iter().collect();
        let proofs = sign_rotation(&new_set, epoch, &signers);

        let result = bridge.rotate_validators(new_set, epoch, proofs);
        assert!(result.is_err(), "same-epoch rotation must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("invalid epoch"),
            "error message should reference invalid epoch, got: {msg}"
        );
    }

    /// Rotating with epoch current + 2 (skipping an epoch) must be rejected.
    #[test]
    fn test_reject_skipped_epoch() {
        let kps = det_keypairs(3);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        // epoch == 2 (skipping 1) — must fail
        let epoch = 2u64;
        let signers: Vec<&Keypair> = kps.iter().collect();
        let proofs = sign_rotation(&new_set, epoch, &signers);

        let result = bridge.rotate_validators(new_set, epoch, proofs);
        assert!(result.is_err(), "skipped-epoch rotation must be rejected");
    }

    /// Rotating with a past epoch (stale) must be rejected.
    #[test]
    fn test_reject_stale_epoch() {
        let kps = det_keypairs(3);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        // First perform a valid rotation so bridge.epoch == 1
        {
            let signers: Vec<&Keypair> = kps.iter().collect();
            let proofs = sign_rotation(&new_set, 1, &signers);
            bridge
                .rotate_validators(new_set.clone(), 1, proofs)
                .expect("first rotation should succeed");
        }
        assert_eq!(bridge.epoch, 1);

        // Now attempt to rotate back to epoch 1 (stale)
        let newer_kps: Vec<Keypair> = (20..23).map(det_keypair).collect();
        let newer_set = validator_set_from(&newer_kps);
        let signers: Vec<&Keypair> = new_kps.iter().collect();
        let proofs = sign_rotation(&newer_set, 1, &signers);

        let result = bridge.rotate_validators(newer_set, 1, proofs);
        assert!(result.is_err(), "stale epoch replay must be rejected");
    }

    // ---------------------------------------------------------------------------
    // Quorum-threshold tests
    // ---------------------------------------------------------------------------

    /// Exactly threshold-many valid signatures must be accepted.
    #[test]
    fn test_exactly_threshold_accepted() {
        // 4 validators → threshold = (4*2)/3+1 = 3
        let kps = det_keypairs(4);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..14).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let threshold = bridge.validators.threshold();
        assert_eq!(threshold, 3, "expected threshold of 3 for 4 validators");

        // Provide exactly 3 signatures (the threshold)
        let signers: Vec<&Keypair> = kps[..threshold].iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        bridge
            .rotate_validators(new_set, 1, proofs)
            .expect("exactly-threshold rotation must succeed");
        assert_eq!(bridge.epoch, 1);
    }

    /// threshold − 1 valid signatures must be rejected.
    #[test]
    fn test_below_threshold_rejected() {
        // 4 validators → threshold = 3; provide only 2
        let kps = det_keypairs(4);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..14).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let threshold = bridge.validators.threshold(); // 3
        let below = threshold - 1; // 2

        let signers: Vec<&Keypair> = kps[..below].iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        let result = bridge.rotate_validators(new_set, 1, proofs);
        assert!(result.is_err(), "below-threshold rotation must fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("insufficient quorum"),
            "error should report insufficient quorum, got: {msg}"
        );
    }

    /// A 5-validator set has threshold 4. Providing 4 is the minimum acceptance.
    #[test]
    fn test_exactly_threshold_five_validators() {
        // 5 validators → threshold = (5*2)/3+1 = 4
        let kps = det_keypairs(5);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..15).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let threshold = bridge.validators.threshold();
        assert_eq!(threshold, 4, "expected threshold of 4 for 5 validators");

        // Exactly 4 signatures
        let signers: Vec<&Keypair> = kps[..threshold].iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        bridge
            .rotate_validators(new_set, 1, proofs)
            .expect("threshold-4 rotation must succeed");
    }

    // ---------------------------------------------------------------------------
    // Duplicate signer tests
    // ---------------------------------------------------------------------------

    /// Duplicate entries for the same signer must be deduplicated — they should
    /// count as only one vote. If deduplication causes the count to drop below
    /// the quorum threshold the rotation must be rejected.
    #[test]
    fn test_duplicate_signer_counts_once_below_threshold() {
        // 4 validators → threshold = 3
        // Provide 3 proof entries but two of them are for the same keypair → unique = 2 < 3
        let kps = det_keypairs(4);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..14).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let epoch = 1u64;
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch)).unwrap();

        // kps[0] signs twice, kps[1] signs once → 3 entries but only 2 unique
        let mut proofs = Vec::new();
        for _ in 0..2 {
            proofs.push((kps[0].public, kps[0].sign(&payload)));
        }
        proofs.push((kps[1].public, kps[1].sign(&payload)));

        let result = bridge.rotate_validators(new_set, epoch, proofs);
        assert!(
            result.is_err(),
            "duplicate signer must not inflate quorum count"
        );
    }

    /// Even with a duplicate, if the remaining unique signers still meet the
    /// threshold the rotation should succeed.
    #[test]
    fn test_duplicate_signer_still_meets_threshold() {
        // 4 validators → threshold = 3
        // Provide 4 entries: kps[0] twice, kps[1] once, kps[2] once → 3 unique = threshold
        let kps = det_keypairs(4);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..14).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let epoch = 1u64;
        let payload = bincode::serialize(&(new_set.to_bytes_vec(), epoch)).unwrap();

        let mut proofs = Vec::new();
        // kps[0] appears twice
        proofs.push((kps[0].public, kps[0].sign(&payload)));
        proofs.push((kps[0].public, kps[0].sign(&payload)));
        // kps[1] and kps[2] appear once each → total unique = 3
        proofs.push((kps[1].public, kps[1].sign(&payload)));
        proofs.push((kps[2].public, kps[2].sign(&payload)));

        bridge
            .rotate_validators(new_set, epoch, proofs)
            .expect("3 unique valid signers must meet threshold for 4-validator set");
        assert_eq!(bridge.epoch, 1);
    }

    // ---------------------------------------------------------------------------
    // Signer-not-in-set and empty-proof tests
    // ---------------------------------------------------------------------------

    /// A proof entry whose public key is not in the current validator set must be
    /// rejected immediately.
    #[test]
    fn test_reject_signer_not_in_validator_set() {
        let kps = det_keypairs(3);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        // Use an outsider keypair (index 50) not in the initial set
        let outsider = det_keypair(50);
        let signers: Vec<&Keypair> = vec![&outsider];
        let proofs = sign_rotation(&new_set, 1, &signers);

        let result = bridge.rotate_validators(new_set, 1, proofs);
        assert!(result.is_err(), "outsider signer must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("signer not in current validator set"),
            "error should mention invalid signer, got: {msg}"
        );
    }

    /// An entirely empty proof list must be rejected.
    #[test]
    fn test_reject_empty_proofs() {
        let kps = det_keypairs(3);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let result = bridge.rotate_validators(new_set, 1, vec![]);
        assert!(result.is_err(), "empty proof list must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("empty proofs"),
            "error should report empty proofs, got: {msg}"
        );
    }

    // ---------------------------------------------------------------------------
    // Replay / rotated-out-set tests
    // ---------------------------------------------------------------------------

    /// After a rotation, inbound messages bearing the old epoch must be rejected
    /// by `validate_inbound_epoch`.
    #[test]
    fn test_validate_inbound_epoch_rejects_old_epoch() {
        let kps = det_keypairs(3);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let signers: Vec<&Keypair> = kps.iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);
        bridge
            .rotate_validators(new_set, 1, proofs)
            .expect("rotation must succeed");

        // epoch 0 is the old (rotated-out) epoch
        let result = bridge.validate_inbound_epoch(0);
        assert!(
            result.is_err(),
            "old-epoch inbound message must be rejected after rotation"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("retired validator set"),
            "error should reference retired validator set, got: {msg}"
        );
    }

    /// Inbound messages with the *current* epoch after rotation must be accepted.
    #[test]
    fn test_validate_inbound_epoch_accepts_current_epoch() {
        let kps = det_keypairs(3);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let new_kps: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let new_set = validator_set_from(&new_kps);

        let signers: Vec<&Keypair> = kps.iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);
        bridge
            .rotate_validators(new_set, 1, proofs)
            .expect("rotation must succeed");

        assert!(
            bridge.validate_inbound_epoch(1).is_ok(),
            "current epoch must be accepted"
        );
    }

    /// A replay attack: old-set signatures presented for a new rotation after the
    /// set has been rotated out must fail because those keys are no longer in the
    /// current validator set.
    #[test]
    fn test_reject_replay_of_old_validator_set_on_rotation() {
        // Set A (initial)
        let kps_a: Vec<Keypair> = (0..4).map(det_keypair).collect();
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        // Set B
        let kps_b: Vec<Keypair> = (10..14).map(det_keypair).collect();
        let set_b = validator_set_from(&kps_b);

        // Set C
        let kps_c: Vec<Keypair> = (20..24).map(det_keypair).collect();
        let set_c = validator_set_from(&kps_c);

        // Rotate A → B (epoch 1)
        {
            let signers: Vec<&Keypair> = kps_a.iter().collect();
            let proofs = sign_rotation(&set_b, 1, &signers);
            bridge
                .rotate_validators(set_b.clone(), 1, proofs)
                .expect("A→B rotation must succeed");
        }
        assert_eq!(bridge.epoch, 1);

        // Now attempt B → C at epoch 2, but using *A's* (old/rotated-out) signatures
        let signers_a: Vec<&Keypair> = kps_a.iter().collect();
        let proofs_from_a = sign_rotation(&set_c, 2, &signers_a);

        let result = bridge.rotate_validators(set_c, 2, proofs_from_a);
        assert!(
            result.is_err(),
            "rotation signed by rotated-out validator set A must be rejected"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("signer not in current validator set"),
            "error should reference invalid signer from old set, got: {msg}"
        );
        // Bridge state must be unchanged
        assert_eq!(bridge.epoch, 1, "epoch must not advance on rejected rotation");
    }

    // ---------------------------------------------------------------------------
    // Sequential multi-rotation tests
    // ---------------------------------------------------------------------------

    /// Three successive rotations (A→B→C→D) must all succeed with epoch
    /// monotonically increasing 0 → 1 → 2 → 3.
    #[test]
    fn test_sequential_rotations_epoch_monotonicity() {
        let kps_a: Vec<Keypair> = (0..3).map(det_keypair).collect();
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        let sets: Vec<(Vec<Keypair>, Vec<Keypair>)> = vec![
            // `det_keypair` is deterministic, so regenerating kps_a's range is
            // equivalent to cloning it — and ed25519-dalek::Keypair doesn't
            // implement Clone, so we can't clone it directly.
            ((0..3).map(det_keypair).collect(), (10..13).map(det_keypair).collect()),
            ((10..13).map(det_keypair).collect(), (20..23).map(det_keypair).collect()),
            ((20..23).map(det_keypair).collect(), (30..33).map(det_keypair).collect()),
        ];

        for (i, (current_kps, next_kps)) in sets.into_iter().enumerate() {
            let new_set = validator_set_from(&next_kps);
            let epoch = (i + 1) as u64;
            let signers: Vec<&Keypair> = current_kps.iter().collect();
            let proofs = sign_rotation(&new_set, epoch, &signers);
            bridge
                .rotate_validators(new_set, epoch, proofs)
                .unwrap_or_else(|e| panic!("rotation {epoch} failed: {e}"));
            assert_eq!(bridge.epoch, epoch, "epoch must equal {epoch} after rotation {i}");
        }
    }

    /// After two rotations, inbound messages from the very first epoch (0) and
    /// the intermediate epoch (1) must both be rejected.
    #[test]
    fn test_validate_inbound_rejects_all_prior_epochs_after_double_rotation() {
        let kps_a: Vec<Keypair> = (0..3).map(det_keypair).collect();
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        // Rotation 1: A → B
        let kps_b: Vec<Keypair> = (10..13).map(det_keypair).collect();
        let set_b = validator_set_from(&kps_b);
        {
            let signers: Vec<&Keypair> = kps_a.iter().collect();
            let proofs = sign_rotation(&set_b, 1, &signers);
            bridge.rotate_validators(set_b, 1, proofs).unwrap();
        }

        // Rotation 2: B → C
        let kps_c: Vec<Keypair> = (20..23).map(det_keypair).collect();
        let set_c = validator_set_from(&kps_c);
        {
            let signers: Vec<&Keypair> = kps_b.iter().collect();
            let proofs = sign_rotation(&set_c, 2, &signers);
            bridge.rotate_validators(set_c, 2, proofs).unwrap();
        }

        assert_eq!(bridge.epoch, 2);

        // Both prior epochs must be rejected
        assert!(
            bridge.validate_inbound_epoch(0).is_err(),
            "epoch 0 must be rejected after double rotation"
        );
        assert!(
            bridge.validate_inbound_epoch(1).is_err(),
            "epoch 1 must be rejected after advancing to epoch 2"
        );
        // Current epoch must be accepted
        assert!(
            bridge.validate_inbound_epoch(2).is_ok(),
            "epoch 2 (current) must be accepted"
        );
    }

    // ---------------------------------------------------------------------------
    // Threshold formula correctness
    // ---------------------------------------------------------------------------

    /// Verify the threshold formula for various set sizes.
    #[test]
    fn test_threshold_formula() {
        let cases: &[(usize, usize)] = &[
            (1, 1), // 1 validator → threshold = 1
            (2, 2), // 2 → 2
            (3, 3), // 3 → 3
            (4, 3), // 4 → 3
            (5, 4), // 5 → 4
            (6, 5), // 6 → 5
            (7, 5), // 7 → 5
        ];
        for &(n, expected_threshold) in cases {
            let validators: Vec<Vec<u8>> = (0..n as u8)
                .map(|i| det_keypair(i).public.to_bytes().to_vec())
                .collect();
            let vs = ValidatorSet { validators };
            assert_eq!(
                vs.threshold(),
                expected_threshold,
                "threshold mismatch for n={n}: got {}, expected {expected_threshold}",
                vs.threshold()
            );
        }
    }
}
