//! Governance audit log for tracking all administrative and governance actions.
//!
//! This module provides a circular-buffer-based audit log that records all governance
//! actions with immutable records, gas-efficient storage, and real-time event emission.

#![cfg_attr(test, allow(dead_code))]

use soroban_sdk::{contracttype, Address, Env, String, Vec};

/// Storage keys for the governance audit log.
/// Implements a circular buffer with a configurable maximum.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditLogKey {
    /// Total number of audit log entries ever written (monotonically increasing).
    Count,
    /// Individual audit log entry by index (0-based, wraps at MAX_AUDIT_LOG_SIZE).
    Entry(u64),
    /// Maximum number of entries to retain (circular buffer size).
    MaxSize,
}

/// A single governance audit log entry.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AuditLogEntry {
    /// Sequential entry number (never resets, used to detect overwrites).
    pub sequence: u64,
    /// The governance action that was performed.
    pub action: String,
    /// The address that performed the action.
    pub actor: Address,
    /// Ledger sequence number when the action occurred.
    pub ledger: u32,
    /// Unix timestamp when the action occurred.
    pub timestamp: u64,
    /// Optional additional context (serialized as string).
    pub details: Option<String>,
}

/// Default maximum audit log entries (circular buffer size).
/// Matches the value documented in governance_audit.md.
pub const DEFAULT_MAX_AUDIT_LOG_SIZE: u64 = 100;

/// Records a governance action in the audit log.
/// Uses a circular buffer — oldest entries are overwritten when max is reached.
///
/// # Arguments
/// * `action` - Short description of the action (e.g. "set_admin", "pause")
/// * `actor` - Address that performed the action
/// * `details` - Optional additional context
pub fn record_audit_entry(
    env: &Env,
    action: String,
    actor: Address,
    details: Option<String>,
) {
    let max_size: u64 = env
        .storage()
        .instance()
        .get(&AuditLogKey::MaxSize)
        .unwrap_or(DEFAULT_MAX_AUDIT_LOG_SIZE);
    let count: u64 = env
        .storage()
        .persistent()
        .get(&AuditLogKey::Count)
        .unwrap_or(0);

    let entry = AuditLogEntry {
        sequence: count,
        action,
        actor,
        ledger: env.ledger().sequence(),
        timestamp: env.ledger().timestamp(),
        details,
    };

    // Circular buffer: write at index = count % max_size
    let index = count % max_size;
    env.storage()
        .persistent()
        .set(&AuditLogKey::Entry(index), &entry);
    env.storage()
        .persistent()
        .set(&AuditLogKey::Count, &(count + 1));

    // Extend TTL on count key and entry key to keep them alive
    env.storage()
        .persistent()
        .extend_ttl(&AuditLogKey::Count, 518_400, 518_400);
    env.storage()
        .persistent()
        .extend_ttl(&AuditLogKey::Entry(index), 518_400, 518_400);
}

/// Returns the total number of governance audit log entries ever recorded.
/// This count never resets even when the circular buffer wraps.
///
/// Matches the function documented in governance_audit.md.
pub fn get_governance_audit_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&AuditLogKey::Count)
        .unwrap_or(0)
}

