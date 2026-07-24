//! Domain-separation tests for bridge quorum proofs (issue #1146).
//!
//! These prove that a quorum signature is bound to BOTH:
//!   1. the constant purpose tag [`QUORUM_PROOF_DOMAIN`], and
//!   2. the per-instance `bridge_id`,
//!
//! so a signature gathered for one bridge instance/purpose cannot be replayed
//! against another that shares the same validator set and epoch.

#[cfg(test)]
mod domain_separation_tests {
    use crate::{Bridge, ValidatorSet, QUORUM_PROOF_DOMAIN};
    use ed25519_dalek::{Keypair, PublicKey, Signature, Signer};

    const BRIDGE_A: &[u8] = b"stellarlend-bridge-A";
    const BRIDGE_B: &[u8] = b"stellarlend-bridge-B";

    /// Deterministic keypair seeded from `index` (no `OsRng`), matching the
    /// pattern used by the other bridge test modules.
    fn det_keypair(index: u8) -> Keypair {
        let mut seed = [0u8; 32];
        seed[0] = index.wrapping_add(1);
        for i in 1..32 {
            seed[i] = index.wrapping_mul(7).wrapping_add(i as u8);
        }
        Keypair::from_bytes(&{
            use ed25519_dalek::SecretKey;
            let secret = SecretKey::from_bytes(&seed).expect("valid secret key");
            let public: PublicKey = (&secret).into();
            let mut combined = [0u8; 64];
            combined[..32].copy_from_slice(&seed);
            combined[32..].copy_from_slice(public.as_bytes());
            combined
        })
        .expect("valid keypair from seed")
    }

    fn validator_set_from(kps: &[Keypair]) -> ValidatorSet {
        ValidatorSet {
            validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
        }
    }

    /// Sign an arbitrary `payload` with each signer, producing the proof vec
    /// `rotate_validators` expects.
    fn sign_with(payload: &[u8], signers: &[&Keypair]) -> Vec<(PublicKey, Signature)> {
        signers
            .iter()
            .map(|kp| (kp.public, kp.sign(payload)))
            .collect()
    }

    /// Current set of 4 validators, a new set of 3, and the next epoch (1).
    /// Threshold for a 4-validator set is `(4*2)/3 + 1 = 3`.
    fn fixture() -> (Vec<Keypair>, ValidatorSet, ValidatorSet, u64) {
        let current = (0..4).map(det_keypair).collect::<Vec<_>>();
        let current_set = validator_set_from(&current);
        let new = (10..13).map(det_keypair).collect::<Vec<_>>();
        let new_set = validator_set_from(&new);
        (current, current_set, new_set, 1)
    }

    #[test]
    fn correct_domain_is_accepted() {
        let (current, current_set, new_set, epoch) = fixture();
        let mut bridge = Bridge::new_with_id(current_set, BRIDGE_A.to_vec());

        // Sign the canonical, domain-separated payload for THIS bridge instance.
        let payload = Bridge::quorum_proof_payload(&bridge.bridge_id, &new_set, epoch).unwrap();
        let signers: Vec<&Keypair> = current.iter().take(3).collect();
        let proofs = sign_with(&payload, &signers);

        bridge
            .rotate_validators(new_set, epoch, proofs)
            .expect("a correctly domain-separated quorum proof must rotate");
        assert_eq!(bridge.epoch, 1);
    }

    #[test]
    fn wrong_bridge_id_is_rejected() {
        let (current, current_set, new_set, epoch) = fixture();
        let mut bridge = Bridge::new_with_id(current_set, BRIDGE_A.to_vec());

        // Validators sign a payload bound to a DIFFERENT bridge instance (B).
        let foreign = Bridge::quorum_proof_payload(BRIDGE_B, &new_set, epoch).unwrap();
        let signers: Vec<&Keypair> = current.iter().take(3).collect();
        let proofs = sign_with(&foreign, &signers);

        assert!(
            bridge.rotate_validators(new_set, epoch, proofs).is_err(),
            "a proof bound to another bridge id must not verify"
        );
        assert_eq!(bridge.epoch, 0, "epoch must be unchanged on rejection");
    }

    #[test]
    fn wrong_purpose_tag_is_rejected() {
        let (current, current_set, new_set, epoch) = fixture();
        let mut bridge = Bridge::new_with_id(current_set, BRIDGE_A.to_vec());

        // Same bridge id, but signed under a different (non-quorum) purpose tag.
        let other_purpose = b"stellarlend::bridge::some_other_purpose::v1".as_slice();
        let payload = bincode::serialize(&(
            other_purpose,
            &bridge.bridge_id[..],
            new_set.to_bytes_vec(),
            epoch,
        ))
        .unwrap();
        let signers: Vec<&Keypair> = current.iter().take(3).collect();
        let proofs = sign_with(&payload, &signers);

        assert!(
            bridge.rotate_validators(new_set, epoch, proofs).is_err(),
            "a proof signed under a different purpose tag must not verify"
        );
        assert_eq!(bridge.epoch, 0);
    }

    #[test]
    fn legacy_untagged_signature_is_rejected() {
        let (current, current_set, new_set, epoch) = fixture();
        let mut bridge = Bridge::new_with_id(current_set, BRIDGE_A.to_vec());

        // The OLD format: bincode((new_set_bytes, epoch)) with no domain prefix.
        // Existing signatures over this payload must no longer verify.
        let legacy = bincode::serialize(&(new_set.to_bytes_vec(), epoch)).unwrap();
        let signers: Vec<&Keypair> = current.iter().take(3).collect();
        let proofs = sign_with(&legacy, &signers);

        assert!(
            bridge.rotate_validators(new_set, epoch, proofs).is_err(),
            "a legacy un-tagged signature must not verify after domain separation"
        );
        assert_eq!(bridge.epoch, 0);
    }

    #[test]
    fn signature_for_one_instance_does_not_verify_on_another() {
        let (current, current_set, new_set, epoch) = fixture();

        // Collect a valid proof for instance A.
        let payload_a = Bridge::quorum_proof_payload(BRIDGE_A, &new_set, epoch).unwrap();
        let signers: Vec<&Keypair> = current.iter().take(3).collect();
        let proofs = sign_with(&payload_a, &signers);

        // Replay the very same proof against instance B (same validators/epoch,
        // different bridge id). It must be rejected.
        let mut bridge_b = Bridge::new_with_id(current_set, BRIDGE_B.to_vec());
        assert!(
            bridge_b.rotate_validators(new_set, epoch, proofs).is_err(),
            "instance A's proof must not rotate instance B"
        );
        assert_eq!(bridge_b.epoch, 0);
    }

    #[test]
    fn domain_constant_is_versioned() {
        // The purpose tag is a stable, versioned constant; document its value via
        // a test so an accidental change is caught.
        assert_eq!(
            QUORUM_PROOF_DOMAIN,
            b"stellarlend::bridge::quorum_proof::v1"
        );
    }
}
