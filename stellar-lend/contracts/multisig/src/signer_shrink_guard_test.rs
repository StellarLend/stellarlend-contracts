/// Signer-shrink bricking-prevention tests for the multisig contract.
///
/// # Safety invariant under test
///
/// Applying a new signer set whose size is smaller than the current threshold
/// would permanently brick the multisig because quorum could never be reached.
/// These tests verify the documented coupling between signer-set size and the
/// live threshold at `apply_signers_change` time.
///
/// # Coverage
///
/// 1. Shrink below threshold — rejected (bricking prevented).
/// 2. Shrink to exactly threshold size — succeeds (tight but valid quorum).
/// 3. Live threshold getter is unchanged after a rejected apply.
/// 4. Queued threshold reduction landing first enables a subsequent shrink.
#[cfg(test)]
mod signer_shrink_guard_tests {
    use crate::{
        MultisigContract, MultisigContractClient, MultisigError, MIN_SIGNERS_DELAY_LEDGERS,
        MIN_THRESHOLD_DELAY_LEDGERS,
    };
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger;
    use soroban_sdk::{Address, Env, Vec};

    // -- Helpers --

    /// Build an initialised contract with `signer_count` registered signers
    /// and an active threshold of `threshold`.
    fn setup(threshold: u32, signer_count: usize) -> (Env, Address, MultisigContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);

        client.initialize(&admin, &threshold);

        let mut signers = Vec::new(&env);
        for _ in 0..signer_count {
            signers.push_back(Address::generate(&env));
        }
        client.set_signers(&signers);

        (env, admin, client)
    }

    /// Build a `Vec<Address>` of `n` freshly generated addresses.
    fn make_signers(env: &Env, n: usize) -> Vec<Address> {
        let mut v = Vec::new(env);
        for _ in 0..n {
            v.push_back(Address::generate(env));
        }
        v
    }

    /// Queue a signer-set change then advance past the delay so it is immediately applicable.
    fn queue_and_ready(env: &Env, client: &MultisigContractClient, new_signers: Vec<Address>) {
        client.queue_signers_change(&new_signers);
        let seq = env.ledger().sequence();
        env.ledger().set_sequence_number(seq + MIN_SIGNERS_DELAY_LEDGERS);
    }

    // -- Test 1: shrink below threshold is rejected --

    /// Applying a signer set whose size is less than the current threshold must
    /// be rejected to prevent permanently bricking the multisig.
    #[test]
    fn test_shrink_below_threshold_is_rejected() {
        // threshold = 3, initial signers = 5; attempting to shrink to 2.
        let (env, _admin, client) = setup(3, 5);
        assert_eq!(client.get_threshold(), 3);

        let tiny_set = make_signers(&env, 2); // 2 < 3
        queue_and_ready(&env, &client, tiny_set);

        let result = client.try_apply_signers_change();
        assert!(
            result.is_err(),
            "applying a signer set smaller than the threshold must fail"
        );
    }

    // -- Test 2: shrink to exactly threshold size succeeds --

    /// Shrinking to exactly the threshold size is the tightest valid quorum
    /// and must be accepted.
    #[test]
    fn test_shrink_to_exactly_threshold_succeeds() {
        // threshold = 3, initial signers = 5; shrink to 3.
        let (env, _admin, client) = setup(3, 5);

        let exact_set = make_signers(&env, 3); // 3 == threshold
        queue_and_ready(&env, &client, exact_set);

        client.apply_signers_change();

        assert_eq!(
            client.get_signers().unwrap().len(),
            3,
            "signer set must have exactly 3 members after shrink-to-threshold"
        );
    }

    // -- Test 3: threshold is unchanged after a rejected apply --

    /// A rejected `apply_signers_change` must leave the live threshold intact.
    #[test]
    fn test_threshold_unchanged_after_rejected_apply() {
        let (env, _admin, client) = setup(3, 4);
        let threshold_before = client.get_threshold();

        let tiny_set = make_signers(&env, 1); // 1 < 3
        queue_and_ready(&env, &client, tiny_set);

        let _ = client.try_apply_signers_change();

        assert_eq!(
            client.get_threshold(),
            threshold_before,
            "threshold must be unchanged after a rejected shrink"
        );
    }

    // -- Test 4: threshold reduction first then shrink succeeds --

    /// Reducing the threshold before applying the shrink makes the operation
    /// valid that was previously invalid.
    #[test]
    fn test_threshold_reduction_enables_subsequent_shrink() {
        // threshold = 3, signers = 5.  Reduce threshold to 2 first.
        let (env, _admin, client) = setup(3, 5);

        client.queue_threshold_change(&2);
        let seq = env.ledger().sequence();
        env.ledger().set_sequence_number(seq + MIN_THRESHOLD_DELAY_LEDGERS);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 2);

        // Now shrink to 2 — valid because threshold == 2.
        let two_signers = make_signers(&env, 2);
        let seq2 = env.ledger().sequence();
        client.queue_signers_change(&two_signers);
        env.ledger().set_sequence_number(seq2 + MIN_SIGNERS_DELAY_LEDGERS);

        client.apply_signers_change();
        assert_eq!(
            client.get_signers().unwrap().len(),
            2,
            "signer set must have 2 members after valid shrink post threshold-reduction"
        );
    }
}
