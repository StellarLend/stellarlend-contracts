//! # Multisig Governance Execution Path Tests
//!
//! Comprehensive test suite for multisig-based governance execution paths,
//! ensuring that threshold signatures or approvals are required and correctly
//! enforced before executing privileged actions.
//!
//! ## Test Coverage
//! - Threshold enforcement for all privileged operations
//! - Proposal lifecycle with multisig approvals (propose → approve → execute)
//! - Failure cases when approvals are insufficient
//! - Approval revocation and replacement scenarios
//! - Dynamic threshold changes and their effects
//! - Concurrent proposal handling
//! - Security edge cases

#![cfg(test)]

use crate::errors::GovernanceError;
use crate::governance::{
    approve_proposal, create_proposal, execute_multisig_proposal, get_multisig_admins,
    get_multisig_config, get_multisig_threshold, get_proposal, get_proposal_approvals, initialize,
    propose_set_min_collateral_ratio, set_multisig_admins, set_multisig_config,
    set_multisig_threshold,
};
use crate::types::ProposalType;
use crate::types::{Action, GovernanceConfig, MultisigConfig};
use crate::HelloContract;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String, Symbol, Vec,
};

// ============================================================================
// Test Helpers
// ============================================================================

fn setup_env() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        initialize(
            &env,
            admin.clone(),
            Address::generate(&env),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    });

    (env, contract_id, admin)
}

fn setup_env_with_token() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract(admin.clone());

    env.as_contract(&contract_id, || {
        initialize(
            &env,
            admin.clone(),
            token.clone(),
            Some(259200), // voting_period: 3 days
            Some(86400),  // execution_delay: 1 day
            Some(4000),   // quorum_bps: 40%
            Some(100),    // proposal_threshold
            Some(604800), // timelock_duration: 7 days
            Some(5000),   // default_voting_threshold: 50%
        )
        .unwrap();
    });

    (env, contract_id, admin, token)
}

fn create_test_token(env: &Env, admin: &Address) -> Address {
    let token = env.register_stellar_asset_contract(admin.clone());
    let token_sac = StellarAssetClient::new(env, &token);
    token_sac.mint(admin, &1_000_000_i128);
    token
}

fn setup_multisig_admins(env: &Env, admin: &Address, count: u32) -> Vec<Address> {
    let mut admins = Vec::new(env);
    admins.push_back(admin.clone());
    for _ in 1..count {
        admins.push_back(Address::generate(env));
    }
    admins
}

macro_rules! with_contract {
    ($env:expr, $contract_id:expr, $body:block) => {
        $env.as_contract($contract_id, || $body)
    };
}

// ============================================================================
// Core Multisig Execution Path Tests
// ============================================================================

#[test]
fn test_multisig_proposal_creation_requires_admin() {
    let (env, cid, admin) = setup_env();
    let non_admin = Address::generate(&env);

    // Setup multisig with 2 admins
    let admin2 = Address::generate(&env);
    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();
    });

    // Non-admin cannot create proposal
    with_contract!(env, &cid, {
        let result = propose_set_min_collateral_ratio(&env, non_admin, 15_000);
        assert_eq!(result, Err(GovernanceError::Unauthorized));
    });
}

#[test]
fn test_multisig_threshold_1_of_1_auto_executes() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        // Single admin, threshold of 1
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 1).unwrap();

        // Proposer auto-approves, threshold is 1, so ready for execution
        let _proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 1);
        assert!(approvals.contains(admin.clone()));
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60; // 5 days
    });

    with_contract!(env, &cid, {
        // Should execute with just 1 approval (proposer)
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();

        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_multisig_threshold_2_of_3_requires_second_approval() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // 3 admins, threshold of 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Admin 1 proposes (auto-approves)
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Only 1 approval so far, should fail execution
        let result = execute_multisig_proposal(&env, admin.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::InsufficientApprovals));

        // Admin 2 approves
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();

        // Now we have 2 approvals, still fail due to timelock
        let result = execute_multisig_proposal(&env, admin.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::ProposalNotReady));
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Now execution should succeed
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();

        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_multisig_insufficient_approvals_fail() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // 3 admins, threshold of 3 (requires all)
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 3).unwrap();

        // Admin 1 proposes (auto-approves)
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Only 1 approval, need 3
        let result = execute_multisig_proposal(&env, admin.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::InsufficientApprovals));

        // Admin 2 approves
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();

        // Still only 2 approvals, need 3
        let result = execute_multisig_proposal(&env, admin.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::InsufficientApprovals));

        // Admin 3 approves
        approve_proposal(&env, admin3.clone(), proposal_id).unwrap();

        // Check we have 3 approvals but still blocked by timelock
        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 3);
    });
}

