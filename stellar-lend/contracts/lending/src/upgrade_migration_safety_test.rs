// Upgrade and Storage Migration Safety Test Suite
//
// This test suite validates contract upgrade scenarios with focus on:
// - Storage layout compatibility across versions
// - User state preservation (balances, positions, configs)
// - Failed upgrade handling and rollback scenarios
// - Multi-step upgrade paths
// - Concurrent state modifications during upgrade proposals

extern crate alloc;
use alloc::{format, vec::Vec};

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String as SorobanString};

use crate::{LendingContract, LendingContractClient, UpgradeStage};

// ═══════════════════════════════════════════════════════
// Test Helpers
// ═══════════════════════════════════════════════════════

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

fn setup_contract(env: &Env) -> (LendingContractClient<'_>, Address) {
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    (client, admin)
}

fn setup_with_upgrade_init(
    env: &Env,
    required_approvals: u32,
) -> (LendingContractClient<'_>, Address) {
    let (client, admin) = setup_contract(env);
    client.upgrade_init(&admin, &hash(env, 1), &required_approvals);
    (client, admin)
}

// Seed user state with data store entries
fn seed_user_state(env: &Env, client: &LendingContractClient, admin: &Address, users: &[Address]) {
    // Initialize data store for user metadata
    client.data_store_init(admin);

    for (idx, _user) in users.iter().enumerate() {
        let key = SorobanString::from_str(env, &format!("user_{idx}"));
        let value = soroban_sdk::Bytes::from_slice(env, &[idx as u8; 32]);
        client.data_save(admin, &key, &value);
    }
}

// ═══════════════════════════════════════════════════════
// 1. Basic Upgrade with State Preservation
// ═══════════════════════════════════════════════════════

#[test]
fn test_upgrade_preserves_admin_and_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    assert_eq!(client.current_version(), 0);
    assert_eq!(client.current_wasm_hash(), hash(&env, 1));

    // Propose and execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    // Verify version and hash updated
    assert_eq!(client.current_version(), 1);
    assert_eq!(client.current_wasm_hash(), hash(&env, 2));

    // Verify admin still has control
    let new_approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &new_approver);
}

#[test]
fn test_upgrade_preserves_data_store_entries() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Initialize data store and save entries
    client.data_store_init(&admin);
    let key1 = SorobanString::from_str(&env, "balance_user1");
    let val1 = soroban_sdk::Bytes::from_slice(&env, &[1, 2, 3, 4]);
    client.data_save(&admin, &key1, &val1);

    let key2 = SorobanString::from_str(&env, "position_user2");
    let val2 = soroban_sdk::Bytes::from_slice(&env, &[5, 6, 7, 8]);
    client.data_save(&admin, &key2, &val2);

    assert_eq!(client.data_entry_count(), 2);

    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    // Verify all data preserved
    assert_eq!(client.data_load(&key1), val1);
    assert_eq!(client.data_load(&key2), val2);
    assert_eq!(client.data_entry_count(), 2);
}

#[test]
fn test_upgrade_preserves_multiple_user_states() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Create multiple users with state
    let users: Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
    seed_user_state(&env, &client, &admin, &users);

    let pre_upgrade_count = client.data_entry_count();
    assert_eq!(pre_upgrade_count, 5);

    // Execute upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &2);
    client.upgrade_execute(&admin, &proposal_id);

    // Verify all user states preserved
    assert_eq!(client.data_entry_count(), pre_upgrade_count);

    for (idx, _user) in users.iter().enumerate() {
        let key = SorobanString::from_str(&env, &format!("user_{idx}"));
        let expected = soroban_sdk::Bytes::from_slice(&env, &[idx as u8; 32]);
        assert_eq!(client.data_load(&key), expected);
    }
}

// ═══════════════════════════════════════════════════════
// 2. Multi-Step Upgrade Path
// ═══════════════════════════════════════════════════════