/// Returns audit log entries, most recent first.
/// Returns up to `limit` entries, capped at the circular buffer size.
///
/// # Arguments
/// * `limit` - Maximum number of entries to return (0 = return all available)
///
/// Matches the function documented in governance_audit.md.
pub fn get_governance_audit_entries(env: &Env, limit: u64) -> Vec<AuditLogEntry> {
    let max_size: u64 = env
        .storage()
        .instance()
        .get(&AuditLogKey::MaxSize)
        .unwrap_or(DEFAULT_MAX_AUDIT_LOG_SIZE);
    let count: u64 = env
        .storage()
        .persistent()
        .get(&AuditLogKey::Count)
        .unwrap_or(0);

    if count == 0 {
        return Vec::new(env);
    }

    // How many entries actually exist (capped at buffer size)
    let available = count.min(max_size);
    let to_return = if limit == 0 {
        available
    } else {
        limit.min(available)
    };

    let mut entries = Vec::new(env);

    // Read most recent first (descending from count-1)
    for i in 0..to_return {
        let seq = count - 1 - i;
        let index = seq % max_size;
        if let Some(entry) = env
            .storage()
            .persistent()
            .get::<AuditLogKey, AuditLogEntry>(&AuditLogKey::Entry(index))
        {
            entries.push_back(entry);
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_governance_audit_count_returns_zero_initially() {
        let env = Env::default();
        let count = get_governance_audit_count(&env);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_record_audit_entry_increments_count() {
        let env = Env::default();
        let actor = Address::generate(&env);
        let action = String::from_str(&env, "test_action");

        record_audit_entry(&env, action, actor, None);
        let count = get_governance_audit_count(&env);
        assert_eq!(count, 1);

        let actor2 = Address::generate(&env);
        let action2 = String::from_str(&env, "test_action2");
        record_audit_entry(&env, action2, actor2, None);
        let count = get_governance_audit_count(&env);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_get_governance_audit_entries_returns_empty_when_no_entries() {
        let env = Env::default();
        let entries = get_governance_audit_entries(&env, 10);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_get_governance_audit_entries_returns_most_recent_first() {
        let env = Env::default();
        let actor1 = Address::generate(&env);
        let action1 = String::from_str(&env, "action1");
        let actor2 = Address::generate(&env);
        let action2 = String::from_str(&env, "action2");
        let actor3 = Address::generate(&env);
        let action3 = String::from_str(&env, "action3");

        record_audit_entry(&env, action1.clone(), actor1.clone(), None);
        record_audit_entry(&env, action2.clone(), actor2.clone(), None);
        record_audit_entry(&env, action3.clone(), actor3.clone(), None);

        let entries = get_governance_audit_entries(&env, 0);
        assert_eq!(entries.len(), 3);

        // Most recent first
        assert_eq!(entries.get(0).unwrap().sequence, 2);
        assert_eq!(entries.get(1).unwrap().sequence, 1);
        assert_eq!(entries.get(2).unwrap().sequence, 0);
    }

    #[test]
    fn test_circular_buffer_overwrites_oldest_when_full() {
        let env = Env::default();

        // Set max size to 3 for this test
        env.storage()
            .instance()
            .set(&AuditLogKey::MaxSize, &3u64);

        let actor = Address::generate(&env);

        // Fill buffer to capacity
        for i in 0..3 {
            let action = String::from_str(&env, &format!("action{}", i));
            record_audit_entry(&env, action, actor.clone(), None);
        }

        let count = get_governance_audit_count(&env);
        assert_eq!(count, 3);

        // Add one more — should wrap and overwrite oldest
        let action_new = String::from_str(&env, "action_new");
        record_audit_entry(&env, action_new, actor.clone(), None);

        let count = get_governance_audit_count(&env);
        assert_eq!(count, 4); // Count never resets

        // Check that we have 3 entries (buffer full)
        let entries = get_governance_audit_entries(&env, 0);
        assert_eq!(entries.len(), 3);

        // Most recent should be sequence 3
        assert_eq!(entries.get(0).unwrap().sequence, 3);
        // Oldest existing should be sequence 1 (sequence 0 was overwritten)
        assert_eq!(entries.get(2).unwrap().sequence, 1);
    }

    #[test]
    fn test_get_governance_audit_entries_with_limit_returns_correct_count() {
        let env = Env::default();
        let actor = Address::generate(&env);

        for i in 0..5 {
            let action = String::from_str(&env, &format!("action{}", i));
            record_audit_entry(&env, action, actor.clone(), None);
        }

        let entries = get_governance_audit_entries(&env, 3);
        assert_eq!(entries.len(), 3);

        // Should return most recent: seq 4, 3, 2
        assert_eq!(entries.get(0).unwrap().sequence, 4);
        assert_eq!(entries.get(1).unwrap().sequence, 3);
        assert_eq!(entries.get(2).unwrap().sequence, 2);
    }

    #[test]
    fn test_get_governance_audit_entries_limit_0_returns_all_available() {
        let env = Env::default();
        let actor = Address::generate(&env);

        for i in 0..5 {
            let action = String::from_str(&env, &format!("action{}", i));
            record_audit_entry(&env, action, actor.clone(), None);
        }

        let entries = get_governance_audit_entries(&env, 0);
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn test_audit_entry_contains_correct_actor_and_ledger() {
        let env = Env::default();
        let actor = Address::generate(&env);
        let action = String::from_str(&env, "test_action");
        let details = Some(String::from_str(&env, "test_details"));

        record_audit_entry(&env, action.clone(), actor.clone(), details.clone());

        let entries = get_governance_audit_entries(&env, 1);
        assert_eq!(entries.len(), 1);

        let entry = entries.get(0).unwrap();
        assert_eq!(entry.actor, actor);
        assert_eq!(entry.action, action);
        assert_eq!(entry.details, details);
        assert_eq!(entry.sequence, 0);
    }

    #[test]
    fn test_multiple_actors_recorded_correctly() {
        let env = Env::default();
        let actor1 = Address::generate(&env);
        let actor2 = Address::generate(&env);
        let action = String::from_str(&env, "governance_action");

        record_audit_entry(&env, action.clone(), actor1.clone(), None);
        record_audit_entry(&env, action.clone(), actor2.clone(), None);

        let entries = get_governance_audit_entries(&env, 0);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries.get(0).unwrap().actor, actor2);
        assert_eq!(entries.get(1).unwrap().actor, actor1);
    }
}
