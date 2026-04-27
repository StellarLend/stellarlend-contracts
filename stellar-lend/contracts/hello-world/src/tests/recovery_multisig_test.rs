//! # Recovery and Multisig Test Suite
//!
//! Comprehensive tests for guardian-based social recovery and multisig governance.
//!
//! ## Test Coverage
//! ### Recovery:
//! - Guardian management (add, remove, threshold)
//! - Recovery lifecycle (start, approve, execute)
//! - Authorization and access control
//! - Edge cases (expiration, duplicate approvals, insufficient approvals)
//!
//! ### Multisig:
//! - Admin management (set admins, set threshold)
//! - Proposal lifecycle (propose, approve, execute)
//! - Threshold enforcement
//! - Complex scenarios (parallel proposals, admin rotation)
#![allow(unused_variables)]
#![cfg(test)]

use crate::governance::*;
use crate::HelloContract;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol, Vec,
};

// ============================================================================
// Test Helpers
// ============================================================================

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        initialize_governance(&env, admin.clone()).unwrap();
    });

    (env, contract_id, admin)
}

macro_rules! with_contract {
    ($env:expr, $contract_id:expr, $body:block) => {
        $env.as_contract($contract_id, || $body)
    };
}

// ============================================================================
// Guardian Management Tests
// ============================================================================

#[test]
fn test_add_guardian_success() {
    let (env, cid, admin) = setup();
    let guardian = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin, guardian.clone()).unwrap();
        let guardians = get_guardians(&env).unwrap();
        assert_eq!(guardians.len(), 1);
        assert_eq!(guardians.get(0).unwrap(), guardian);
    });
}

#[test]
fn test_add_guardian_unauthorized() {
    let (env, cid, _admin) = setup();
    let non_admin = Address::generate(&env);
    let guardian = Address::generate(&env);

    with_contract!(env, &cid, {
        let result = add_guardian(&env, non_admin, guardian);
        assert_eq!(result, Err(GovernanceError::Unauthorized));
    });
}

#[test]
fn test_add_guardian_duplicate() {
    let (env, cid, admin) = setup();
    let guardian = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), guardian.clone()).unwrap();
        let result = add_guardian(&env, admin, guardian);
        assert_eq!(result, Err(GovernanceError::GuardianAlreadyExists));
    });
}

#[test]
fn test_remove_guardian_success() {
    let (env, cid, admin) = setup();
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), g1.clone()).unwrap();
        add_guardian(&env, admin.clone(), g2.clone()).unwrap();
        remove_guardian(&env, admin, g1).unwrap();

        let guardians = get_guardians(&env).unwrap();
        assert_eq!(guardians.len(), 1);
        assert_eq!(guardians.get(0).unwrap(), g2);
    });
}

#[test]
fn test_set_guardian_threshold() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        for _ in 0..3 {
            add_guardian(&env, admin.clone(), Address::generate(&env)).unwrap();
        }
        set_guardian_threshold(&env, admin, 2).unwrap();
        assert_eq!(get_guardian_threshold(&env), 2);
    });
}

#[test]
fn test_set_guardian_threshold_invalid() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), Address::generate(&env)).unwrap();

        let result = set_guardian_threshold(&env, admin.clone(), 0);
        assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));

        let result = set_guardian_threshold(&env, admin, 5);
        assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));
    });
}

// ============================================================================
// Recovery Lifecycle Tests
// ============================================================================

#[test]
fn test_start_recovery_success() {
    let (env, cid, admin) = setup();
    let guardian = Address::generate(&env);
    let new_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), guardian.clone()).unwrap();
        start_recovery(&env, guardian.clone(), admin.clone(), new_admin.clone()).unwrap();

        let recovery = get_recovery_request(&env).unwrap();
        assert_eq!(recovery.old_admin, admin);
        assert_eq!(recovery.new_admin, new_admin);
        assert_eq!(recovery.initiator, guardian);

        let approvals = get_recovery_approvals(&env).unwrap();
        assert_eq!(approvals.len(), 1);
    });
}

#[test]
fn test_start_recovery_unauthorized() {
    let (env, cid, admin) = setup();
    let non_guardian = Address::generate(&env);
    let new_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        let result = start_recovery(&env, non_guardian, admin, new_admin);
        assert_eq!(result, Err(GovernanceError::Unauthorized));
    });
}