#[test]
fn test_sequential_upgrades_preserve_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    client.data_store_init(&admin);
    let key = SorobanString::from_str(&env, "persistent_data");
    let val = soroban_sdk::Bytes::from_slice(&env, &[0xAA; 16]);
    client.data_save(&admin, &key, &val);

    // Upgrade v0 -> v1
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);
    assert_eq!(client.current_version(), 1);
    assert_eq!(client.data_load(&key), val);

    // Upgrade v1 -> v2
    let p2 = client.upgrade_propose(&admin, &hash(&env, 3), &2);
    client.upgrade_execute(&admin, &p2);
    assert_eq!(client.current_version(), 2);
    assert_eq!(client.data_load(&key), val);

    // Upgrade v2 -> v5 (skip versions)
    let p3 = client.upgrade_propose(&admin, &hash(&env, 4), &5);
    client.upgrade_execute(&admin, &p3);
    assert_eq!(client.current_version(), 5);
    assert_eq!(client.data_load(&key), val);
}

#[test]
fn test_upgrade_with_state_modifications_between_versions() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    client.data_store_init(&admin);

    // Version 0: Initial state
    let k1 = SorobanString::from_str(&env, "k1");
    let v1 = soroban_sdk::Bytes::from_slice(&env, &[1]);
    client.data_save(&admin, &k1, &v1);

    // Upgrade to v1
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);

    // Modify state in v1
    let k2 = SorobanString::from_str(&env, "k2");
    let v2 = soroban_sdk::Bytes::from_slice(&env, &[2]);
    client.data_save(&admin, &k2, &v2);

    // Upgrade to v2
    let p2 = client.upgrade_propose(&admin, &hash(&env, 3), &2);
    client.upgrade_execute(&admin, &p2);

    // Verify both states preserved
    assert_eq!(client.data_load(&k1), v1);
    assert_eq!(client.data_load(&k2), v2);
    assert_eq!(client.data_entry_count(), 2);
}

// ═══════════════════════════════════════════════════════
// 3. Rollback Scenarios
// ═══════════════════════════════════════════════════════

#[test]
fn test_rollback_restores_previous_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Execute upgrade v0 -> v1
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    assert_eq!(client.current_version(), 1);
    assert_eq!(client.current_wasm_hash(), hash(&env, 2));

    // Rollback to v0
    client.upgrade_rollback(&admin, &proposal_id);
    assert_eq!(client.current_version(), 0);
    assert_eq!(client.current_wasm_hash(), hash(&env, 1));

    // Verify proposal marked as rolled back
    let status = client.upgrade_status(&proposal_id);
    assert_eq!(status.stage, UpgradeStage::RolledBack);
}

#[test]
fn test_rollback_preserves_user_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Setup state before upgrade
    client.data_store_init(&admin);
    let key = SorobanString::from_str(&env, "critical_data");
    let val = soroban_sdk::Bytes::from_slice(&env, &[0xFF; 32]);
    client.data_save(&admin, &key, &val);

    // Upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    // Modify state after upgrade
    let key2 = SorobanString::from_str(&env, "new_data");
    let val2 = soroban_sdk::Bytes::from_slice(&env, &[0xAA; 16]);
    client.data_save(&admin, &key2, &val2);

    // Rollback
    client.upgrade_rollback(&admin, &proposal_id);

    // Verify all state still accessible (storage is persistent)
    assert_eq!(client.data_load(&key), val);
    assert_eq!(client.data_load(&key2), val2);
}

#[test]
#[should_panic]
fn test_rollback_cannot_be_repeated() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);
    client.upgrade_rollback(&admin, &proposal_id);

    // Second rollback should fail
    client.upgrade_rollback(&admin, &proposal_id);
}

