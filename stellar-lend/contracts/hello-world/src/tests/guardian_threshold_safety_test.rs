//! Guardian Threshold Safety Regression Tests
//!
//! Tests for the guardian threshold safety improvements implemented in
//! issue #513 to prevent bricking recovery operations.

#![cfg(test)]

use soroban_sdk::{Address, Env};
use stellarlend_hello_world::errors::GovernanceError;
use stellarlend_hello_world::governance::*;
use stellarlend_hello_world::recovery::*;
use stellarlend_hello_world::storage::GovernanceDataKey;
use stellarlend_hello_world::types::GuardianConfig;

#[test]
fn test_guardian_threshold_change_during_recovery_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let guardian3 = Address::generate(&env);
    let old_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    // Initialize governance with 3 guardians, threshold 2
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    set_guardians(&env, admin.clone(), guardians, 2).unwrap();

    // Start recovery
    start_recovery(
        &env,
        guardian1.clone(),
        old_admin.clone(),
        new_admin.clone(),
    )
    .unwrap();

    // Try to change threshold during recovery - should fail
    let result = set_guardian_threshold(&env, admin.clone(), 3);
    assert_eq!(result, Err(GovernanceError::RecoveryInProgress));
}

#[test]
fn test_guardian_removal_during_recovery_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let guardian3 = Address::generate(&env);
    let old_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    // Initialize governance with 3 guardians, threshold 2
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    set_guardians(&env, admin.clone(), guardians, 2).unwrap();

    // Start recovery
    start_recovery(
        &env,
        guardian1.clone(),
        old_admin.clone(),
        new_admin.clone(),
    )
    .unwrap();

    // Try to remove guardian during recovery - should fail
    let result = remove_guardian(&env, admin.clone(), guardian2.clone());
    assert_eq!(result, Err(GovernanceError::RecoveryInProgress));
}

#[test]
fn test_guardian_removal_would_brick_recovery_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let guardian3 = Address::generate(&env);
    let old_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    // Initialize governance with 3 guardians, threshold 2
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    set_guardians(&env, admin.clone(), guardians, 2).unwrap();

    // Start recovery
    start_recovery(
        &env,
        guardian1.clone(),
        old_admin.clone(),
        new_admin.clone(),
    )
    .unwrap();

    // Get approvals so far (guardian1 auto-approved)
    let approvals: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryApprovals)
        .unwrap();
    assert_eq!(approvals.len(), 1);

    // Try to remove guardian2 (who hasn't approved yet) - this would make recovery impossible
    // since we'd only have guardian1 and guardian3 left, and only guardian1 has approved
    let result = remove_guardian(&env, admin.clone(), guardian2.clone());
    assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));
}

#[test]
fn test_guardian_removal_safe_when_enough_approvals_remain() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let guardian3 = Address::generate(&env);
    let old_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    // Initialize governance with 3 guardians, threshold 2
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    set_guardians(&env, admin.clone(), guardians, 2).unwrap();

    // Start recovery
    start_recovery(
        &env,
        guardian1.clone(),
        old_admin.clone(),
        new_admin.clone(),
    )
    .unwrap();

    // Get second approval from guardian2
    approve_recovery(&env, guardian2.clone()).unwrap();

    // Now try to remove guardian3 (who hasn't approved) - this should be safe
    // since guardian1 and guardian2 have already approved, meeting threshold 2
    let result = remove_guardian(&env, admin.clone(), guardian3.clone());
    assert_eq!(result, Ok(()));
}

#[test]
fn test_threshold_change_when_no_recovery_succeeds() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let guardian3 = Address::generate(&env);

    // Initialize governance with 3 guardians, threshold 2
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    set_guardians(&env, admin.clone(), guardians, 2).unwrap();

    // Change threshold when no recovery is active - should succeed
    let result = set_guardian_threshold(&env, admin.clone(), 3);
    assert_eq!(result, Ok(()));
}

#[test]
fn test_recovery_threshold_edge_cases() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);

    // Initialize governance with 2 guardians, threshold 1
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());

    set_guardians(&env, admin.clone(), guardians, 1).unwrap();

    // Start recovery with threshold 1
    start_recovery(
        &env,
        guardian1.clone(),
        Address::generate(&env),
        Address::generate(&env),
    )
    .unwrap();

    // Should be able to execute immediately since threshold is 1 and initiator auto-approved
    let result = execute_recovery(&env, Address::generate(&env));
    assert_eq!(result, Ok(()));
}

#[test]
fn test_guardian_threshold_zero_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);

    // Initialize governance with 2 guardians
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());

    set_guardians(&env, admin.clone(), guardians, 1).unwrap();

    // Try to set threshold to 0 - should fail
    let result = set_guardian_threshold(&env, admin.clone(), 0);
    assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));
}

#[test]
fn test_guardian_threshold_exceeds_count_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);

    // Initialize governance with 2 guardians
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());

    set_guardians(&env, admin.clone(), guardians, 1).unwrap();

    // Try to set threshold to 3 (exceeds guardian count) - should fail
    let result = set_guardian_threshold(&env, admin.clone(), 3);
    assert_eq!(result, Err(GovernanceError::InvalidGuardianConfig));
}

#[test]
fn test_auto_threshold_adjustment_on_removal() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let guardian1 = Address::generate(&env);
    let guardian2 = Address::generate(&env);
    let guardian3 = Address::generate(&env);

    // Initialize governance with 3 guardians, threshold 3
    initialize(
        &env,
        admin.clone(),
        Address::generate(&env), // vote token
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut guardians = Vec::new(&env);
    guardians.push_back(guardian1.clone());
    guardians.push_back(guardian2.clone());
    guardians.push_back(guardian3.clone());

    set_guardians(&env, admin.clone(), guardians, 3).unwrap();

    // Remove one guardian - threshold should auto-adjust to 2
    let result = remove_guardian(&env, admin.clone(), guardian3.clone());
    assert_eq!(result, Ok(()));

    // Check that threshold was adjusted
    let config: GuardianConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
        .unwrap();
    assert_eq!(config.threshold, 2);
    assert_eq!(config.guardians.len(), 2);
}
