#![cfg(test)]

//! Guardian threshold safety tests.
//!
//! Verifies the safety guardrails added to `set_guardian_threshold` and
//! `remove_guardian` in `governance.rs`:
//!
//! | Test | Scenario | Expected |
//! |---|---|---|
//! | `test_guardian_threshold_change_during_recovery_fails` | Threshold change while recovery active | `RecoveryInProgress` |
//! | `test_guardian_removal_during_recovery_fails` | Guardian removal while recovery active | `RecoveryInProgress` |
//! | `test_guardian_removal_would_brick_recovery_fails` | Remove guardian → count < threshold | `InvalidGuardianConfig` |
//! | `test_guardian_removal_safe_when_enough_remain` | Remove guardian → count >= threshold | Success |
//! | `test_threshold_change_when_no_recovery_succeeds` | Normal threshold change | Success |
//! | `test_recovery_threshold_edge_case_one` | Threshold = 1 with one guardian | Success |
//! | `test_guardian_threshold_zero_fails` | Set threshold to 0 | `InvalidGuardianConfig` |
//! | `test_guardian_threshold_exceeds_count_fails` | Set threshold > guardian count | `InvalidGuardianConfig` |
//! | `test_guardian_removal_clears_after_recovery_completes` | Remove after recovery completes | Success |

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

use crate::governance::{self, GovernanceDataKey, GovernanceError};
use crate::types::RecoveryRequest;

// ---------------------------------------------------------------------------
// Minimal test host contract
// ---------------------------------------------------------------------------

#[contract]
struct ThresholdTestHost;

#[contractimpl]
impl ThresholdTestHost {
    /// Initialise governance and seed N guardians.
    pub fn setup(env: Env, admin: Address, guardians: Vec<Address>) {
        governance::initialize(
            &env,
            admin.clone(),
            Address::generate(&env), // dummy vote token
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        for g in guardians.iter() {
            governance::add_guardian(&env, admin.clone(), g).unwrap();
        }
    }

    /// Inject a fake active recovery so we can test the RecoveryInProgress guard
    /// without having to satisfy the full guardian signature flow.
    pub fn inject_active_recovery(env: Env, old_admin: Address, new_admin: Address) {
        let request = RecoveryRequest {
            old_admin,
            new_admin,
            initiated_at: env.ledger().timestamp(),
            approval_count: 1,
        };
        env.storage()
            .instance()
            .set(&GovernanceDataKey::RecoveryRequest, &request);
        let mut approvals: Vec<Address> = Vec::new(&env);
        approvals.push_back(Address::generate(&env));
        env.storage()
            .instance()
            .set(&GovernanceDataKey::RecoveryApprovals, &approvals);
    }

    /// Remove the active recovery state (simulates `execute_recovery`).
    pub fn clear_recovery(env: Env) {
        env.storage()
            .instance()
            .remove(&GovernanceDataKey::RecoveryRequest);
        env.storage()
            .instance()
            .remove(&GovernanceDataKey::RecoveryApprovals);
    }

    pub fn set_threshold(env: Env, admin: Address, threshold: u32) -> Result<(), GovernanceError> {
        governance::set_guardian_threshold(&env, admin, threshold)
    }

    pub fn remove_g(
        env: Env,
        admin: Address,
        guardian: Address,
    ) -> Result<(), GovernanceError> {
        governance::remove_guardian(&env, admin, guardian)
    }

    pub fn get_threshold(env: Env) -> u32 {
        governance::get_guardian_config(&env)
            .map(|gc| gc.threshold)
            .unwrap_or(0)
    }

    pub fn guardian_count(env: Env) -> u32 {
        governance::get_guardian_config(&env)
            .map(|gc| gc.guardians.len() as u32)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Test 1 — threshold change blocked during recovery
// ---------------------------------------------------------------------------

#[test]
fn test_guardian_threshold_change_during_recovery_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());
    guardians.push_back(g2.clone());

    client.setup(&admin, &guardians);
    // Threshold starts at 1 (default after add_guardian calls).
    client.inject_active_recovery(&admin, &Address::generate(&env));

    let result = client.try_set_threshold(&admin, &2);
    assert_eq!(result, Err(Ok(GovernanceError::RecoveryInProgress)));
}

// ---------------------------------------------------------------------------
// Test 2 — guardian removal blocked during recovery
// ---------------------------------------------------------------------------

#[test]
fn test_guardian_removal_during_recovery_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());
    guardians.push_back(g2.clone());

    client.setup(&admin, &guardians);
    client.inject_active_recovery(&admin, &Address::generate(&env));

    let result = client.try_remove_g(&admin, &g1);
    assert_eq!(result, Err(Ok(GovernanceError::RecoveryInProgress)));
}

