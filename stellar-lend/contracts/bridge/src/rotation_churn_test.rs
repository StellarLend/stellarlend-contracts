#[cfg(test)]
mod rotation_churn_tests {
    use crate::{Bridge, ValidatorSet};
    use ed25519_dalek::{Keypair, Signature, Signer};

    /// Build a deterministic `Keypair` seeded from a fixed 32-byte seed derived from `index`.
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

    /// Build `n` deterministic keypairs.
    fn det_keypairs(n: u8) -> Vec<Keypair> {
        (0..n).map(det_keypair).collect()
    }

    /// Construct a `ValidatorSet` from a slice of keypairs.
    fn validator_set_from(kps: &[Keypair]) -> ValidatorSet {
        ValidatorSet {
            validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
        }
    }

    /// Sign the rotation payload `(new_set_bytes, epoch)` with a subset of keypairs.
    fn sign_rotation(
        new_set: &ValidatorSet,
        epoch: u64,
        signers: &[&Keypair],
    ) -> Vec<(ed25519_dalek::PublicKey, Signature)> {
        // Bridges in these tests are constructed via `Bridge::new`, which uses an
        // empty `bridge_id`. Sign over the same domain-separated payload that
        // `verify_quorum_proof` recomputes (issue #1146).
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

    /// Test: Unset limit (default) preserves the existing behavior (no churn limit enforced).
    #[test]
    fn test_churn_limit_unset_noop() {
        let kps_a = det_keypairs(4); // A, B, C, D
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        // Entirely new validator set (4 new validators) -> Churn = 4 + 4 = 8
        let kps_b: Vec<Keypair> = (10..14).map(det_keypair).collect();
        let new_set = validator_set_from(&kps_b);

        let signers: Vec<&Keypair> = kps_a.iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        // Should succeed because max_churn is None (unset)
        let result = bridge.rotate_validators(new_set, 1, proofs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 8);
        assert_eq!(bridge.epoch, 1);
    }

    /// Test: Churn within the configured limit is accepted.
    #[test]
    fn test_churn_within_limit_accepted() {
        let kps_a = det_keypairs(4); // A, B, C, D
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        // Configure churn limit: max 2 changes
        bridge.set_max_churn(Some(2));

        // Rotate by replacing 1 validator: A, B, C, E (added 1, removed 1 -> churn = 2)
        let mut kps_b = det_keypairs(4);
        kps_b[3] = det_keypair(10); // Replace D with E
        let new_set = validator_set_from(&kps_b);

        let signers: Vec<&Keypair> = kps_a.iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        let result = bridge.rotate_validators(new_set, 1, proofs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
        assert_eq!(bridge.epoch, 1);
    }

    /// Test: Churn over the configured limit is rejected.
    #[test]
    fn test_churn_over_limit_rejected() {
        let kps_a = det_keypairs(4); // A, B, C, D
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        // Configure churn limit: max 2 changes
        bridge.set_max_churn(Some(2));

        // Rotate by replacing 2 validators: A, B, E, F (added 2, removed 2 -> churn = 4)
        let mut kps_b = det_keypairs(4);
        kps_b[2] = det_keypair(10); // Replace C with E
        kps_b[3] = det_keypair(11); // Replace D with F
        let new_set = validator_set_from(&kps_b);

        let signers: Vec<&Keypair> = kps_a.iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        let result = bridge.rotate_validators(new_set, 1, proofs);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("exceeds the limit"));
        assert_eq!(bridge.epoch, 0, "Epoch must not advance on rejected rotation");
    }

    /// Test: Full-set replacement is rejected when limited.
    #[test]
    fn test_full_set_replacement_rejected_when_limited() {
        let kps_a = det_keypairs(4);
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        // Set max churn limit to 3 (less than total replacement of 4 + 4 = 8)
        bridge.set_max_churn(Some(3));

        let kps_b: Vec<Keypair> = (10..14).map(det_keypair).collect();
        let new_set = validator_set_from(&kps_b);

        let signers: Vec<&Keypair> = kps_a.iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        let result = bridge.rotate_validators(new_set, 1, proofs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds the limit"));
    }

    /// Test: Quorum proof verification is still fully enforced.
    #[test]
    fn test_quorum_still_enforced() {
        let kps_a = det_keypairs(4);
        let initial = validator_set_from(&kps_a);
        let mut bridge = Bridge::new(initial);

        // Configure churn limit: max 2 changes
        bridge.set_max_churn(Some(2));

        // Rotate by replacing 1 validator (churn = 2, which is allowed)
        let mut kps_b = det_keypairs(4);
        kps_b[3] = det_keypair(10);
        let new_set = validator_set_from(&kps_b);

        // Provide insufficient quorum: threshold is 3, provide only 2 signatures
        let signers: Vec<&Keypair> = kps_a[..2].iter().collect();
        let proofs = sign_rotation(&new_set, 1, &signers);

        let result = bridge.rotate_validators(new_set, 1, proofs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("insufficient quorum"));
    }
}
