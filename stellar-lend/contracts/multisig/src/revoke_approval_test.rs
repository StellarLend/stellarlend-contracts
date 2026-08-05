#[cfg(test)]
mod revoke_approval_tests {
    use crate::{
        MultisigContract, MultisigContractClient, MultisigError, MIN_THRESHOLD_DELAY_LEDGERS,
    };
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{Address, Env, Vec};

    fn setup_with_signers(threshold: u32, signer_count: usize) -> (Env, Address, Address, Vec<Address>) {
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

    #[test]
    fn test_revoke_approval_removes_signer_and_reduces_quorum() {
        let (env, _admin, contract_id, signers) = setup_with_signers(2, 2);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();

        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal(&3, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100));

        client.approve_proposal(&signer_a, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);

        client.revoke_approval(&signer_b, &proposal_id);

        let approvals = client.get_proposal_approvals(&proposal_id).unwrap();
        assert!(!approvals.contains(&signer_b));
        assert_eq!(approvals.iter().filter(|a| *a == signer_a).count(), 1);

        env.ledger()
            .set_sequence_number(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 1);
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::InsufficientApprovals))
        );
    }

    #[test]
    fn test_revoke_nonexistent_approval_returns_error() {
        let (env, _admin, contract_id, signers) = setup_with_signers(1, 1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal(&2, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100));

        assert_eq!(
            client.try_revoke_approval(&signer_a, &proposal_id),
            Err(Ok(MultisigError::ApprovalNotFound))
        );
    }

    #[test]
    fn test_revoke_by_non_approver_is_rejected() {
        let (env, _admin, contract_id, signers) = setup_with_signers(1, 2);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();
        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal(&2, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100));

        client.approve_proposal(&signer_a, &proposal_id);

        assert_eq!(
            client.try_revoke_approval(&signer_b, &proposal_id),
            Err(Ok(MultisigError::ApprovalNotFound))
        );
    }

    #[test]
    fn test_revoke_on_expired_or_executed_proposal_is_rejected() {
        let (env, _admin, contract_id, signers) = setup_with_signers(1, 1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal(&2, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10));
        client.approve_proposal(&signer_a, &proposal_id);

        env.ledger()
            .set_sequence_number(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 20);
        assert_eq!(
            client.try_revoke_approval(&signer_a, &proposal_id),
            Err(Ok(MultisigError::ProposalExpired))
        );

        let proposal_id_2 = client.create_proposal(&2, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100));
        client.approve_proposal(&signer_a, &proposal_id_2);
        client.execute_proposal(&proposal_id_2);
        assert_eq!(
            client.try_revoke_approval(&signer_a, &proposal_id_2),
            Err(Ok(MultisigError::ProposalAlreadyExecuted))
        );
    }

    #[test]
    fn test_revoke_then_reapprove_restores_quorum() {
        let (env, _admin, contract_id, signers) = setup_with_signers(2, 2);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signer_a = signers.get(0).unwrap();
        let signer_b = signers.get(1).unwrap();
        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal(&2, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100));

        client.approve_proposal(&signer_a, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);
        client.revoke_approval(&signer_b, &proposal_id);
        client.approve_proposal(&signer_b, &proposal_id);

        let approvals = client.get_proposal_approvals(&proposal_id).unwrap();
        assert!(approvals.contains(&signer_b));
    }

    #[test]
    fn test_revoke_uninitialized_returns_error() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        let signer = Address::generate(&env);

        assert_eq!(
            client.try_revoke_approval(&signer, &1),
            Err(Ok(MultisigError::NotInitialized))
        );
    }

    #[test]
    fn test_revoke_missing_proposal_returns_error() {
        let (env, _admin, contract_id, signers) = setup_with_signers(1, 1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let signer_a = signers.get(0).unwrap();

        assert_eq!(
            client.try_revoke_approval(&signer_a, &999),
            Err(Ok(MultisigError::ProposalNotFound))
        );
    }

    #[test]
    fn test_revoke_by_not_a_signer_returns_error() {
        let (env, _admin, contract_id, signers) = setup_with_signers(1, 1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let non_signer = Address::generate(&env);
        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal(&2, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100));

        assert_eq!(
            client.try_revoke_approval(&non_signer, &proposal_id),
            Err(Ok(MultisigError::NotASigner))
        );
    }

    #[test]
    fn test_revoke_fallback_admin_signer() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &1);

        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal(&2, &(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 100));

        // Admin is the default approver on creation
        let approvals = client.get_proposal_approvals(&proposal_id).unwrap();
        assert!(approvals.contains(&admin));

        client.revoke_approval(&admin, &proposal_id);
        let approvals_after = client.get_proposal_approvals(&proposal_id).unwrap();
        assert!(!approvals_after.contains(&admin));
    }
}