// ---------------------------------------------------------------------------
// Test 3 — removal blocked when it would brick recovery (count < threshold)
// ---------------------------------------------------------------------------

#[test]
fn test_guardian_removal_would_brick_recovery_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());
    guardians.push_back(g2.clone());

    client.setup(&admin, &guardians);
    // Raise threshold to 2 so removing either guardian would brick recovery.
    client.set_threshold(&admin, &2);

    let result = client.try_remove_g(&admin, &g1);
    assert_eq!(result, Err(Ok(GovernanceError::InvalidGuardianConfig)));
}

// ---------------------------------------------------------------------------
// Test 4 — removal safe when enough guardians remain
// ---------------------------------------------------------------------------

#[test]
fn test_guardian_removal_safe_when_enough_remain() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let g3 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());
    guardians.push_back(g2.clone());
    guardians.push_back(g3.clone());

    client.setup(&admin, &guardians);
    // threshold is 1 by default — removing one guardian leaves 2 >= 1, safe.
    client.remove_g(&admin, &g3);
    assert_eq!(client.guardian_count(), 2);
}

// ---------------------------------------------------------------------------
// Test 5 — threshold change succeeds when no recovery is active
// ---------------------------------------------------------------------------

#[test]
fn test_threshold_change_when_no_recovery_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());
    guardians.push_back(g2.clone());

    client.setup(&admin, &guardians);
    client.set_threshold(&admin, &2);
    assert_eq!(client.get_threshold(), 2);
}

// ---------------------------------------------------------------------------
// Test 6 — threshold = 1 with a single guardian is valid
// ---------------------------------------------------------------------------

#[test]
fn test_recovery_threshold_edge_case_one() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());

    client.setup(&admin, &guardians);
    client.set_threshold(&admin, &1);
    assert_eq!(client.get_threshold(), 1);
}

// ---------------------------------------------------------------------------
// Test 7 — threshold = 0 is always invalid
// ---------------------------------------------------------------------------

#[test]
fn test_guardian_threshold_zero_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());

    client.setup(&admin, &guardians);
    let result = client.try_set_threshold(&admin, &0);
    assert_eq!(result, Err(Ok(GovernanceError::InvalidGuardianConfig)));
}

// ---------------------------------------------------------------------------
// Test 8 — threshold > guardian count is invalid
// ---------------------------------------------------------------------------

#[test]
fn test_guardian_threshold_exceeds_count_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());
    guardians.push_back(g2.clone());

    client.setup(&admin, &guardians);
    // 2 guardians, threshold = 3 → invalid.
    let result = client.try_set_threshold(&admin, &3);
    assert_eq!(result, Err(Ok(GovernanceError::InvalidGuardianConfig)));
}

// ---------------------------------------------------------------------------
// Test 9 — removal succeeds after recovery completes
// ---------------------------------------------------------------------------

#[test]
fn test_guardian_removal_clears_after_recovery_completes() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ThresholdTestHost, ());
    let client = ThresholdTestHostClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let g1 = Address::generate(&env);
    let g2 = Address::generate(&env);
    let mut guardians = Vec::new(&env);
    guardians.push_back(g1.clone());
    guardians.push_back(g2.clone());

    client.setup(&admin, &guardians);
    // Start then complete recovery.
    client.inject_active_recovery(&admin, &Address::generate(&env));
    client.clear_recovery();

    // Now removal should succeed (threshold = 1, removing one leaves 1 >= 1).
    client.remove_g(&admin, &g2);
    assert_eq!(client.guardian_count(), 1);
}