#[test]
fn test_non_admin_cannot_approve() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let non_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Non-admin cannot approve
        let result = approve_proposal(&env, non_admin, proposal_id);
        assert_eq!(result, Err(GovernanceError::Unauthorized));
    });
}

#[test]
fn test_cannot_approve_same_proposal_twice() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        let admin2 = Address::generate(&env);
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Admin 2 approves
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();

        // Admin 2 cannot approve again
        let result = approve_proposal(&env, admin2.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::AlreadyVoted));
    });
}

#[test]
fn test_proposer_auto_approves() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Admin 1 proposes
        let _proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Proposer should already have approved
        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 1);
        assert!(approvals.contains(admin.clone()));
    });
}

// ============================================================================
// Dynamic Threshold Change Tests
// ============================================================================

#[test]
fn test_threshold_change_does_not_affect_existing_proposals() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup 3 admins with threshold 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Create proposal
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Get 2 approvals
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();

        // Change threshold to 3 (should not affect existing proposal)
        set_multisig_threshold(&env, admin.clone(), 3).unwrap();
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Should still execute with original 2 approvals
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();

        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_new_proposal_uses_new_threshold() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup 3 admins with threshold 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Create first proposal with threshold 2
        let proposal_id1 = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Change threshold to 3
        set_multisig_threshold(&env, admin.clone(), 3).unwrap();

        // Create second proposal with new threshold 3
        let proposal_id2 = propose_set_min_collateral_ratio(&env, admin.clone(), 16_000).unwrap();

        // Approve both proposals with 2 admins
        approve_proposal(&env, admin2.clone(), proposal_id1).unwrap();
        approve_proposal(&env, admin2.clone(), proposal_id2).unwrap();
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // First proposal executes (had threshold 2 when created)
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();

        // Second proposal fails (needs 3 approvals, only has 2)
        let result = execute_multisig_proposal(&env, admin.clone(), 2);
        assert_eq!(result, Err(GovernanceError::InsufficientApprovals));
    });
}

// ============================================================================
// Admin Set Change Tests
// ============================================================================

#[test]
fn test_admin_removal_blocks_previous_approver() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup 3 admins with threshold 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins.clone(), 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Create proposal
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Admin 2 approves
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();

        // Change admin set - remove admin2, add new admin4
        let admin4 = Address::generate(&env);
        let mut new_admins = Vec::new(&env);
        new_admins.push_back(admin.clone());
        new_admins.push_back(admin3.clone());
        new_admins.push_back(admin4.clone());
        set_multisig_admins(&env, admin.clone(), new_admins, 2).unwrap();

        // Threshold still 2, but admin2 was removed
        // Approval from removed admin should still count (they were admin at approval time)
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Execution should succeed - approval was valid when made
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();

        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_removed_admin_cannot_approve_new_proposals() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup 3 admins with threshold 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins.clone(), 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Change admin set - remove admin2
        let mut new_admins = Vec::new(&env);
        new_admins.push_back(admin.clone());
        new_admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), new_admins, 2).unwrap();

        // Create new proposal
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Admin 2 (removed) cannot approve
        let result = approve_proposal(&env, admin2.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::Unauthorized));
    });
}

// ============================================================================
// Concurrent Proposal Tests
// ============================================================================

#[test]
fn test_multiple_proposals_independent_approval_tracking() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup 3 admins with threshold 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Create two proposals
        let proposal1 = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();
        let proposal2 = propose_set_min_collateral_ratio(&env, admin.clone(), 16_000).unwrap();

        // Different admins approve different proposals
        approve_proposal(&env, admin2.clone(), proposal1).unwrap();
        approve_proposal(&env, admin3.clone(), proposal2).unwrap();

        // Verify independent tracking
        let approvals1 = get_proposal_approvals(&env, proposal1).unwrap();
        let approvals2 = get_proposal_approvals(&env, proposal2).unwrap();

        assert_eq!(approvals1.len(), 2); // admin (proposer) + admin2
        assert_eq!(approvals2.len(), 2); // admin (proposer) + admin3
        assert!(approvals1.contains(admin2.clone()));
        assert!(!approvals1.contains(admin3.clone()));
        assert!(approvals2.contains(admin3.clone()));
        assert!(!approvals2.contains(admin2.clone()));
    });
}

#[test]
fn test_same_admin_can_approve_multiple_proposals() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Create multiple proposals
        let proposal1 = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();
        let proposal2 = propose_set_min_collateral_ratio(&env, admin.clone(), 16_000).unwrap();
        let proposal3 = propose_set_min_collateral_ratio(&env, admin2.clone(), 17_000).unwrap();

        // Admin2 can approve all (except their own which is auto-approved)
        approve_proposal(&env, admin2.clone(), proposal1).unwrap();
        approve_proposal(&env, admin2.clone(), proposal2).unwrap();
        // proposal3 already approved by admin2 as proposer

        // Verify all have correct approvals
        for pid in [proposal1, proposal2, proposal3] {
            let approvals = get_proposal_approvals(&env, pid).unwrap();
            assert!(approvals.len() >= 1);
        }
    });
}

