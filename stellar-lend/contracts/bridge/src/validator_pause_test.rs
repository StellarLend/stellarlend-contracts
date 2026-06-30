//! Tests for the per-validator bridge pause flag
//! ([`crate::Bridge::pause_validator`] / [`crate::Bridge::unpause_validator`]).
//!
//! Coverage targets:
//!   - Default state: no guardian, empty paused set
//!   - Guardian-bound: pause / unpause reject without a configured guardian
//!   - Signature auth: pause / unpause reject on invalid guardian signature
//!   - Signature replay: pause sig cannot be replayed as unpause, and vice versa
//!   - Action-bound replay: pause sig for validator A cannot be replayed for validator B
//!   - Fail-closed: pause that would make active count < new threshold is rejected
//!   - Idempotency: already-paused / not-paused are explicitly rejected (not no-ops)
//!   - Unknown validator: pause / unpause reject targets not in the current set
//!   - Effect on quorum: a single paused validator lowers the effective
//!     supermajority threshold by one
//!   - Bridge survives with quorum after a non-breaking pause: rotation still
//!     succeeds, but with one fewer required signer
//!   - Paused validator sig is silently skipped in verify_quorum_proof (not
//!     an error) so stale network gossip doesn't break the bridge
//!   - Paused validators with bogus sigs do not poison the proof batch
//!   - Rotation resets the paused set
//!   - Pause list / is_paused / active_validator_count helpers agree

#[cfg(test)]
mod validator_pause_tests {
    use crate::{Bridge, BridgeError, ValidatorEvent, ValidatorSet};
    use ed25519_dalek::{Keypair, SecretKey, Signature, Signer};

    // -----------------------------------------------------------------------
    // Deterministic key helpers
    // -----------------------------------------------------------------------

    fn det_keypair(index: u8) -> Keypair {
        // 32-byte seed: first byte encodes the index, rest are a fixed pattern.
        let mut seed = [0u8; 32];
        seed[0] = index.wrapping_add(1); // avoid all-zero seed
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

    fn det_keypairs_range(start: u8, end: u8) -> Vec<Keypair> {
        (start..end).map(det_keypair).collect()
    }

    fn validator_set_from(kps: &[Keypair]) -> ValidatorSet {
        ValidatorSet {
            validators: kps.iter().map(|kp| kp.public.to_bytes().to_vec()).collect(),
        }
    }

    fn make_bridge_with_guardian(n_validators: u8, guardian_idx: u8) -> (Bridge, Vec<Keypair>, Keypair) {
        let validators = det_keypairs_range(10, 10 + n_validators);
        let initial = validator_set_from(&validators);
        let mut bridge = Bridge::new(initial);
        let guardian = det_keypair(guardian_idx);
        bridge.set_guardian(guardian.public);
        (bridge, validators, guardian)
    }

    fn payload_for(action: &[u8], pk_bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(action.len() + pk_bytes.len());
        out.extend_from_slice(action);
        out.extend_from_slice(pk_bytes);
        out
    }

    fn sign_pause(guardian: &Keypair, target: &ed25519_dalek::PublicKey) -> Signature {
        let payload = payload_for(b"BRIDGE_PAUSE:", &target.to_bytes());
        guardian.sign(&payload)
    }

    fn sign_unpause(guardian: &Keypair, target: &ed25519_dalek::PublicKey) -> Signature {
        let payload = payload_for(b"BRIDGE_UNPAUSE:", &target.to_bytes());
        guardian.sign(&payload)
    }

    fn sign_rotation(new_set: &ValidatorSet, epoch: u64, signers: &[&Keypair]) -> Vec<(ed25519_dalek::PublicKey, Signature)> {
        let payload = Bridge::quorum_proof_payload(&[], new_set, epoch).unwrap();
        signers
            .iter()
            .map(|kp| {
                let sig = kp.sign(&payload);
                (kp.public, sig)
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Default state
    // -----------------------------------------------------------------------

    #[test]
    fn fresh_bridge_has_no_guardian_and_empty_paused_set() {
        let bridge = Bridge::new(ValidatorSet { validators: vec![] });
        assert!(bridge.guardian().is_none());
        assert!(bridge.paused_list().is_empty());
        assert_eq!(bridge.active_validator_count(), 0);
        assert_eq!(bridge.effective_threshold(), 1);
    }

    // -----------------------------------------------------------------------
    // Guardian gating
    // -----------------------------------------------------------------------

    #[test]
    fn pause_rejects_without_configured_guardian() {
        let kps = det_keypairs_range(10, 13);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let bogus_sig = Signature::from_bytes(&[0u8; 64]).expect("64 zero bytes is a valid signature encoding");
        let err = bridge
            .pause_validator(&kps[0].public, &bogus_sig)
            .expect_err("pause must fail when no guardian is configured");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::NoGuardianConfigured)
        );
        assert!(bridge.paused_list().is_empty(), "no state change on rejection");
    }

    #[test]
    fn unpause_rejects_without_configured_guardian() {
        let kps = det_keypairs_range(10, 13);
        let initial = validator_set_from(&kps);
        let mut bridge = Bridge::new(initial);

        let bogus_sig = Signature::from_bytes(&[0u8; 64]).expect("64 zero bytes is a valid signature encoding");
        let err = bridge
            .unpause_validator(&kps[0].public, &bogus_sig)
            .expect_err("unpause must fail when no guardian is configured");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::NoGuardianConfigured)
        );
    }