#[test]
fn test_rollback_then_new_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Upgrade and rollback
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);
    client.upgrade_rollback(&admin, &p1);

    assert_eq!(client.current_version(), 0);

    // New upgrade should work
    let p2 = client.upgrade_propose(&admin, &hash(&env, 3), &1);
    client.upgrade_execute(&admin, &p2);
    assert_eq!(client.current_version(), 1);
}

// ═══════════════════════════════════════════════════════
// 4. Failed Upgrade Scenarios
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_execute_without_approval_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 2);

    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    // Propose but don't get enough approvals
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);

    // Try to execute without threshold - should fail
    client.upgrade_execute(&admin, &proposal_id);
}

#[test]
#[should_panic]
fn test_execute_already_executed_proposal_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    // Second execution should fail
    client.upgrade_execute(&admin, &proposal_id);
}

#[test]
#[should_panic]
fn test_propose_same_version_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Upgrade to v1
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);

    // Try to propose v1 again - should fail
    client.upgrade_propose(&admin, &hash(&env, 3), &1);
}

#[test]
#[should_panic]
fn test_propose_lower_version_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Upgrade to v5
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &5);
    client.upgrade_execute(&admin, &p1);

    // Try to propose v3 - should fail
    client.upgrade_propose(&admin, &hash(&env, 3), &3);
}

// ═══════════════════════════════════════════════════════
// 5. Concurrent Operations During Upgrade
// ═══════════════════════════════════════════════════════

#[test]
fn test_state_modifications_during_proposal_phase() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 2);

    client.data_store_init(&admin);

    // Create proposal
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::Proposed
    );

    // Modify state while proposal is pending
    let key = SorobanString::from_str(&env, "during_proposal");
    let val = soroban_sdk::Bytes::from_slice(&env, &[0xBB; 8]);
    client.data_save(&admin, &key, &val);

    // Complete upgrade
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);
    client.upgrade_approve(&approver, &proposal_id);
    client.upgrade_execute(&admin, &proposal_id);

    // Verify state preserved
    assert_eq!(client.data_load(&key), val);
}

#[test]
fn test_multiple_pending_proposals() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 3);

    let approver1 = Address::generate(&env);
    let approver2 = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver1);
    client.upgrade_add_approver(&admin, &approver2);

    // Create multiple proposals
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    let p2 = client.upgrade_propose(&admin, &hash(&env, 3), &2);

    // Approve and execute first proposal
    client.upgrade_approve(&approver1, &p1);
    client.upgrade_approve(&approver2, &p1);
    client.upgrade_execute(&admin, &p1);

    assert_eq!(client.current_version(), 1);

    // Second proposal should now be invalid (version too low)
    let result = client.try_upgrade_execute(&admin, &p2);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════
// 6. Storage Schema Migration
// ═══════════════════════════════════════════════════════

#[test]
fn test_schema_version_bump_during_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    client.data_store_init(&admin);
    assert_eq!(client.data_schema_version(), 0);

    // Upgrade contract
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);

    // Bump schema version to match new contract
    let memo = SorobanString::from_str(&env, "v1_schema_migration");
    client.data_migrate_bump_version(&admin, &1, &memo);

    assert_eq!(client.data_schema_version(), 1);
    assert_eq!(client.current_version(), 1);
}

#[test]
fn test_backup_restore_across_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    client.data_store_init(&admin);

    // Create and backup state
    let k1 = SorobanString::from_str(&env, "k1");
    let v1 = soroban_sdk::Bytes::from_slice(&env, &[1, 2, 3]);
    client.data_save(&admin, &k1, &v1);

    let backup_name = SorobanString::from_str(&env, "pre_upgrade_backup");
    client.data_backup(&admin, &backup_name);

    // Upgrade
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);

    // Modify state after upgrade
    let k2 = SorobanString::from_str(&env, "k2");
    let v2 = soroban_sdk::Bytes::from_slice(&env, &[4, 5, 6]);
    client.data_save(&admin, &k2, &v2);

    // Restore pre-upgrade backup
    client.data_restore(&admin, &backup_name);

    // Should have only pre-upgrade data
    assert_eq!(client.data_load(&k1), v1);
    assert_eq!(client.data_entry_count(), 1);
}

