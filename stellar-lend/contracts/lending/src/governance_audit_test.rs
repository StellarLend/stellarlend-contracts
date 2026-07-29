//! Governance audit log tests for StellarLend lending contract.
//!
//! Tests verify that all governance and admin actions are properly recorded
//! in the audit log with correct sequence numbers, actors, timestamps, and ledger info.

#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Bytes, Env, String,
};

/// Create a fresh, initialized contract client.
fn init_client() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

// ─────────────────────────────────────────────────────────────────────────
// Audit Log Count and Entry Tests
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_governance_audit_count_returns_zero_initially() {
    let (_, client, _) = init_client();
    let count = client.get_governance_audit_count();
    assert_eq!(count, 0);
}

#[test]
fn test_record_audit_entry_increments_count() {
    let (env, client, admin) = init_client();

    // Perform an admin action that should be logged
    client.set_min_borrow(&100);

    let count = client.get_governance_audit_count();
    assert_eq!(count, 1);

    // Perform another admin action
    client.set_min_borrow(&200);
    let count = client.get_governance_audit_count();
    assert_eq!(count, 2);
}

#[test]
fn test_get_governance_audit_entries_returns_empty_when_no_entries() {
    let (_, client, _) = init_client();
    let entries = client.get_governance_audit_entries(&0);
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_get_governance_audit_entries_returns_most_recent_first() {
    let (env, client, admin) = init_client();

    // Record multiple actions
    client.set_min_borrow(&100);
    client.set_min_borrow(&200);
    client.set_min_borrow(&300);

    let entries = client.get_governance_audit_entries(&0);
    assert_eq!(entries.len(), 3);

    // Most recent first (reverse chronological)
    assert_eq!(entries.get(0).unwrap().sequence, 2);
    assert_eq!(entries.get(1).unwrap().sequence, 1);
    assert_eq!(entries.get(2).unwrap().sequence, 0);
}

#[test]
fn test_get_governance_audit_entries_with_limit_returns_correct_count() {
    let (env, client, admin) = init_client();

    // Record 5 actions
    for i in 0..5 {
        client.set_min_borrow(&(i * 100 + 100));
    }

    // Request only 3
    let entries = client.get_governance_audit_entries(&3);
    assert_eq!(entries.len(), 3);

    // Should return most recent: seq 4, 3, 2
    assert_eq!(entries.get(0).unwrap().sequence, 4);
    assert_eq!(entries.get(1).unwrap().sequence, 3);
    assert_eq!(entries.get(2).unwrap().sequence, 2);
}

#[test]
fn test_get_governance_audit_entries_limit_0_returns_all_available() {
    let (env, client, admin) = init_client();

    for i in 0..5 {
        client.set_min_borrow(&(i * 100 + 100));
    }

    let entries = client.get_governance_audit_entries(&0);
    assert_eq!(entries.len(), 5);
}

#[test]
fn test_audit_entry_contains_correct_actor_and_details() {
    let (env, client, admin) = init_client();

    // Perform an admin action
    client.set_min_borrow(&500);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);

    let entry = entries.get(0).unwrap();
    assert_eq!(entry.actor, admin);
    assert_eq!(entry.sequence, 0);
    // Check that action is set to something
    let action_str = entry.action.to_string();
    assert!(!action_str.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────
// Governance Action Recording Tests
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn test_governance_action_is_recorded_after_set_min_borrow() {
    let (env, client, admin) = init_client();

    let count_before = client.get_governance_audit_count();
    client.set_min_borrow(&1000);
    let count_after = client.get_governance_audit_count();

    assert_eq!(count_before, 0);
    assert_eq!(count_after, 1);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_governance_action_is_recorded_after_set_debt_ceiling() {
    let (env, client, admin) = init_client();

    client.set_debt_ceiling(&1_000_000_000_000_000);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_governance_action_is_recorded_after_set_insurance_share() {
    let (env, client, admin) = init_client();

    client.set_insurance_share(&500);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_governance_action_is_recorded_after_accept_admin() {
    let (env, client, admin) = init_client();
    let new_admin = Address::generate(&env);

    // Propose new admin
    client.propose_admin(&new_admin);

    // Accept (no entry yet since proposal doesn't record)
    client.accept_admin();

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    // The new admin who accepted should be in the audit log
    assert_eq!(entries.get(0).unwrap().actor, new_admin);
}

#[test]
fn test_governance_action_is_recorded_after_set_guardian() {
    let (env, client, admin) = init_client();
    let guardian = Address::generate(&env);

    client.set_guardian(&guardian);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_governance_action_is_recorded_after_set_emergency_state() {
    let (env, client, admin) = init_client();

    client.set_emergency_state(&EmergencyState::Shutdown);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_governance_action_is_recorded_after_set_pause() {
    let (env, client, admin) = init_client();

    client.set_pause(&PauseType::All, &true, &1000);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_governance_action_is_recorded_after_set_flash_fee() {
    let (env, client, admin) = init_client();

    client.set_flash_fee(&500);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_governance_action_is_recorded_after_set_asset_isolation() {
    let (env, client, admin) = init_client();
    let asset = Address::generate(&env);

    client.set_asset_isolation(&asset, &true, &1_000_000_000_000);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries.get(0).unwrap().actor, admin);
}

#[test]
fn test_multiple_actions_recorded_in_sequence() {
    let (env, client, admin) = init_client();

    client.set_min_borrow(&100);
    client.set_debt_ceiling(&1_000_000_000_000);
    client.set_insurance_share(&1000);

    let entries = client.get_governance_audit_entries(&0);
    assert_eq!(entries.len(), 3);

    // Verify all entries are recorded with correct sequence and actor
    assert_eq!(entries.get(0).unwrap().sequence, 2);
    assert_eq!(entries.get(1).unwrap().sequence, 1);
    assert_eq!(entries.get(2).unwrap().sequence, 0);

    for entry in entries.iter() {
        assert_eq!(entry.actor, admin);
    }
}

#[test]
fn test_audit_entry_contains_ledger_and_timestamp() {
    let (env, client, admin) = init_client();

    let ledger_before = env.ledger().sequence();

    client.set_min_borrow(&100);

    let entries = client.get_governance_audit_entries(&1);
    assert_eq!(entries.len(), 1);

    let entry = entries.get(0).unwrap();
    // Ledger should be set and >= ledger_before
    assert!(entry.ledger >= ledger_before);
    // Timestamp should be set (non-zero in test)
    assert!(entry.timestamp > 0);
}

#[test]
fn test_multiple_different_actions_logged_correctly() {
    let (env, client, admin) = init_client();

    client.set_min_borrow(&100);
    client.set_liquidation_grace_period(&3600);
    client.set_flash_fee(&250);

    let entries = client.get_governance_audit_entries(&0);
    assert_eq!(entries.len(), 3);

    // All should have the same admin
    for entry in entries.iter() {
        assert_eq!(entry.actor, admin);
    }

    // All should have different (or same) sequences but in order
    assert_eq!(entries.get(0).unwrap().sequence, 2);
    assert_eq!(entries.get(1).unwrap().sequence, 1);
    assert_eq!(entries.get(2).unwrap().sequence, 0);
}

#[test]
fn test_audit_log_persists_across_calls() {
    let (env, client, admin) = init_client();

    // First call
    client.set_min_borrow(&100);
    let count1 = client.get_governance_audit_count();
    assert_eq!(count1, 1);

    // Second call
    client.set_min_borrow(&200);
    let count2 = client.get_governance_audit_count();
    assert_eq!(count2, 2);

    // Verify both entries are still there
    let entries = client.get_governance_audit_entries(&0);
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_audit_entries_return_correct_count_with_limit() {
    let (env, client, admin) = init_client();

    // Record 10 actions
    for i in 0..10 {
        client.set_min_borrow(&(i * 100 + 100));
    }

    let entries_5 = client.get_governance_audit_entries(&5);
    assert_eq!(entries_5.len(), 5);

    let entries_10 = client.get_governance_audit_entries(&10);
    assert_eq!(entries_10.len(), 10);

    let entries_20 = client.get_governance_audit_entries(&20);
    assert_eq!(entries_20.len(), 10); // Can't return more than available

    let entries_all = client.get_governance_audit_entries(&0);
    assert_eq!(entries_all.len(), 10);
}

#[test]
fn test_sequence_numbers_are_monotonic() {
    let (env, client, admin) = init_client();

    for i in 0..5 {
        client.set_min_borrow(&(i * 100 + 100));
    }

    let entries = client.get_governance_audit_entries(&0);

    // Entries are most-recent-first, so sequence should be descending
    let mut prev_seq = u64::MAX;
    for entry in entries.iter() {
        assert!(entry.sequence < prev_seq);
        prev_seq = entry.sequence;
    }
}