#[test]
fn test_approve_recovery_success() {
    let (env, cid, admin) = setup();
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let new_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), g1.clone()).unwrap();
        add_guardian(&env, admin.clone(), g2.clone()).unwrap();
        start_recovery(&env, g1.clone(), admin, new_admin).unwrap();
        approve_recovery(&env, g2.clone()).unwrap();

        let approvals = get_recovery_approvals(&env).unwrap();
        assert_eq!(approvals.len(), 2);
        assert!(approvals.contains(g1));
        assert!(approvals.contains(g2));
    });
}

#[test]
fn test_approve_recovery_duplicate() {
    let (env, cid, admin) = setup();
    let guardian = Address::generate(&env);
    let new_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), guardian.clone()).unwrap();
        start_recovery(&env, guardian.clone(), admin, new_admin).unwrap();

        let result = approve_recovery(&env, guardian);
        assert_eq!(result, Err(GovernanceError::AlreadyVoted));
    });
}

#[test]
fn test_execute_recovery_success() {
    let (env, cid, admin) = setup();
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let executor = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), g1.clone()).unwrap();
        add_guardian(&env, admin.clone(), g2.clone()).unwrap();
        set_guardian_threshold(&env, admin.clone(), 2).unwrap();

        start_recovery(&env, g1, admin.clone(), new_admin.clone()).unwrap();
        approve_recovery(&env, g2).unwrap();
        execute_recovery(&env, executor).unwrap();

        let admins = get_multisig_admins(&env).unwrap();
        assert!(!admins.contains(admin));
        assert!(admins.contains(new_admin));
        assert!(get_recovery_request(&env).is_none());
    });
}

#[test]
fn test_execute_recovery_insufficient_approvals() {
    let (env, cid, admin) = setup();
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let g3 = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let executor = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), g1.clone()).unwrap();
        add_guardian(&env, admin.clone(), g2).unwrap();
        add_guardian(&env, admin.clone(), g3).unwrap();
        set_guardian_threshold(&env, admin.clone(), 3).unwrap();

        start_recovery(&env, g1, admin, new_admin).unwrap();

        let result = execute_recovery(&env, executor);
        assert_eq!(result, Err(GovernanceError::InsufficientApprovals));
    });
}

#[test]
fn test_recovery_expiration() {
    let (env, cid, admin) = setup();
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let new_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), g1.clone()).unwrap();
        add_guardian(&env, admin.clone(), g2.clone()).unwrap();
        start_recovery(&env, g1, admin, new_admin).unwrap();
    });

    env.ledger().with_mut(|li| {
        li.timestamp += 3 * 24 * 60 * 60 + 1;
    });

    with_contract!(env, &cid, {
        let result = approve_recovery(&env, g2);
        assert_eq!(result, Err(GovernanceError::ProposalExpired));
        assert!(get_recovery_request(&env).is_none());
    });
}

// ============================================================================
// Multisig Admin Management Tests
// ============================================================================

#[test]
fn test_set_multisig_admins_success() {
    let (env, cid, admin) = setup();
    let new_admin1 = Address::generate(&env);
    let new_admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut new_admins = Vec::new(&env);
        new_admins.push_back(new_admin1.clone());
        new_admins.push_back(new_admin2.clone());

        set_multisig_admins(&env, admin, new_admins, 2).unwrap();

        let stored_admins = get_multisig_admins(&env).unwrap();
        assert_eq!(stored_admins.len(), 2);
        assert!(stored_admins.contains(new_admin1));
        assert!(stored_admins.contains(new_admin2));
    });
}

#[test]
fn test_set_multisig_admins_empty() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        let empty_admins = Vec::new(&env);
        let result = set_multisig_admins(&env, admin, empty_admins, 2);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

#[test]
fn test_set_multisig_threshold_success() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        for _ in 0..2 {
            admins.push_back(Address::generate(&env));
        }
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin, 2).unwrap();
        assert_eq!(get_multisig_threshold(&env), 2);
    });
}

#[test]
fn test_set_multisig_threshold_invalid() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        let result = set_multisig_threshold(&env, admin.clone(), 0);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));

        let result = set_multisig_threshold(&env, admin, 5);
        assert_eq!(result, Err(GovernanceError::InvalidMultisigConfig));
    });
}

