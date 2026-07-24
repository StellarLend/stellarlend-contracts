//! validatorset_proptest.rs — Property-based tests for `ValidatorSet` quorum invariants.
//!
//! # Proven properties
//!
//! 1. For every non-empty validator set, `threshold()` stays in `[1, len()]`.
//! 2. `contains_pk(pk)` is true exactly when `pk.to_bytes()` appears in
//!    `to_bytes_vec()`.
//! 3. Repeating the same validator key does not increase the effective
//!    validator count or quorum threshold.

#[cfg(test)]
mod tests {
    use crate::ValidatorSet;
    use ed25519_dalek::{Keypair, PublicKey, SecretKey};
    use proptest::prelude::*;
    use std::collections::HashSet;

    const MAX_SEED_SPACE: u8 = 40;
    const MAX_VALIDATORS_PER_CASE: usize = 32;

    /// Builds a deterministic keypair from a single-byte seed so proptest cases
    /// are reproducible while still spanning many distinct validator keys.
    fn deterministic_keypair(seed: u8) -> Keypair {
        let mut secret_bytes = [0u8; 32];
        secret_bytes[0] = seed.wrapping_add(1);
        for (index, byte) in secret_bytes.iter_mut().enumerate().skip(1) {
            *byte = seed
                .wrapping_mul(13)
                .wrapping_add((index as u8).wrapping_mul(17));
        }

        let secret = SecretKey::from_bytes(&secret_bytes).expect("seed must form a secret key");
        let public: PublicKey = (&secret).into();

        let mut keypair_bytes = [0u8; 64];
        keypair_bytes[..32].copy_from_slice(&secret_bytes);
        keypair_bytes[32..].copy_from_slice(public.as_bytes());

        Keypair::from_bytes(&keypair_bytes).expect("deterministic keypair must be valid")
    }

    /// Converts a seed list into the raw byte representation expected by
    /// `ValidatorSet`, preserving duplicates and order.
    fn validator_bytes_from_seeds(seeds: &[u8]) -> Vec<Vec<u8>> {
        seeds.iter()
            .map(|seed| deterministic_keypair(*seed).public.to_bytes().to_vec())
            .collect()
    }

    /// Returns the unique validator count represented by `seeds`.
    fn unique_validator_count(seeds: &[u8]) -> usize {
        validator_bytes_from_seeds(seeds)
            .into_iter()
            .collect::<HashSet<_>>()
            .len()
    }

    /// Supermajority threshold helper used to validate the contract logic.
    fn supermajority_threshold(unique_len: usize) -> usize {
        (unique_len * 2) / 3 + 1
    }

    fn validator_set_strategy() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(0..MAX_SEED_SPACE, 0..=MAX_VALIDATORS_PER_CASE)
    }

    proptest! {
        /// Non-empty sets always require at least one and at most all unique validators.
        #[test]
        fn threshold_stays_within_non_empty_bounds(seeds in validator_set_strategy()) {
            let validator_set = ValidatorSet {
                validators: validator_bytes_from_seeds(&seeds),
            };
            let unique_len = validator_set.len();

            if unique_len == 0 {
                prop_assert_eq!(validator_set.threshold(), 1);
            } else {
                prop_assert!(validator_set.threshold() >= 1);
                prop_assert!(validator_set.threshold() <= unique_len);
            }
        }

        /// Membership checks stay aligned with the raw encoded validator list.
        #[test]
        fn contains_pk_matches_to_bytes_vec(
            seeds in validator_set_strategy(),
            probe_seed in 0..MAX_SEED_SPACE
        ) {
            let raw_validators = validator_bytes_from_seeds(&seeds);
            let validator_set = ValidatorSet {
                validators: raw_validators.clone(),
            };

            let probe = deterministic_keypair(probe_seed).public;
            let probe_bytes = probe.to_bytes().to_vec();
            let expected_membership = raw_validators.iter().any(|validator| validator == &probe_bytes);

            prop_assert_eq!(validator_set.contains_pk(&probe), expected_membership);
            prop_assert_eq!(
                validator_set.to_bytes_vec().iter().any(|validator| validator == &probe_bytes),
                expected_membership
            );
        }

        /// Duplicate validator keys do not increase effective quorum size.
        #[test]
        fn duplicate_keys_do_not_inflate_threshold(seeds in validator_set_strategy()) {
            let validator_set = ValidatorSet {
                validators: validator_bytes_from_seeds(&seeds),
            };
            let unique_len = unique_validator_count(&seeds);

            prop_assert_eq!(validator_set.len(), unique_len);
            prop_assert_eq!(validator_set.threshold(), supermajority_threshold(unique_len));
            prop_assert!(validator_set.threshold() <= validator_set.len().max(1));

            if unique_len < seeds.len() {
                let raw_threshold = supermajority_threshold(seeds.len());
                prop_assert!(validator_set.threshold() <= raw_threshold);
            }
        }
    }

    /// A singleton validator set still requires exactly one signature.
    #[test]
    fn singleton_set_has_threshold_one() {
        let validators = validator_bytes_from_seeds(&[7]);
        let validator_set = ValidatorSet { validators };

        assert_eq!(validator_set.len(), 1);
        assert_eq!(validator_set.threshold(), 1);
    }

    /// Large sets keep threshold bounded by the number of unique validators.
    #[test]
    fn large_set_threshold_matches_unique_supermajority() {
        let seeds: Vec<u8> = (0..MAX_VALIDATORS_PER_CASE as u8).collect();
        let validator_set = ValidatorSet {
            validators: validator_bytes_from_seeds(&seeds),
        };

        assert_eq!(validator_set.len(), MAX_VALIDATORS_PER_CASE);
        assert_eq!(
            validator_set.threshold(),
            supermajority_threshold(MAX_VALIDATORS_PER_CASE)
        );
    }

    /// Repeated keys are preserved in the raw byte view but ignored for quorum math.
    #[test]
    fn duplicate_keys_preserve_raw_bytes_without_raising_threshold() {
        let validators = validator_bytes_from_seeds(&[1, 1, 1, 2]);
        let validator_set = ValidatorSet {
            validators: validators.clone(),
        };

        assert_eq!(validator_set.to_bytes_vec(), validators);
        assert_eq!(validator_set.len(), 2);
        assert_eq!(validator_set.threshold(), 2);
    }
}