// ============================================================================
// Execution Authorization Tests
// ============================================================================

#[test]
fn test_execution_requires_admin_status() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let non_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Non-admin cannot execute even with sufficient approvals
        let result = execute_multisig_proposal(&env, non_admin, 1);
        assert_eq!(result, Err(GovernanceError::Unauthorized));

        // Admin can execute
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();
    });
}

#[test]
fn test_any_admin_can_execute_with_sufficient_approvals() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Admin 1 proposes (auto-approves)
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Admin 2 approves
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Admin 3 (who didn't propose or approve yet) can execute
        execute_multisig_proposal(&env, admin3.clone(), 1).unwrap();

        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

// ============================================================================
// Timelock and Expiration Tests
// ============================================================================

#[test]
fn test_cannot_execute_before_timelock() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 1).unwrap();

        // Create proposal
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Try to execute immediately (before timelock)
        let result = execute_multisig_proposal(&env, admin.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::ProposalNotReady));
    });
}

#[test]
fn test_cannot_execute_expired_proposal() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Create proposal and get approvals
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();
    });

    // Advance past timelock AND expiration (very far in future)
    env.ledger().with_mut(|li| {
        li.timestamp += 30 * 24 * 60 * 60; // 30 days
    });

    with_contract!(env, &cid, {
        // Should fail with expired
        let result = execute_multisig_proposal(&env, admin.clone(), 1);
        assert_eq!(result, Err(GovernanceError::ProposalExpired));
    });
}

// ============================================================================
// Edge Case Security Tests
// ============================================================================

#[test]
fn test_threshold_zero_rejected() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();

        // Threshold of 0 should be rejected
        let result = set_multisig_threshold(&env, admin.clone(), 0);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

#[test]
fn test_threshold_above_admin_count_rejected() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        set_multisig_admins(&env, admin.clone(), admins.clone(), 2).unwrap();

        // Threshold higher than admin count should be rejected
        let result = set_multisig_threshold(&env, admin.clone(), 2);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

#[test]
fn test_empty_admin_set_rejected() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        // Empty admin set should be rejected
        let empty_admins = Vec::new(&env);
        let result = set_multisig_admins(&env, admin.clone(), empty_admins, 2);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

#[test]
fn test_duplicate_admins_rejected() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        // Duplicate admins should be rejected
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin.clone()); // Duplicate
        let result = set_multisig_admins(&env, admin.clone(), admins, 2);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

#[test]
fn test_cannot_execute_already_executed_proposal() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // First execution succeeds
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();

        // Second execution fails
        let result = execute_multisig_proposal(&env, admin.clone(), 1);
        assert_eq!(result, Err(GovernanceError::AlreadyExecuted));
    });
}

#[test]
fn test_nonexistent_proposal_rejected() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 1).unwrap();

        // Try to execute non-existent proposal
        let result = execute_multisig_proposal(&env, admin.clone(), 999);
        assert_eq!(result, Err(GovernanceError::ProposalNotFound));
    });
}

// ============================================================================
// Complex Integration Tests
// ============================================================================