// ============================================================================
// Proposal Lifecycle Tests
// ============================================================================

#[test]
fn test_create_proposal_success() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        let proposal_type = ProposalType::MinCollateralRatio(12_000);
        let description = Symbol::new(&env, "increase_mcr");

        let proposal_id = create_proposal(
            &env,
            admin.clone(),
            proposal_type.clone(),
            description.clone(),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(proposal_id, 1);
        let proposal = get_proposal(&env, proposal_id).unwrap();
        assert_eq!(proposal.id, proposal_id);
        assert_eq!(proposal.proposer, admin);
        assert_eq!(proposal.proposal_type, proposal_type);
        assert_eq!(proposal.status, ProposalStatus::Active);
    });
}

#[test]
fn test_propose_unauthorized() {
    let (env, cid, _admin) = setup();
    let non_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        let result = propose_set_min_collateral_ratio(&env, non_admin, 12_000);
        assert_eq!(result, Err(GovernanceError::Unauthorized));
    });
}

#[test]
fn test_approve_proposal_success() {
    let (env, cid, admin) = setup();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin, 12_000).unwrap();
        approve_proposal(&env, admin2.clone(), proposal_id).unwrap();

        let approvals = get_proposal_approvals(&env, proposal_id).unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals.get(0).unwrap(), admin2);
    });
}

#[test]
fn test_approve_proposal_duplicate() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 12_000).unwrap();
        approve_proposal(&env, admin.clone(), proposal_id).unwrap();

        let result = approve_proposal(&env, admin, proposal_id);
        assert_eq!(result, Err(GovernanceError::AlreadyVoted));
    });
}

#[test]
fn test_execute_multisig_proposal_success() {
    let (env, cid, admin) = setup();
    let admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 2).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 12_000).unwrap();
        approve_proposal(&env, admin.clone(), proposal_id).unwrap();
        approve_proposal(&env, admin2, proposal_id).unwrap();
    });

    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        execute_multisig_proposal(&env, admin, 1).unwrap();
        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_execute_multisig_proposal_insufficient_approvals() {
    let (env, cid, admin) = setup();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3);
        set_multisig_admins(&env, admin.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin.clone(), 3).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 12_000).unwrap();
        approve_proposal(&env, admin.clone(), proposal_id).unwrap();
        approve_proposal(&env, admin2, proposal_id).unwrap();

        let result = execute_multisig_proposal(&env, admin, proposal_id);
        assert_eq!(result, Err(GovernanceError::InsufficientApprovals));
    });
}

#[test]
fn test_execute_multisig_proposal_timelock_not_expired() {
    let (env, cid, admin) = setup();

    with_contract!(env, &cid, {
        let proposal_id = propose_set_min_collateral_ratio(&env, admin.clone(), 12_000).unwrap();
        approve_proposal(&env, admin.clone(), proposal_id).unwrap();

        let result = execute_multisig_proposal(&env, admin, proposal_id);
        assert_eq!(result, Err(GovernanceError::ProposalNotReady));
    });
}

// ============================================================================
// Complex Scenarios
// ============================================================================

#[test]
fn test_full_multisig_flow_3_of_5() {
    let (env, cid, admin1) = setup();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);
    let admin4 = Address::generate(&env);
    let admin5 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin1.clone());
        admins.push_back(admin2.clone());
        admins.push_back(admin3.clone());
        admins.push_back(admin4);
        admins.push_back(admin5);
        set_multisig_admins(&env, admin1.clone(), admins, 2).unwrap();
        set_multisig_threshold(&env, admin1.clone(), 3).unwrap();

        let proposal_id = propose_set_min_collateral_ratio(&env, admin1.clone(), 12_000).unwrap();
        approve_proposal(&env, admin1.clone(), proposal_id).unwrap();
        approve_proposal(&env, admin2, proposal_id).unwrap();
        approve_proposal(&env, admin3, proposal_id).unwrap();

        let approvals = get_proposal_approvals(&env, proposal_id).unwrap();
        assert_eq!(approvals.len(), 3);
    });

    env.ledger().with_mut(|li| {
        li.timestamp += 5 * 24 * 60 * 60;
    });

    with_contract!(env, &cid, {
        execute_multisig_proposal(&env, admin1, 1).unwrap();
        let proposal = get_proposal(&env, 1).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    });
}

