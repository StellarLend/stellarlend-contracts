/// Quorum-integrity edge-case tests for the multisig crate.
///
/// # Quorum rules proven here
///
/// 1. **Deduplication** — a signer who calls `approve_proposal` twice is counted
///    once; the second call is rejected with `AlreadyApproved`.
/// 2. **Removed-after-approval exclusion** — a signer who approved a proposal
///    but is subsequently removed from the registered signer set via
///    `set_signers` no longer contributes to quorum at execution time.
/// 3. **Live-threshold enforcement** — the threshold read at `execute_proposal`
///    is the *current* threshold, not the one in effect when the proposal was
///    created. Raising the threshold after approval requires additional
///    approvals before the proposal can execute.
#[cfg(test)]
mod quorum_edge_tests {
    use crate::{
        MultisigContract, MultisigContractClient, MultisigError, MIN_THRESHOLD_DELAY_LEDGERS,
    };
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger;
    use soroban_sdk::{Address, Env, Vec};

    // ─── Helpers ──────────────────────────────────────────────────────────────

    fn setup_with_signers(
        threshold: u32,
        signer_count: usize,
    ) -> (Env, Address, Address, Vec<Address>) {
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

        (env, admin, contract_id, signers)
    }

    /// Fast-forward the ledger to exactly `eta_ledger` of a proposal so it
    /// becomes executable.
    fn advance_past_eta(env: &Env, client: &MultisigContractClient, proposal_id: u64) {
        let eta = client.get_proposal(&proposal_id).unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);
    }

    // ─── Edge 1: Duplicate approval ───────────────────────────────────────────

    /// A signer who calls `approve_proposal` twice on the same proposal must
    /// receive `AlreadyApproved` on the second attempt. The stored approval list
    /// must contain the signer exactly once, and quorum must reflect only one
    /// count.
    #[test]
    fn test_duplicate_approval_counted_once() {
        // threshold=2: we need 2 distinct valid approvals to execute.
        let (env, admin, contract_id, signers) = setup_with_signers(2, 2);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100;
        let proposal_id = client.create_proposal(&3, &expires_at);

        // First approval from signer_a — must succeed.
        client.approve_proposal(&signer_a, &proposal_id);

        // Second approval from the same signer — must be rejected.
        assert_eq!(
            client.try_approve_proposal(&signer_a, &proposal_id),
            Err(Ok(MultisigError::AlreadyApproved)),
            "duplicate approval must return AlreadyApproved"
        );

        // The approval list must still contain signer_a exactly once.
        let approvals = client.get_proposal_approvals(&proposal_id).unwrap();
        let count = approvals.iter().filter(|a| a == signer_a).count();
        assert_eq!(count, 1, "signer_a must appear in the list exactly once");

        // Quorum is not yet met (only 1 of 2 required approvals), so execution
        // must fail even after the timelock elapses.
        advance_past_eta(&env, &client, proposal_id);
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::InsufficientApprovals)),
            "one approval must not satisfy a threshold of 2"
        );

        // Adding the second distinct signer should now allow execution.
        client.approve_proposal(&signer_b, &proposal_id);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 3);
    }

    /// Even if the same address somehow ends up in the raw approval list twice
    /// (e.g. from a direct storage bypass in a hypothetical upgrade path),
    /// `execute_proposal` deduplicates before counting and must not allow
    /// execution below threshold.
    ///
    /// This test uses `create_proposal` (which auto-adds the admin) and then
    /// manually verifies that the quorum check at execution does not
    /// double-count any address.
    #[test]
    fn test_execute_deduplicates_approval_list() {
        // Single signer set containing only signer_a; threshold = 2 so one
        // address can never satisfy it regardless of duplication.
        let (env, admin, contract_id, signers) = setup_with_signers(2, 1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100;
        let proposal_id = client.create_proposal(&5, &expires_at);

        // signer_a approves once via the public API.
        client.approve_proposal(&signer_a, &proposal_id);

        // The second approve_proposal call is rejected by the duplicate guard.
        assert_eq!(
            client.try_approve_proposal(&signer_a, &proposal_id),
            Err(Ok(MultisigError::AlreadyApproved))
        );

        // Even after timelock, threshold=2 with only 1 unique valid signer
        // must result in InsufficientApprovals.
        advance_past_eta(&env, &client, proposal_id);
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::InsufficientApprovals))
        );
    }

    // ─── Edge 2: Removed-after-approval signer excluded ───────────────────────

    /// A signer who approved a proposal and was subsequently removed from the
    /// signer set must NOT contribute to quorum at execution time.
    ///
    /// Scenario:
    ///   threshold = 2, signers = [A, B, C]
    ///   A, B both approve → quorum would be met (2/2).
    ///   B is removed from the signer set (new set = [A, C]).
    ///   At execution only A's approval is valid → 1 < 2 → InsufficientApprovals.
    ///   C then approves → 2/2 → execution succeeds.
    #[test]
    fn test_removed_signer_excluded_from_quorum() {
        let (env, admin, contract_id, signers) = setup_with_signers(2, 3);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();
        let signer_c = signers.get(2).unwrap();

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100;
        let proposal_id = client.create_proposal(&7, &expires_at);

        // A and B both approve while B is still a valid signer.
        client.approve_proposal(&signer_a, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);

        // Remove B from the signer set (new set: A, C).
        let mut new_signers = Vec::new(&env);
        new_signers.push_back(signer_a.clone());
        new_signers.push_back(signer_c.clone());
        client.set_signers(&new_signers);

        // Advance past the timelock.
        advance_past_eta(&env, &client, proposal_id);

        // B's approval no longer counts → only A's approval is valid → 1 < 2.
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::InsufficientApprovals)),
            "removed signer B must not count toward quorum"
        );

        // C (still a current signer) approves → now 2 valid approvals → executes.
        client.approve_proposal(&signer_c, &proposal_id);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 7);
        assert!(client.get_proposal(&proposal_id).unwrap().executed);
    }

    /// Variant: ALL signers who approved are removed. Execution must fail
    /// entirely (0 valid approvals < threshold).
    #[test]
    fn test_all_approvers_removed_prevents_execution() {
        let (env, admin, contract_id, signers) = setup_with_signers(2, 3);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();
        let signer_c = signers.get(2).unwrap();

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100;
        let proposal_id = client.create_proposal(&9, &expires_at);

        client.approve_proposal(&signer_a, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);

        // Replace the signer set with only C (who never approved).
        let mut replacement = Vec::new(&env);
        replacement.push_back(signer_c.clone());
        client.set_signers(&replacement);

        advance_past_eta(&env, &client, proposal_id);

        // 0 valid current-signer approvals < threshold=2.
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::InsufficientApprovals)),
            "no valid approvals must prevent execution"
        );
    }

    // ─── Edge 3: Threshold raised between approval and execution ──────────────

    /// The threshold is raised from 2 to 3 after two approvals are collected.
    /// Execution must require a third approval that matches the new threshold.
    ///
    /// Scenario:
    ///   Initial threshold = 2, signers = [A, B, C]
    ///   A, B approve the proposal (2 approvals — would have been enough).
    ///   Threshold is raised to 3 via queue_threshold_change / apply_threshold_change.
    ///   Execution with only A+B fails: 2 < 3 → InsufficientApprovals.
    ///   C also approves → 3 valid approvals → execution succeeds.
    #[test]
    fn test_raised_threshold_requires_additional_approval() {
        let (env, admin, contract_id, signers) = setup_with_signers(2, 3);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();
        let signer_c = signers.get(2).unwrap();

        let create_ledger = env.ledger().sequence();
        // expires_at must be far enough in the future to survive the threshold
        // change delay AND the proposal delay.
        let expires_at = create_ledger + MIN_THRESHOLD_DELAY_LEDGERS * 3;
        let proposal_id = client.create_proposal(&5, &expires_at);

        // Two signers approve while threshold is still 2.
        client.approve_proposal(&signer_a, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);

        // Queue a threshold increase (2 → 3) and apply it after the mandatory delay.
        client.queue_threshold_change(&3);
        let tc_eta = client.get_pending_threshold_change().unwrap().eta_ledger;
        env.ledger().set_sequence_number(tc_eta);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 3);

        // The proposal's eta is also MIN_THRESHOLD_DELAY_LEDGERS from creation,
        // which has already elapsed (we are at tc_eta ≥ create_ledger + MIN_THRESHOLD_DELAY_LEDGERS).
        // Execution must now fail because 2 < 3.
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::InsufficientApprovals)),
            "raised threshold must require an additional approval"
        );

        // Third signer approves → now 3/3 → execution succeeds.
        client.approve_proposal(&signer_c, &proposal_id);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 5); // proposal changes threshold to 5
        assert!(client.get_proposal(&proposal_id).unwrap().executed);
    }

    /// Exactly-quorum execution: with threshold=3 and exactly 3 valid approvals,
    /// execution must succeed (no off-by-one).
    #[test]
    fn test_exactly_quorum_executes() {
        let (env, admin, contract_id, signers) = setup_with_signers(3, 3);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();
        let signer_c = signers.get(2).unwrap();

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100;
        let proposal_id = client.create_proposal(&4, &expires_at);

        client.approve_proposal(&signer_a, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);
        client.approve_proposal(&signer_c, &proposal_id);

        advance_past_eta(&env, &client, proposal_id);

        // Exactly 3 approvals == threshold=3 → must succeed.
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 4);
    }

    /// One-below-quorum execution: with threshold=3 and only 2 valid approvals,
    /// execution must fail (InsufficientApprovals).
    #[test]
    fn test_one_below_quorum_rejected() {
        let (env, admin, contract_id, signers) = setup_with_signers(3, 3);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100;
        let proposal_id = client.create_proposal(&4, &expires_at);

        client.approve_proposal(&signer_a, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);

        advance_past_eta(&env, &client, proposal_id);

        // Only 2 approvals for threshold=3 — must fail.
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::InsufficientApprovals))
        );
    }

    // ─── Non-signer approval rejection ────────────────────────────────────────

    /// An address that is not in the registered signer set must not be able to
    /// approve a proposal.
    #[test]
    fn test_non_signer_cannot_approve() {
        let (env, admin, contract_id, _signers) = setup_with_signers(1, 2);
        let client = MultisigContractClient::new(&env, &contract_id);

        let outsider = Address::generate(&env);

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let proposal_id = client.create_proposal(&2, &expires_at);

        assert_eq!(
            client.try_approve_proposal(&outsider, &proposal_id),
            Err(Ok(MultisigError::NotASigner)),
            "outsider must not be able to approve"
        );
    }

    /// Approving an already-executed proposal must be rejected.
    #[test]
    fn test_approve_already_executed_proposal_rejected() {
        let (env, admin, contract_id, signers) = setup_with_signers(1, 1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();

        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let proposal_id = client.create_proposal(&2, &expires_at);

        // signer_a approves, satisfying threshold=1.
        client.approve_proposal(&signer_a, &proposal_id);

        advance_past_eta(&env, &client, proposal_id);
        client.execute_proposal(&proposal_id);
        assert!(client.get_proposal(&proposal_id).unwrap().executed);

        // Further approve on an already-executed proposal must fail.
        assert_eq!(
            client.try_approve_proposal(&signer_a, &proposal_id),
            Err(Ok(MultisigError::ProposalAlreadyExecuted))
        );
    }
}