#[test]
fn test_migration_with_large_dataset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    client.data_store_init(&admin);

    // Create large dataset
    for i in 0..50 {
        let key = SorobanString::from_str(&env, &format!("key_{i}"));
        let val = soroban_sdk::Bytes::from_slice(&env, &[i as u8; 64]);
        client.data_save(&admin, &key, &val);
    }

    assert_eq!(client.data_entry_count(), 50);

    // Upgrade
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &p1);

    // Verify all data intact
    assert_eq!(client.data_entry_count(), 50);

    for i in 0..50 {
        let key = SorobanString::from_str(&env, &format!("key_{i}"));
        let expected = soroban_sdk::Bytes::from_slice(&env, &[i as u8; 64]);
        assert_eq!(client.data_load(&key), expected);
    }
}

// ═══════════════════════════════════════════════════════
// 7. Authorization and Security
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_non_admin_cannot_rollback() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    let stranger = Address::generate(&env);
    client.upgrade_rollback(&stranger, &proposal_id);
}

#[test]
#[should_panic]
fn test_non_approver_cannot_execute() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);

    let stranger = Address::generate(&env);
    client.upgrade_execute(&stranger, &proposal_id);
}

#[test]
fn test_approver_permissions_preserved_across_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 2);

    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    // Upgrade
    let p1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_approve(&approver, &p1);
    client.upgrade_execute(&admin, &p1);

    // Approver should still be able to approve new proposals
    let p2 = client.upgrade_propose(&admin, &hash(&env, 3), &2);
    let count = client.upgrade_approve(&approver, &p2);
    assert_eq!(count, 2);
}

// ═══════════════════════════════════════════════════════
// 8. Edge Cases
// ═══════════════════════════════════════════════════════

#[test]
fn test_upgrade_with_empty_data_store() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    client.data_store_init(&admin);
    assert_eq!(client.data_entry_count(), 0);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    assert_eq!(client.data_entry_count(), 0);
    assert_eq!(client.current_version(), 1);
}

#[test]
fn test_upgrade_with_max_approvers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 10);

    // Add 9 more approvers (admin is already one)
    let approvers: Vec<Address> = (0..9).map(|_| Address::generate(&env)).collect();
    for approver in &approvers {
        client.upgrade_add_approver(&admin, approver);
    }

    // Create proposal
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);

    // Get all approvals
    for approver in &approvers {
        client.upgrade_approve(approver, &proposal_id);
    }

    // Execute
    client.upgrade_execute(&admin, &proposal_id);
    assert_eq!(client.current_version(), 1);
}

#[test]
fn test_rapid_version_increments() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    // Rapidly upgrade through versions
    for version in 1..=10 {
        let hash_byte = (version + 1) as u8;
        let proposal_id = client.upgrade_propose(&admin, &hash(&env, hash_byte), &version);
        client.upgrade_execute(&admin, &proposal_id);
        assert_eq!(client.current_version(), version);
    }
}

#[test]
fn test_upgrade_preserves_writer_permissions() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_with_upgrade_init(&env, 1);

    client.data_store_init(&admin);
    let writer = Address::generate(&env);
    client.data_grant_writer(&admin, &writer);

    // Writer can save before upgrade
    let k1 = SorobanString::from_str(&env, "k1");
    let v1 = soroban_sdk::Bytes::from_slice(&env, &[1]);
    client.data_save(&writer, &k1, &v1);

    // Upgrade
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    // Writer can still save after upgrade
    let k2 = SorobanString::from_str(&env, "k2");
    let v2 = soroban_sdk::Bytes::from_slice(&env, &[2]);
    client.data_save(&writer, &k2, &v2);

    assert_eq!(client.data_entry_count(), 2);
}