#[test]
fn test_admin_rotation() {
    let (env, cid, old_admin) = setup();
    let new_admin1 = Address::generate(&env);
    let new_admin2 = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut new_admins = Vec::new(&env);
        new_admins.push_back(new_admin1.clone());
        new_admins.push_back(new_admin2.clone());
        set_multisig_admins(&env, old_admin.clone(), new_admins, 2).unwrap();

        let stored_admins = get_multisig_admins(&env).unwrap();
        assert!(stored_admins.contains(new_admin1.clone()));
        assert!(stored_admins.contains(new_admin2));
        assert!(!stored_admins.contains(old_admin.clone()));

        let result = propose_set_min_collateral_ratio(&env, old_admin, 12_000);
        assert_eq!(result, Err(GovernanceError::Unauthorized));

        let proposal_id = propose_set_min_collateral_ratio(&env, new_admin1, 12_000).unwrap();
        assert!(proposal_id > 0);
    });
}

#[test]
fn test_remove_last_guardian_rejected() {
    let (env, cid, admin) = setup();
    let guardian = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), guardian.clone()).unwrap();
        let result = remove_guardian(&env, admin, guardian);
        assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));

        let guardians = get_guardians(&env).unwrap();
        assert_eq!(guardians.len(), 1);
    });
}

#[test]
fn test_start_recovery_rejects_same_admin_rotation() {
    let (env, cid, admin) = setup();
    let guardian = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), guardian.clone()).unwrap();
        let result = start_recovery(&env, guardian, admin.clone(), admin);
        assert_eq!(result, Err(GovernanceError::InvalidProposal));
    });
}

#[test]
fn test_start_recovery_rejects_existing_new_admin() {
    let (env, cid, admin1) = setup();
    let admin2 = Address::generate(&env);
    let guardian = Address::generate(&env);

    with_contract!(env, &cid, {
        let mut admins = Vec::new(&env);
        admins.push_back(admin1.clone());
        admins.push_back(admin2.clone());
        set_multisig_admins(&env, admin1.clone(), admins).unwrap();

        add_guardian(&env, admin1.clone(), guardian.clone()).unwrap();
        let result = start_recovery(&env, guardian, admin1, admin2);
        assert_eq!(result, Err(GovernanceError::InvalidProposal));
    });
}

#[test]
fn test_approve_recovery_clears_invalidated_request_after_admin_rotation() {
    let (env, cid, admin) = setup();
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let replacement_admin = Address::generate(&env);
    let unrelated_admin = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), guardian1.clone()).unwrap();
        add_guardian(&env, admin.clone(), guardian2.clone()).unwrap();
        start_recovery(&env, guardian1, admin.clone(), replacement_admin).unwrap();

        let mut admins = Vec::new(&env);
        admins.push_back(unrelated_admin);
        set_multisig_admins(&env, admin, admins).unwrap();

        let result = approve_recovery(&env, guardian2);
        assert_eq!(result, Err(GovernanceError::InvalidProposal));
        assert!(get_recovery_request(&env).is_none());
        assert!(get_recovery_approvals(&env).is_none());
    });
}

#[test]
fn test_execute_recovery_clears_invalidated_request_after_admin_rotation() {
    let (env, cid, admin) = setup();
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let replacement_admin = Address::generate(&env);
    let unrelated_admin = Address::generate(&env);
    let executor = Address::generate(&env);

    with_contract!(env, &cid, {
        add_guardian(&env, admin.clone(), guardian1.clone()).unwrap();
        add_guardian(&env, admin.clone(), guardian2.clone()).unwrap();
        set_guardian_threshold(&env, admin.clone(), 2).unwrap();
        start_recovery(&env, guardian1, admin.clone(), replacement_admin).unwrap();
        approve_recovery(&env, guardian2).unwrap();

        let mut admins = Vec::new(&env);
        admins.push_back(unrelated_admin);
        set_multisig_admins(&env, admin, admins).unwrap();

        let result = execute_recovery(&env, executor);
        assert_eq!(result, Err(GovernanceError::InvalidProposal));
        assert!(get_recovery_request(&env).is_none());
        assert!(get_recovery_approvals(&env).is_none());
    });
}