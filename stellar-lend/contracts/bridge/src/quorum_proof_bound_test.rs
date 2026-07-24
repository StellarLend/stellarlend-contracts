#[cfg(test)]
mod quorum_proof_bound_tests {
    use crate::{Bridge, ValidatorSet};
    use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signature, Signer};

    /// Build a deterministic ed25519 keypair for reproducible quorum tests.
    fn det_keypair(index: u8) -> Keypair {
        let mut seed = [0u8; 32];
        seed[0] = index.wrapping_add(1);
        for i in 1..32 {
            seed[i] = index.wrapping_mul(11).wrapping_add(i as u8);
        }

        let secret = SecretKey::from_bytes(&seed).expect("valid secret key");
        let public: PublicKey = (&secret).into();
        let mut combined = [0u8; 64];
        combined[..32].copy_from_slice(&seed);
        combined[32..].copy_from_slice(public.as_bytes());
        Keypair::from_bytes(&combined).expect("valid keypair")
    }

    /// Build `n` deterministic keypairs with adjacent indices.
    fn det_keypairs(start: u8, n: u8) -> Vec<Keypair> {
        (start..start + n).map(det_keypair).collect()
    }

    /// Convert keypairs into the byte-backed validator-set representation.
    fn validator_set_from(kps: &[Keypair]) -> ValidatorSet {
        ValidatorSet {
            validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
        }
    }

    /// Sign a bridge rotation payload with the supplied signers.
    fn sign_rotation(
        bridge: &Bridge,
        new_set: &ValidatorSet,
        epoch: u64,
        signers: &[&Keypair],
    ) -> Vec<(PublicKey, Signature)> {
        let payload = Bridge::quorum_proof_payload(&bridge.bridge_id, new_set, epoch)
            .expect("payload serialization must succeed");

        signers
            .iter()
            .map(|kp| (kp.public, kp.sign(&payload)))
            .collect()
    }

    /// Return a syntactically valid but cryptographically invalid signature.
    fn invalid_signature() -> Signature {
        Signature::from_bytes(&[0u8; 64]).expect("zero bytes form a signature object")
    }

    /// Rejects proof vectors larger than the current validator set before
    /// membership or signature verification can run.
    #[test]
    fn oversized_proof_vector_is_rejected_before_signature_work() {
        let current = det_keypairs(0, 4);
        let next = det_keypairs(10, 4);
        let initial = validator_set_from(&current);
        let new_set = validator_set_from(&next);
        let mut bridge = Bridge::new(initial);

        let outsider = det_keypair(99);
        let mut proofs = sign_rotation(
            &bridge,
            &new_set,
            1,
            &[&current[0], &current[1], &current[2], &current[3]],
        );
        proofs.push((outsider.public, invalid_signature()));

        let err = bridge
            .rotate_validators(new_set, 1, proofs)
            .expect_err("proof vector larger than the current set must reject");
        assert!(
            err.to_string().contains("current validator set has 4"),
            "expected size-bound error before signer verification, got: {err}"
        );
        assert_eq!(bridge.epoch, 0);
    }

    /// Rejects duplicate signer entries before any signature is verified.
    #[test]
    fn duplicate_signer_entries_are_rejected_up_front() {
        let current = det_keypairs(0, 4);
        let next = det_keypairs(10, 4);
        let initial = validator_set_from(&current);
        let new_set = validator_set_from(&next);
        let mut bridge = Bridge::new(initial);

        let mut proofs = sign_rotation(&bridge, &new_set, 1, &[&current[0], &current[1]]);
        proofs.push((current[0].public, invalid_signature()));

        let err = bridge
            .rotate_validators(new_set, 1, proofs)
            .expect_err("duplicate signer entries must reject");
        assert!(
            err.to_string().contains("duplicate signer"),
            "expected duplicate-signer error before signature verification, got: {err}"
        );
        assert_eq!(bridge.epoch, 0);
    }

    /// A proof vector exactly as large as the current set is still allowed when
    /// all entries are unique and valid.
    #[test]
    fn proof_equal_to_current_validator_count_is_allowed() {
        let current = det_keypairs(0, 4);
        let next = det_keypairs(10, 4);
        let initial = validator_set_from(&current);
        let new_set = validator_set_from(&next);
        let mut bridge = Bridge::new(initial);

        let proofs = sign_rotation(
            &bridge,
            &new_set,
            1,
            &[&current[0], &current[1], &current[2], &current[3]],
        );

        bridge
            .rotate_validators(new_set, 1, proofs)
            .expect("unique full-set proof should satisfy quorum");
        assert_eq!(bridge.epoch, 1);
    }

    /// Preserves the existing exact-quorum success path for valid in-bound
    /// proofs.
    #[test]
    fn exact_quorum_unique_proof_is_allowed() {
        let current = det_keypairs(0, 4);
        let next = det_keypairs(10, 4);
        let initial = validator_set_from(&current);
        let new_set = validator_set_from(&next);
        let mut bridge = Bridge::new(initial);

        let proofs = sign_rotation(
            &bridge,
            &new_set,
            1,
            &[&current[0], &current[1], &current[2]],
        );

        bridge
            .rotate_validators(new_set, 1, proofs)
            .expect("three unique signers meet quorum for four validators");
        assert_eq!(bridge.epoch, 1);
    }

    /// Preserves the existing below-quorum rejection path for valid but
    /// insufficient unique proofs.
    #[test]
    fn below_quorum_unique_proof_still_rejects() {
        let current = det_keypairs(0, 4);
        let next = det_keypairs(10, 4);
        let initial = validator_set_from(&current);
        let new_set = validator_set_from(&next);
        let mut bridge = Bridge::new(initial);

        let proofs = sign_rotation(&bridge, &new_set, 1, &[&current[0], &current[1]]);

        let err = bridge
            .rotate_validators(new_set, 1, proofs)
            .expect_err("two unique signers are below quorum for four validators");
        assert!(
            err.to_string().contains("insufficient quorum"),
            "expected quorum error, got: {err}"
        );
        assert_eq!(bridge.epoch, 0);
    }
}
