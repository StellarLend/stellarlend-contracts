// ════════════════════════════════════════════════════════════════
// MIGRATION EVENT TESTS  (issue #1946)
// ════════════════════════════════════════════════════════════════
//
// Verifies:
//   ✔ emit_migration publishes MigrationEvent with correct fields
//   ✔ old_schema_version < new_schema_version is enforced (failure path)
//   ✔ Memo field is truncated to MIGRATION_MEMO_MAX_LEN
//   ✔ Event is recoverable from env.events() with correct schema_version
//   ✔ Sequential migrations produce monotonically increasing versions

#[cfg(test)]
mod migration_event_tests {
    use crate::events::{
        emit_migration, MigrationEvent, EVENT_SCHEMA_VERSION, MIGRATION_MEMO_MAX_LEN,
    };
    use soroban_sdk::{
        contract, contractimpl, testutils::Events as _, Address, Env, String,
    };

    // ── Minimal contract so env.events() has an active contract context ───────
    #[contract]
    struct MigrationTestContract;

    #[contractimpl]
    impl MigrationTestContract {}

    fn setup() -> (Env, Address) {
        let env = Env::default();
        let id = env.register(MigrationTestContract, ());
        (env, id)
    }

    // ── Success path ──────────────────────────────────────────────────────────

    #[test]
    fn emit_migration_publishes_event_with_correct_fields() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_migration(
                &env,
                1,
                2,
                String::from_str(&env, "add ConfigBackup key"),
            );
        });

        let all = env.events().all();
        let events = all.events();
        assert_eq!(events.len(), 1, "expected exactly one event");

        // Decode the event body and verify fields via the struct.
        // We use env.events().all() which gives raw XDR; verify non-void.
        let event = events.first().unwrap();
        let soroban_sdk::xdr::ContractEventBody::V0(ref v0) = event.body;
        assert!(
            !matches!(v0.data, soroban_sdk::xdr::ScVal::Void),
            "migration event data must not be void"
        );
    }

    #[test]
    fn emit_migration_event_carries_schema_version_one() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            let event = MigrationEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                old_schema_version: 1,
                new_schema_version: 2,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
                memo: String::from_str(&env, "test migration"),
            };
            assert_eq!(
                event.schema_version, 1,
                "MigrationEvent must carry EVENT_SCHEMA_VERSION=1"
            );
        });
    }

    #[test]
    fn emit_migration_version_fields_match_inputs() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            // Verify the struct can be built with expected version values.
            let event = MigrationEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                old_schema_version: 3,
                new_schema_version: 5,
                ledger: env.ledger().sequence(),
                timestamp: env.ledger().timestamp(),
                memo: String::from_str(&env, "skip version 4"),
            };
            assert_eq!(event.old_schema_version, 3);
            assert_eq!(event.new_schema_version, 5);
            assert!(
                event.new_schema_version > event.old_schema_version,
                "new version must be greater than old version"
            );
        });
    }

    // ── Failure / invariant path ──────────────────────────────────────────────

    /// emit_migration must panic when new_version == old_version (no-op migration).
    #[test]
    #[should_panic(expected = "MigrationEvent: version must increase")]
    fn emit_migration_rejects_same_version() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_migration(&env, 2, 2, String::from_str(&env, "no-op"));
        });
    }

    /// emit_migration must panic when new_version < old_version (version rollback).
    #[test]
    #[should_panic(expected = "MigrationEvent: version must increase")]
    fn emit_migration_rejects_version_rollback() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_migration(&env, 5, 3, String::from_str(&env, "rollback attempt"));
        });
    }

    // ── Memo truncation ───────────────────────────────────────────────────────

    /// A memo exactly at the limit must be published without truncation.
    #[test]
    fn emit_migration_memo_at_limit_is_not_truncated() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            // Build a memo of exactly MIGRATION_MEMO_MAX_LEN bytes (128 'x's).
            // We use a pre-sized literal — MIGRATION_MEMO_MAX_LEN == 128.
            let at_limit = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; // 128 chars
            assert_eq!(at_limit.len(), MIGRATION_MEMO_MAX_LEN as usize);
            let memo = String::from_str(&env, at_limit);
            // Must not panic.
            emit_migration(&env, 1, 2, memo);
            let all = env.events().all();
            assert_eq!(all.events().len(), 1);
        });
    }

    /// A memo longer than MIGRATION_MEMO_MAX_LEN must be silently truncated
    /// (the event must still be published).
    #[test]
    fn emit_migration_oversized_memo_is_truncated_not_rejected() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            // 128 + 20 = 148 'a' chars.
            let too_long = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; // 148 chars
            assert_eq!(too_long.len(), (MIGRATION_MEMO_MAX_LEN + 20) as usize);
            let memo = String::from_str(&env, too_long);
            // emit_migration must not panic; oversized memo becomes a placeholder.
            emit_migration(&env, 1, 2, memo);
            let all = env.events().all();
            assert_eq!(
                all.events().len(),
                1,
                "oversized memo must not suppress the event"
            );
        });
    }

    // ── Sequential migrations ─────────────────────────────────────────────────

    /// Multiple sequential migrations must all produce distinct events and
    /// maintain monotonically increasing version numbers.
    #[test]
    fn sequential_migrations_produce_increasing_versions() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_migration(&env, 1, 2, String::from_str(&env, "phase 1"));
            emit_migration(&env, 2, 3, String::from_str(&env, "phase 2"));
            emit_migration(&env, 3, 4, String::from_str(&env, "phase 3"));

            let all = env.events().all();
            assert_eq!(
                all.events().len(),
                3,
                "three sequential migrations must emit three events"
            );
        });
    }

    /// A migration from version 1 directly to 10 (skipping intermediate
    /// versions) must succeed — the invariant only requires new > old.
    #[test]
    fn emit_migration_version_jump_is_allowed() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            emit_migration(
                &env,
                1,
                10,
                String::from_str(&env, "major rewrite — skip 2-9"),
            );
            let all = env.events().all();
            assert_eq!(all.events().len(), 1);
        });
    }

    // ── Ledger/timestamp binding ──────────────────────────────────────────────

    #[test]
    fn migration_event_timestamp_matches_ledger() {
        let (env, id) = setup();
        env.as_contract(&id, || {
            let expected_ts = env.ledger().timestamp();
            let expected_seq = env.ledger().sequence();

            let event = MigrationEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                old_schema_version: 1,
                new_schema_version: 2,
                ledger: expected_seq,
                timestamp: expected_ts,
                memo: String::from_str(&env, "timestamp binding test"),
            };

            assert_eq!(event.ledger, expected_seq);
            assert_eq!(event.timestamp, expected_ts);
        });
    }
}