    // -----------------------------------------------------------------------
    // Signature authorization
    // -----------------------------------------------------------------------

    #[test]
    fn pause_rejects_if_signature_does_not_verify_against_guardian() {
        let (mut bridge, validators, _guardian) = make_bridge_with_guardian(4, 200);
        // Sign with the WRONG key — a status-quo validator pretending to be the guardian.
        let wrong_signer = &validators[0];
        let target = &validators[1].public;
        let payload = payload_for(b"BRIDGE_PAUSE:", &target.to_bytes());
        let bad_sig = wrong_signer.sign(&payload);

        let err = bridge
            .pause_validator(target, &bad_sig)
            .expect_err("pause must reject signatures not from the configured guardian");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InvalidGuardianSignature)
        );
        assert!(bridge.paused_list().is_empty(), "no state change on rejection");
    }

    #[test]
    fn unpause_rejects_if_signature_does_not_verify_against_guardian() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let target = &validators[1].public;
        // Pause legitimately.
        bridge
            .pause_validator(target, &sign_pause(&guardian, target))
            .expect("pause should succeed with valid guardian signature");
        assert!(bridge.is_paused(target));

        // Try to unpause with the wrong signer.
        let bad_sig = validators[0].sign(&payload_for(b"BRIDGE_UNPAUSE:", &target.to_bytes()));
        let err = bridge
            .unpause_validator(target, &bad_sig)
            .expect_err("unpause must reject signatures not from the configured guardian");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InvalidGuardianSignature)
        );
        assert!(bridge.is_paused(target), "validator must remain paused after rejected unpause");
    }

    // -----------------------------------------------------------------------
    // Signature replay protection
    // -----------------------------------------------------------------------

    /// A signature produced for `pause(A)` must NOT verify as `unpause(A)` —
    /// the action binding tag must distinguish them.
    #[test]
    fn pause_signature_cannot_be_replayed_as_unpause() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let target = &validators[1].public;
        bridge
            .pause_validator(target, &sign_pause(&guardian, target))
            .expect("legitimate pause");
        assert!(bridge.is_paused(target));

        // Now try to "unpause" using the pause signature on the same key.
        let replay = sign_pause(&guardian, target);
        let err = bridge
            .unpause_validator(target, &replay)
            .expect_err("pause-bound signature must not authorize unpause");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InvalidGuardianSignature)
        );
        assert!(bridge.is_paused(target));
    }

    /// A signature produced for `pause(A)` must NOT verify for `pause(B)` —
    /// the target key binding must distinguish them.
    #[test]
    fn pause_signature_for_a_cannot_be_replayed_for_b() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let a = &validators[1].public;
        let b = &validators[2].public;

        let sig_for_a = sign_pause(&guardian, a);
        let err = bridge
            .pause_validator(b, &sig_for_a)
            .expect_err("pause(A) signature must not authorize pause(B)");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::InvalidGuardianSignature)
        );
        assert!(!bridge.is_paused(a));
        assert!(!bridge.is_paused(b));
    }

    // -----------------------------------------------------------------------
    // Idempotency (already paused / not paused)
    // -----------------------------------------------------------------------

    #[test]
    fn pause_rejects_when_already_paused() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(5, 200);
        let target = &validators[1].public;
        bridge
            .pause_validator(target, &sign_pause(&guardian, target))
            .expect("first pause should succeed");

        let err = bridge
            .pause_validator(target, &sign_pause(&guardian, target))
            .expect_err("double pause must be rejected");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::AlreadyPaused)
        );
    }

    #[test]
    fn unpause_rejects_when_not_paused() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(5, 200);
        let target = &validators[1].public;
        // Validator is NOT paused.
        let err = bridge
            .unpause_validator(target, &sign_unpause(&guardian, target))
            .expect_err("unpause of non-paused must be rejected");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::NotPaused)
        );
    }

    // -----------------------------------------------------------------------
    // Unknown validator
    // -----------------------------------------------------------------------

    #[test]
    fn pause_rejects_unknown_validator() {
        let (mut bridge, _validators, guardian) = make_bridge_with_guardian(4, 200);
        let outsider = det_keypair(99);
        // The bridge knows about 10..14; outsider=99 is not in that range.
        assert!(!bridge.validators.contains_pk(&outsider.public));
        let err = bridge
            .pause_validator(&outsider.public, &sign_pause(&guardian, &outsider.public))
            .expect_err("unknown validator must be rejected");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::UnknownValidator)
        );
    }

    #[test]
    fn unpause_rejects_unknown_validator() {
        let (mut bridge, _validators, guardian) = make_bridge_with_guardian(4, 200);
        let outsider = det_keypair(99);
        let err = bridge
            .unpause_validator(&outsider.public, &sign_unpause(&guardian, &outsider.public))
            .expect_err("unknown validator must be rejected");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::UnknownValidator)
        );
    }

    // -----------------------------------------------------------------------
    // Fail-closed math
    // -----------------------------------------------------------------------

    /// Pausing must reject when it would leave zero active validators (the
    /// only case where the effective supermajority threshold `floor(2n/3)+1`
    /// exceeds `n`). The simplest scenario is `n=1`: pausing the only
    /// validator would bring active to 0, so threshold(0)=1 > 0 ⇒ reject.
    ///
    /// Note that for `n` in {3, 4, ...}, pausing one validator leaves the
    /// bridge REACHABLE because the new threshold decreases along with
    /// the active count (e.g. n=3→2, threshold(2)=2=active). The
    /// multi-pause reachability boundary is exercised by
    /// `pause_rejected_only_when_quorum_becomes_unreachable`.
    #[test]
    fn pause_rejected_when_quorum_would_become_unreachable_with_singleton() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(1, 200);
        assert_eq!(bridge.validators.threshold(), 1);
        assert_eq!(bridge.active_validator_count(), 1);

        let target = &validators[0].public;
        let err = bridge
            .pause_validator(target, &sign_pause(&guardian, target))
            .expect_err("pausing the only active validator must be rejected (quorum unreachable)");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::PauseWouldBreakQuorum)
        );
        assert!(!bridge.is_paused(target));
        assert_eq!(bridge.active_validator_count(), 1);
    }

    /// Pausing succeeds when the remaining active count still exceeds the
    /// new effective threshold. n=4, threshold=3. Pause one → active=3,
    /// new_threshold=(3*2)/3+1=3. 3 >= 3 ⇒ accept (still at-quorum).
    #[test]
    fn pause_accepted_when_active_count_meets_new_threshold() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        // Pause one validator: 4 → 3 active, threshold(3) = 3. Still at-quorum.
        let target = &validators[0].public;
        let event = bridge
            .pause_validator(target, &sign_pause(&guardian, target))
            .expect("pausing must succeed when active still reaches new threshold");
        assert_eq!(
            event,
            ValidatorEvent::Paused {
                validator: target.to_bytes().to_vec(),
                epoch: 0,
            }
        );
        assert!(bridge.is_paused(target));
        assert_eq!(bridge.active_validator_count(), 3);
        assert_eq!(bridge.effective_threshold(), 3);
    }

    /// Pausing all-but-two is fine (n=4, pause 2 → active=2 < new_threshold
    /// (2*2)/3+1 = 2); the second would NOT be rejected since 2 == 2. But
    /// pausing a third one (active=1, new_threshold=1, 1 >= 1 ⇒ accept).
    /// Pausing the last one (active=0, new_threshold=1, 0 < 1 ⇒ reject).
    #[test]
    fn pause_rejected_only_when_quorum_becomes_unreachable() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);

        bridge
            .pause_validator(&validators[0].public, &sign_pause(&guardian, &validators[0].public))
            .expect("first pause allowed");

        bridge
            .pause_validator(&validators[1].public, &sign_pause(&guardian, &validators[1].public))
            .expect("second pause: active=2, threshold(2)=2, allowed");

        // active=2, new_threshold=2. Pausing again would make active=1, threshold(1)=1 → ok.
        bridge
            .pause_validator(&validators[2].public, &sign_pause(&guardian, &validators[2].public))
            .expect("third pause: active=1, threshold(1)=1, allowed");

        // active=1, new_threshold=1. Pausing the last one would make active=0.
        // 0 < threshold(0)=1 ⇒ reject (fail-closed).
        let err = bridge
            .pause_validator(&validators[3].public, &sign_pause(&guardian, &validators[3].public))
            .expect_err("pausing the last validator must be rejected");
        assert_eq!(
            err.downcast_ref::<BridgeError>(),
            Some(&BridgeError::PauseWouldBreakQuorum)
        );
        assert!(!bridge.is_paused(&validators[3].public));
        assert_eq!(bridge.active_validator_count(), 1);
    }

    // -----------------------------------------------------------------------
    // Effect on quorum: rotation still works with paused validators
    // -----------------------------------------------------------------------

    /// A 4-validator bridge with 1 paused should still be able to rotate
    /// with 3 active signatures (the new effective threshold).
    #[test]
    fn rotation_with_one_paused_validator_uses_new_threshold() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let paused = &validators[0].public;
        bridge
            .pause_validator(paused, &sign_pause(&guardian, paused))
            .expect("pause ok");

        assert_eq!(bridge.active_validator_count(), 3);
        assert_eq!(bridge.effective_threshold(), 3);

        let next = det_keypairs_range(40, 43);
        let new_set = validator_set_from(&next);
        let epoch = 1u64;
        // Collect 3 active signatures (the new threshold). Provide 4 with
        // one of them being the paused signer — it should be silently skipped.
        let signers: Vec<&Keypair> = validators.iter().collect(); // 4 signers
        let proofs = sign_rotation(&new_set, epoch, &signers);

        bridge
            .rotate_validators(new_set, epoch, proofs)
            .expect("rotation with new threshold must succeed");

        assert_eq!(bridge.epoch, 1);
        // Rotation clears the paused set (new keys, fresh state).
        assert!(bridge.paused_list().is_empty(), "rotation must reset paused set");
    }

    /// After a non-breaking pause, providing exactly `threshold` valid
    /// signatures from the (non-paused) active set should be enough.
    #[test]
    fn rotation_uses_effective_threshold_for_size_check() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(5, 200);
        // n=5 → threshold=4. Pause 2 → active=3, new threshold=3.
        // `validators.indices` 0..5 = det_keypairs from 10..15.
        bridge
            .pause_validator(&validators[0].public, &sign_pause(&guardian, &validators[0].public))
            .unwrap();
        bridge
            .pause_validator(&validators[1].public, &sign_pause(&guardian, &validators[1].public))
            .unwrap();
        assert_eq!(bridge.active_validator_count(), 3);
        assert_eq!(bridge.effective_threshold(), 3);

        let next = det_keypairs_range(50, 53);
        let new_set = validator_set_from(&next);
        let epoch = 1u64;
        // Sign with EXACTLY 3 active validators (the new effective threshold).
        let signers: Vec<&Keypair> = validators[2..5].iter().collect();
        let proofs = sign_rotation(&new_set, epoch, &signers);

        bridge
            .rotate_validators(new_set, epoch, proofs)
            .expect("3 active signatures must meet the new threshold of 3");
        assert_eq!(bridge.epoch, 1);
    }

    /// Rotation with exactly the active count below the new threshold must fail.
    #[test]
    fn rotation_rejects_when_active_signers_below_new_threshold() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(5, 200);
        bridge
            .pause_validator(&validators[0].public, &sign_pause(&guardian, &validators[0].public))
            .unwrap();
        bridge
            .pause_validator(&validators[1].public, &sign_pause(&guardian, &validators[1].public))
            .unwrap();
        assert_eq!(bridge.effective_threshold(), 3);

        let next = det_keypairs_range(50, 53);
        let new_set = validator_set_from(&next);
        let epoch = 1u64;
        // Sign with only 2 active validators (below the new threshold of 3).
        let signers: Vec<&Keypair> = validators[2..4].iter().collect();
        let proofs = sign_rotation(&new_set, epoch, &signers);

        let result = bridge.rotate_validators(new_set, epoch, proofs);
        assert!(result.is_err(), "2 < new threshold of 3 must fail");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("insufficient quorum"), "got: {msg}");
        assert_eq!(bridge.epoch, 0, "no epoch advance on rejection");
    }

    // -----------------------------------------------------------------------
    // Paused sig handling at the verify level (silently skip, do not error)
    // -----------------------------------------------------------------------

    /// A malformed signature from a paused validator must NOT poison the
    /// entire proof batch — paused signatures are skipped silently.
    #[test]
    fn malformed_paused_signature_does_not_break_proof() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let to_pause = &validators[0].public;
        bridge
            .pause_validator(to_pause, &sign_pause(&guardian, to_pause))
            .unwrap();

        let next = det_keypairs_range(40, 43);
        let new_set = validator_set_from(&next);
        let epoch = 1u64;

        // Build a proof where the paused validator's signature slot is filled
        // with GARBAGE bytes. Despite that, the active 3 signers should be
        // enough and the proof should be accepted.
        let mut proofs: Vec<(ed25519_dalek::PublicKey, Signature)> = Vec::new();
        // Paused signer: garbage signature (any 64-byte buffer parses as a Signature
        // value; on-curve validity is verified only by `PublicKey::verify`, which
        // we deliberately never call for paused signers).
        let garbage_sig = Signature::from_bytes(&[7u8; 64])
            .expect("64-byte buffer parses as a Signature<'_> value");
        proofs.push((to_pause.clone(), garbage_sig));
        // Active signers: 3 valid signatures.
        for kp in &validators[1..4] {
            proofs.push(signed_rotation_entry(kp, &new_set, epoch));
        }

        bridge
            .rotate_validators(new_set, epoch, proofs)
            .expect("paused-signer garbage must be silently skipped, not poison the proof");
    }

    /// Duplicate paused signers are rejected before paused-signer skipping.
    #[test]
    fn duplicate_paused_signer_is_rejected_up_front() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let paused = &validators[0].public;
        bridge
            .pause_validator(paused, &sign_pause(&guardian, paused))
            .unwrap();

        let next = det_keypairs_range(40, 43);
        let new_set = validator_set_from(&next);
        let epoch = 1u64;

        // The duplicate is rejected before the paused signer can be skipped.
        let mut proofs: Vec<(ed25519_dalek::PublicKey, Signature)> = Vec::new();
        proofs.push(signed_rotation_entry(&validators[0], &new_set, epoch));
        proofs.push(signed_rotation_entry(&validators[0], &new_set, epoch));
        proofs.push(signed_rotation_entry(&validators[1], &new_set, epoch));

        let result = bridge.rotate_validators(new_set, epoch, proofs);
        assert!(result.is_err(), "duplicate paused signer must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("duplicate signer"),
            "error should mention duplicate signer, got: {msg}"
        );
    }

    fn signed_rotation_entry(
        kp: &Keypair,
        new_set: &ValidatorSet,
        epoch: u64,
    ) -> (ed25519_dalek::PublicKey, Signature) {
        let payload = Bridge::quorum_proof_payload(&[], new_set, epoch).unwrap();
        let sig = kp.sign(&payload);
        (kp.public, sig)
    }

    // -----------------------------------------------------------------------
    // Pause → unpause round trip restores effective threshold
    // -----------------------------------------------------------------------

    #[test]
    fn pause_then_unpause_restores_active_count_and_threshold() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let target = &validators[0].public;
        bridge
            .pause_validator(target, &sign_pause(&guardian, target))
            .unwrap();
        assert_eq!(bridge.active_validator_count(), 3);
        assert_eq!(bridge.effective_threshold(), 3);

        let event = bridge
            .unpause_validator(target, &sign_unpause(&guardian, target))
            .expect("unpause should succeed with valid guardian signature");
        assert_eq!(
            event,
            ValidatorEvent::Unpaused {
                validator: target.to_bytes().to_vec(),
                epoch: 0,
            }
        );
        assert!(!bridge.is_paused(target));
        assert_eq!(bridge.active_validator_count(), 4);
        assert_eq!(bridge.effective_threshold(), 3);
    }

    // -----------------------------------------------------------------------
    // Rotation resets paused set
    // -----------------------------------------------------------------------

    #[test]
    fn rotation_clears_paused_set() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        bridge
            .pause_validator(&validators[0].public, &sign_pause(&guardian, &validators[0].public))
            .unwrap();
        bridge
            .pause_validator(&validators[1].public, &sign_pause(&guardian, &validators[1].public))
            .unwrap();
        assert_eq!(bridge.paused_list().len(), 2);

        let next = det_keypairs_range(40, 44);
        let new_set = validator_set_from(&next);
        let epoch = 1u64;
        // Signers must come from the CURRENT (pre-rotation) validator set so
        // verify_quorum_proof accepts them. Two of the four are paused; the
        // effective_active = 2 and effective_threshold(2) = 2, so quorum is
        // attainable even without the paused keys' contributions.
        let active_signers: Vec<&Keypair> = validators[2..4].iter().collect();
        let proofs = sign_rotation(&new_set, epoch, &active_signers);
        bridge
            .rotate_validators(new_set, epoch, proofs)
            .expect("rotation should succeed with active=2 reaching new threshold=2");

        assert!(bridge.paused_list().is_empty(), "rotation must clear paused flags");
        assert_eq!(bridge.active_validator_count(), bridge.validators.len());
    }

    // -----------------------------------------------------------------------
    // Helper sanity
    // -----------------------------------------------------------------------

    #[test]
    fn paused_list_returns_byte_encodings_of_paused_keys() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let a = &validators[0].public;
        let b = &validators[2].public;
        bridge
            .pause_validator(a, &sign_pause(&guardian, a))
            .unwrap();
        bridge
            .pause_validator(b, &sign_pause(&guardian, b))
            .unwrap();

        let listed: std::collections::HashSet<Vec<u8>> =
            bridge.paused_list().into_iter().collect();
        assert!(listed.contains(&a.to_bytes().to_vec()));
        assert!(listed.contains(&b.to_bytes().to_vec()));
        assert_eq!(listed.len(), 2);
    }

    #[test]
    fn is_active_validator_returns_false_for_paused_or_unknown() {
        let (mut bridge, validators, guardian) = make_bridge_with_guardian(4, 200);
        let v = &validators[0].public;
        assert!(bridge.is_active_validator(v));
        bridge
            .pause_validator(v, &sign_pause(&guardian, v))
            .unwrap();
        assert!(!bridge.is_active_validator(v), "paused validator is no longer active");
        let outsider = det_keypair(99).public;
        assert!(!bridge.is_active_validator(&outsider), "non-member is not active");
    }

    #[test]
    fn validator_event_display_includes_epoch_and_hex_pk() {
        let kp = det_keypair(0);
        let pk_bytes = kp.public.to_bytes().to_vec();
        let event = ValidatorEvent::Paused {
            validator: pk_bytes.clone(),
            epoch: 7,
        };
        let rendered = format!("{event}");
        assert!(rendered.starts_with("ValidatorPaused("));
        assert!(rendered.contains("epoch=7"));
        // Spot-check that some of the bytes show up in low-case hex form.
        assert!(rendered.contains(&format!(
            "{:02x}",
            pk_bytes[0]
        )));
    }
}