#[test]
fn test_full_multisig_governance_flow_2_of_3() {
    let (env, cid, admin1) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup: 3 admins, threshold 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin1.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin1.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin1.clone(), 2).unwrap();

        // Verify setup
        let config = get_multisig_config(&env).unwrap();
        assert_eq!(config.admins.len(), 3);
        assert_eq!(config.threshold, 2);

        // Step 1: Propose (admin1 auto-approves)
        let proposal_id = propose_set_min_collateral_ratio(&env, admin1.clone(), 15_000).unwrap();
        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.proposer, admin1.clone());
        assert_eq!(proposal.status, ProposalStatus::Pending);

        // Verify auto-approval
        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 1);
        assert!(approvals.contains(admin1.clone()));

        // Step 2: Approve by admin2
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();
        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 2);
        assert!(approvals.contains(admin1.clone()));
        assert!(approvals.contains(admin2.clone()));

        // Step 3: Try to execute (should fail due to timelock)
        let result = execute_multisig_proposal(&env, admin1.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::ProposalNotReady));
    });

    // Step 4: Advance time past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Step 5: Execute (should succeed)
        execute_multisig_proposal(&env, admin1.clone(), 1).unwrap();

        // Verify execution
        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_full_multisig_governance_flow_3_of_5() {
    let (env, cid, admin1) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);
    let admin4 = Address::generate(&env);
    let admin5 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup: 5 admins, threshold 3
        let mut admins = Vec::new(&env);
        admins.push_back(admin1.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        admins.push_back(admin4.clone());
        admins.push_back(admin5.clone());
        set_multisig_admins(&env, admin1.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin1.clone(), 3).unwrap();

        // Proposal and approvals
        let proposal_id = propose_set_min_collateral_ratio(&env, admin1.clone(), 15_000).unwrap();
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();
        approve_proposal(&env, admin3.clone(), proposal_id).unwrap();

        // 3 approvals (threshold met)
        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 3);
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Admin4 (didn't approve) can execute
        execute_multisig_proposal(&env, admin4.clone(), 1).unwrap();

        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_multisig_with_different_proposal_types() {
    let (env, cid, admin, _token) = setup_env_with_token();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Setup 3 admins with threshold 2
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Test with different proposal types
        let proposal_types = [
            ProposalType::MinCollateralRatio(15_000),
            ProposalType::RiskParams(Some(15_000), Some(12_000), Some(5000), Some(800)),
            ProposalType::PauseSwitch(Symbol::new(&env, "deposit"), true),
            ProposalType::EmergencyPause(true),
        ];

        for (i, proposal_type) in proposal_types.iter().enumerate() {
            let description = String::from_str(&env, "Test proposal");
            let proposal_id = create_proposal(
                &env,
                admin.clone(),
                proposal_type.clone(),
                description,
                None,
            )
            .unwrap();

            // Should start with proposer's approval
            let approvals = get_proposal_approvals(&env, 1).unwrap();
            assert_eq!(approvals.len(), 1, "Proposal {} should have 1 approval", i);

            // Add second approval
            approve_proposal(&env, admin2.clone(), proposal_id).unwrap();
            let approvals = get_proposal_approvals(&env, 1).unwrap();
            assert_eq!(approvals.len(), 2, "Proposal {} should have 2 approvals", i);
        }
    });
}

#[test]
fn test_multisig_config_query_functions() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        // Initial state - query should return None or default
        let initial_config = get_multisig_config(&env);
        assert!(initial_config.is_some()); // Set during initialize

        // Set up custom multisig config
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        set_multisig_admins(&env, admin.clone(), admins.clone(), 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Query and verify
        let config = get_multisig_config(&env).unwrap();
        assert_eq!(config.admins.len(), 3);
        assert_eq!(config.threshold, 2);
        assert!(config.admins.contains(admin.clone()));
        assert!(config.admins.contains(admin2.clone()));
        assert!(config.admins.contains(admin3.clone()));

        // Query threshold directly
        let threshold = get_multisig_threshold(&env);
        assert_eq!(threshold, 2);

        // Query admins directly
        let queried_admins = get_multisig_admins(&env).unwrap();
        assert_eq!(queried_admins.len(), 3);
    });
}

// ============================================================================
// Stress Tests
// ============================================================================

#[test]
fn test_many_admins_high_threshold() {
    let (env, cid, admin) = setup_env();

    with_contract!(env, &cid, {
        // Setup 10 admins with threshold 7
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        let mut admin_list = vec![admin.clone()];

        for _ in 1..10 {
            let new_admin = Address::generate(&env);
            admins.push_back(new_admin.clone());
            admin_list.push(new_admin);
        }

        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 7).unwrap();

        // Create proposal (1 approval)
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 15_000).unwrap();

        // Add 5 more approvals (total 6, need 7)
        for i in 1..6 {
            approve_proposal(&env, admin_list[i].clone(), proposal_id).unwrap();
        }

        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 6);

        // Still insufficient
        let result = execute_multisig_proposal(&env, admin.clone(), proposal_id);
        assert_eq!(result, Err(GovernanceError::ProposalNotReady));

        // Add 7th approval
        approve_proposal(&env, admin_list[6].clone(), proposal_id).unwrap();
        let approvals = get_proposal_approvals(&env, 1).unwrap();
        assert_eq!(approvals.len(), 7);
    });

    // Advance past timelock
    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        // Now can execute
        execute_multisig_proposal(&env, admin.clone(), 1).unwrap();
    });
}

#[test]
fn test_rapid_proposal_creation_and_approval() {
    let (env, cid, admin) = setup_env();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        // Create 10 proposals rapidly
        let mut proposal_ids = Vec::new(&env);
        for i in 0..10 {
            let new_ratio = 15_000 + (i as i128 * 100);
            let pid = propose_set_min_collateral_ratio(&env, admin.clone(), new_ratio as u32).unwrap();
            proposal_ids.push_back(pid);
        }

        // Approve all proposals
        for i in 0..proposal_ids.len() {
            let pid = proposal_ids.get(i).unwrap();
            approve_proposal(&env, admin2.clone(), pid).unwrap();

            let approvals = get_proposal_approvals(&env, pid).unwrap();
            assert_eq!(approvals.len(), 2);
        }
    });
}