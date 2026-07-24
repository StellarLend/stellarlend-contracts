/// End-to-end test for a multisig-gated proposal: propose → approve → execute.
///
/// Simulates the full governance flow that would gate a lending upgrade:
///   1. Admin creates a proposal (analogous to queuing an upgrade).
///   2. Required signers approve the proposal.
///   3. After the time-lock elapses the proposal is executed.
///
/// Also covers negative paths: attempting to execute before quorum, before
/// the ETA, and after expiry.
#[cfg(test)]
mod upgrade_e2e_tests {
    use crate::{
        MultisigContract, MultisigContractClient, MultisigError, MIN_THRESHOLD_DELAY_LEDGERS,
    };
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger;
    use soroban_sdk::{Address, Env, Vec};

    fn setup(threshold: u32, signer_count: usize) -> (Env, Address, Address, Vec<Address>) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &id);
        client.initialize(&admin, &threshold);
        let mut signers = Vec::new(&env);
        for _ in 0..signer_count {
            signers.push_back(Address::generate(&env));
        }
        client.set_signers(&signers);
        (env, admin, id, signers)
    }

    fn advance_to_eta(env: &Env, client: &MultisigContractClient, proposal_id: u64) {
        let eta = client.get_proposal(&proposal_id).unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);
    }

    // ── Happy path: full propose → approve → execute flow ─────────────────────

    /// A proposal with threshold=1 (admin self-approves at creation) can be
    /// executed after the time-lock elapses — mirrors the simplest upgrade flow.
    #[test]
    fn test_single_signer_propose_approve_execute() {
        let (env, _admin, id, _signers) = setup(1, 1);
        let client = MultisigContractClient::new(&env, &id);

        let current = env.ledger().sequence();
        let expires = current + MIN_THRESHOLD_DELAY_LEDGERS + 200;
        let pid = client.create_proposal(&2, &expires);

        // Advance ledger past the ETA so the time-lock is satisfied.
        advance_to_eta(&env, &client, pid);

        client.execute_proposal(&pid);

        // After execution the proposal is marked done.
        let p = client.get_proposal(&pid).unwrap();
        assert!(p.executed, "Proposal must be marked executed after execute_proposal");
    }

    /// Two-of-two multisig: both signers must approve before execution succeeds.
    #[test]
    fn test_two_of_two_propose_approve_execute() {
        let (env, _admin, id, signers) = setup(2, 2);
        let client = MultisigContractClient::new(&env, &id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();

        let current = env.ledger().sequence();
        let expires = current + MIN_THRESHOLD_DELAY_LEDGERS + 200;
        let pid = client.create_proposal(&3, &expires); // admin counts as first approval

        // Only one approval so far (admin); should fail.
        advance_to_eta(&env, &client, pid);
        assert_eq!(
            client.try_execute_proposal(&pid),
            Err(Ok(MultisigError::InsufficientApprovals)),
            "Should need 2 approvals but only admin approved"
        );

        // Add second signer approval.
        client.approve_proposal(&signer_a, &pid);
        client.execute_proposal(&pid);

        let p = client.get_proposal(&pid).unwrap();
        assert!(p.executed);
        let _ = signer_b; // third signer not needed for 2-of-2
    }

    // ── Negative paths ────────────────────────────────────────────────────────

    /// Executing before the time-lock (ETA) must fail.
    #[test]
    fn test_execute_before_eta_fails() {
        let (env, _admin, id, _signers) = setup(1, 1);
        let client = MultisigContractClient::new(&env, &id);

        let current = env.ledger().sequence();
        let expires = current + MIN_THRESHOLD_DELAY_LEDGERS + 200;
        let pid = client.create_proposal(&2, &expires);

        // Do NOT advance ledger — still before ETA.
        assert_eq!(
            client.try_execute_proposal(&pid),
            Err(Ok(MultisigError::ProposalNotReady)),
            "Execution before ETA must return ProposalNotReady"
        );
    }

    /// Double execution must be rejected.
    #[test]
    fn test_double_execute_fails() {
        let (env, _admin, id, _signers) = setup(1, 1);
        let client = MultisigContractClient::new(&env, &id);

        let current = env.ledger().sequence();
        let expires = current + MIN_THRESHOLD_DELAY_LEDGERS + 200;
        let pid = client.create_proposal(&2, &expires);
        advance_to_eta(&env, &client, pid);
        client.execute_proposal(&pid);

        assert_eq!(
            client.try_execute_proposal(&pid),
            Err(Ok(MultisigError::ProposalAlreadyExecuted)),
            "Second execution must return ProposalAlreadyExecuted"
        );
    }

    /// Executing an expired proposal must fail.
    #[test]
    fn test_execute_expired_proposal_fails() {
        let (env, _admin, id, _signers) = setup(1, 1);
        let client = MultisigContractClient::new(&env, &id);

        let current = env.ledger().sequence();
        let expires = current + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let pid = client.create_proposal(&2, &expires);

        // Advance past expiry.
        env.ledger().set_sequence_number(expires + 1);

        assert_eq!(
            client.try_execute_proposal(&pid),
            Err(Ok(MultisigError::ProposalExpired)),
            "Execution after expiry ledger must return ProposalExpired"
        );
    }
}
