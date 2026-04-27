// contracts/data_store/src/data_store_test.rs
//
extern crate alloc;
use alloc::format;

// Comprehensive test suite for the DataStore contract.
//
// Coverage targets:
//   - All 5 entry points: data_save, data_load, data_backup, data_restore,
//     data_migrate_bump_version
//   - All error paths (every DataStoreError variant)
//   - Authorization boundaries (admin, writer, stranger)
//   - Boundary/edge values (empty key, max-length key, max-length value)
//   - State invariants (entry count, key index integrity)
//   - Idempotent operations (overwrite, re-backup, re-grant)

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env, String};

use crate::data_store::{DataStore, DataStoreClient};

// ═══════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════

/// Spin up a fresh Env + registered contract, returning the client.
#[allow(deprecated)]
fn setup() -> (Env, DataStoreClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(DataStore, ());
    let client = DataStoreClient::new(&env, &id);
    (env, client)
}

/// setup() + init with a fresh admin.
fn setup_init() -> (Env, DataStoreClient<'static>, Address) {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.init(&admin);
    (env, client, admin)
}

/// setup_init() + one granted writer.
fn setup_with_writer() -> (Env, DataStoreClient<'static>, Address, Address) {
    let (env, client, admin) = setup_init();
    let writer = Address::generate(&env);
    client.grant_writer(&admin, &writer);
    (env, client, admin, writer)
}

/// Build a `String` from a Rust `&str` for the test Env.
fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

/// Build a `Bytes` payload from a `&[u8]` slice.
fn b(env: &Env, v: &[u8]) -> Bytes {
    Bytes::from_slice(env, v)
}

/// Build a `Bytes` payload of exactly `n` bytes (all zeroes).
fn bytes_of_len(env: &Env, n: usize) -> Bytes {
    let mut b = Bytes::new(env);
    for _ in 0..n {
        b.push_back(0u8);
    }
    b
}

/// Build a `String` of exactly `n` bytes ('a' repeated).
fn string_of_len(env: &Env, n: usize) -> String {
    String::from_str(env, &"a".repeat(n))
}

// ═══════════════════════════════════════════════════════
// 1. Initialisation
// ═══════════════════════════════════════════════════════

#[test]
fn test_init_sets_admin() {
    let (_env, client, admin) = setup_init();
    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_init_sets_schema_version_zero() {
    let (_, client, _) = setup_init();
    assert_eq!(client.schema_version(), 0);
}

#[test]
fn test_init_entry_count_zero() {
    let (_, client, _) = setup_init();
    assert_eq!(client.entry_count(), 0);
}

#[test]
#[should_panic]
fn test_init_twice_panics() {
    let (_env, client, admin) = setup_init();
    client.init(&admin); // second call → AlreadyInitialized
}

#[test]
#[should_panic]
fn test_data_save_before_init_panics() {
    let (env, client) = setup();
    let addr = Address::generate(&env);
    client.data_save(&addr, &s(&env, "k"), &b(&env, b"v"));
}

// ═══════════════════════════════════════════════════════
// 2. data_save
// ═══════════════════════════════════════════════════════

#[test]
fn test_admin_can_save() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "key1"), &b(&env, b"value1"));
    assert_eq!(client.data_load(&s(&env, "key1")), b(&env, b"value1"));
}

#[test]
fn test_writer_can_save() {
    let (env, client, _, writer) = setup_with_writer();
    client.data_save(&writer, &s(&env, "w_key"), &b(&env, b"w_val"));
    assert_eq!(client.data_load(&s(&env, "w_key")), b(&env, b"w_val"));
}

#[test]
#[should_panic]
fn test_stranger_cannot_save() {
    let (env, client, _) = setup_init();
    let stranger = Address::generate(&env);
    client.data_save(&stranger, &s(&env, "k"), &b(&env, b"v"));
}

#[test]
fn test_save_increments_entry_count() {
    let (env, client, admin) = setup_init();
    assert_eq!(client.entry_count(), 0);
    client.data_save(&admin, &s(&env, "a"), &b(&env, b"1"));
    assert_eq!(client.entry_count(), 1);
    client.data_save(&admin, &s(&env, "b"), &b(&env, b"2"));
    assert_eq!(client.entry_count(), 2);
}

#[test]
fn test_overwrite_does_not_increment_count() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v1"));
    assert_eq!(client.entry_count(), 1);
    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v2")); // overwrite
    assert_eq!(client.entry_count(), 1);
    assert_eq!(client.data_load(&s(&env, "k")), b(&env, b"v2"));
}

#[test]
fn test_save_empty_value_allowed() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "empty"), &b(&env, b""));
    assert_eq!(client.data_load(&s(&env, "empty")), b(&env, b""));
}

#[test]
fn test_save_max_key_length_allowed() {
    let (env, client, admin) = setup_init();
    let max_key = string_of_len(&env, 64); // exactly MAX_KEY_LEN
    client.data_save(&admin, &max_key, &b(&env, b"v"));
    assert!(client.key_exists(&max_key));
}

#[test]
#[should_panic]
fn test_save_key_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long_key = string_of_len(&env, 65); // MAX_KEY_LEN + 1
    client.data_save(&admin, &long_key, &b(&env, b"v"));
}

#[test]
fn test_save_max_value_size_allowed() {
    let (env, client, admin) = setup_init();
    let max_val = bytes_of_len(&env, 4096); // exactly MAX_VALUE_LEN
    client.data_save(&admin, &s(&env, "big"), &max_val);
    assert_eq!(client.data_load(&s(&env, "big")), max_val);
}

#[test]
#[should_panic]
fn test_save_value_too_large_panics() {
    let (env, client, admin) = setup_init();
    let huge = bytes_of_len(&env, 4097); // MAX_VALUE_LEN + 1
    client.data_save(&admin, &s(&env, "huge"), &huge);
}

#[test]
fn test_key_exists_returns_true_after_save() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "x"), &b(&env, b"y"));
    assert!(client.key_exists(&s(&env, "x")));
}

#[test]
fn test_key_exists_returns_false_before_save() {
    let (env, client, _) = setup_init();
    assert!(!client.key_exists(&s(&env, "ghost")));
}

// ═══════════════════════════════════════════════════════
// 3. data_load
// ═══════════════════════════════════════════════════════

#[test]
fn test_load_returns_correct_value() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "greet"), &b(&env, b"hello world"));
    let got = client.data_load(&s(&env, "greet"));
    assert_eq!(got, b(&env, b"hello world"));
}

#[test]
#[should_panic]
fn test_load_missing_key_panics() {
    let (env, client, _) = setup_init();
    client.data_load(&s(&env, "nope"));
}

#[test]
fn test_load_is_public_no_auth_needed() {
    // data_load doesn't require any auth — covered by the fact we call it
    // without any special caller in every other test. Explicit assertion:
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "pub"), &b(&env, b"data"));
    // Any address (or no caller) can read
    let val = client.data_load(&s(&env, "pub"));
    assert_eq!(val, b(&env, b"data"));
}

#[test]
fn test_load_reflects_latest_overwrite() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "ver"), &b(&env, b"v1"));
    client.data_save(&admin, &s(&env, "ver"), &b(&env, b"v2"));
    client.data_save(&admin, &s(&env, "ver"), &b(&env, b"v3"));
    assert_eq!(client.data_load(&s(&env, "ver")), b(&env, b"v3"));
}

// ═══════════════════════════════════════════════════════
// 4. data_backup
// ═══════════════════════════════════════════════════════

#[test]
fn test_backup_captures_all_entries() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "k1"), &b(&env, b"v1"));
    client.data_save(&admin, &s(&env, "k2"), &b(&env, b"v2"));
    client.data_save(&admin, &s(&env, "k3"), &b(&env, b"v3"));

    // Backup should succeed without panicking
    client.data_backup(&admin, &s(&env, "snap1"));
}

#[test]
fn test_writer_can_backup() {
    let (env, client, admin, writer) = setup_with_writer();
    client.data_save(&admin, &s(&env, "x"), &b(&env, b"y"));
    client.data_backup(&writer, &s(&env, "w_snap")); // writer can backup
}

#[test]
#[should_panic]
fn test_stranger_cannot_backup() {
    let (env, client, admin) = setup_init();
    let stranger = Address::generate(&env);
    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v"));
    client.data_backup(&stranger, &s(&env, "snap"));
}

#[test]
fn test_backup_empty_store() {
    let (env, client, admin) = setup_init();
    client.data_backup(&admin, &s(&env, "empty_snap")); // must not panic
}

#[test]
#[should_panic]
fn test_backup_name_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long_name = string_of_len(&env, 33); // MAX_BACKUP_NAME + 1
    client.data_backup(&admin, &long_name);
}

#[test]
fn test_backup_max_name_allowed() {
    let (env, client, admin) = setup_init();
    let name = string_of_len(&env, 32); // exactly MAX_BACKUP_NAME
    client.data_backup(&admin, &name); // must not panic
}

#[test]
fn test_backup_overwrites_previous_snapshot() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v1"));
    client.data_backup(&admin, &s(&env, "snap"));

    // Mutate store
    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v2"));
    client.data_backup(&admin, &s(&env, "snap")); // overwrite backup

    // Restore → should get v2
    client.data_restore(&admin, &s(&env, "snap"));
    assert_eq!(client.data_load(&s(&env, "k")), b(&env, b"v2"));
}

// ═══════════════════════════════════════════════════════
// 5. data_restore
// ═══════════════════════════════════════════════════════

#[test]
fn test_restore_replaces_all_entries() {
    let (env, client, admin) = setup_init();

    // Save initial state and back up
    client.data_save(&admin, &s(&env, "a"), &b(&env, b"alpha"));
    client.data_save(&admin, &s(&env, "b"), &b(&env, b"beta"));
    client.data_backup(&admin, &s(&env, "v1"));

    // Overwrite and add new key
    client.data_save(&admin, &s(&env, "a"), &b(&env, b"CHANGED"));
    client.data_save(&admin, &s(&env, "c"), &b(&env, b"gamma"));
    assert_eq!(client.entry_count(), 3);

    // Restore to v1
    client.data_restore(&admin, &s(&env, "v1"));

    // Original values restored
    assert_eq!(client.data_load(&s(&env, "a")), b(&env, b"alpha"));
    assert_eq!(client.data_load(&s(&env, "b")), b(&env, b"beta"));

    // "c" was added after backup — must not exist anymore
    assert!(!client.key_exists(&s(&env, "c")));

    // Count matches the backup
    assert_eq!(client.entry_count(), 2);
}

#[test]
fn test_restore_to_empty_backup_clears_store() {
    let (env, client, admin) = setup_init();

    client.data_backup(&admin, &s(&env, "empty_snap")); // empty snapshot

    client.data_save(&admin, &s(&env, "x"), &b(&env, b"y"));
    assert_eq!(client.entry_count(), 1);

    client.data_restore(&admin, &s(&env, "empty_snap"));
    assert_eq!(client.entry_count(), 0);
    assert!(!client.key_exists(&s(&env, "x")));
}

#[test]
#[should_panic]
fn test_restore_missing_backup_panics() {
    let (env, client, admin) = setup_init();
    client.data_restore(&admin, &s(&env, "ghost_snap")); // no such backup
}

#[test]
#[should_panic]
fn test_writer_cannot_restore() {
    let (env, client, admin, writer) = setup_with_writer();
    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v"));
    client.data_backup(&admin, &s(&env, "snap"));
    client.data_restore(&writer, &s(&env, "snap")); // writer not allowed
}

#[test]
#[should_panic]
fn test_stranger_cannot_restore() {
    let (env, client, admin) = setup_init();
    let stranger = Address::generate(&env);
    client.data_backup(&admin, &s(&env, "snap"));
    client.data_restore(&stranger, &s(&env, "snap"));
}

#[test]
fn test_backup_preserved_after_restore() {
    // The backup itself should still exist after restore (can be re-applied)
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v"));
    client.data_backup(&admin, &s(&env, "snap"));
    client.data_restore(&admin, &s(&env, "snap")); // first restore
    client.data_restore(&admin, &s(&env, "snap")); // second restore — must not panic
}

#[test]
fn test_restore_is_idempotent() {
    let (env, client, admin) = setup_init();
    client.data_save(&admin, &s(&env, "stable"), &b(&env, b"value"));
    client.data_backup(&admin, &s(&env, "snap"));
    client.data_restore(&admin, &s(&env, "snap"));
    client.data_restore(&admin, &s(&env, "snap")); // second time same state
    assert_eq!(client.data_load(&s(&env, "stable")), b(&env, b"value"));
}

// ═══════════════════════════════════════════════════════
// 6. data_migrate_bump_version
// ═══════════════════════════════════════════════════════

#[test]
fn test_migrate_bumps_version() {
    let (_env, client, admin) = setup_init();
    assert_eq!(client.schema_version(), 0);
    client.data_migrate_bump_version(&admin, &1, &None);
    assert_eq!(client.schema_version(), 1);
}

#[test]
fn test_migrate_can_skip_versions() {
    let (_env, client, admin) = setup_init();
    client.data_migrate_bump_version(&admin, &42, &None);
    assert_eq!(client.schema_version(), 42);
}

#[test]
fn test_migrate_sequential_bumps() {
    let (env, client, admin) = setup_init();
    client.data_migrate_bump_version(&admin, &1, &None);
    client.data_migrate_bump_version(&admin, &2, &None);
    client.data_migrate_bump_version(&admin, &3, &Some(s(&env, "added indexes")));
    assert_eq!(client.schema_version(), 3);
}

#[test]
fn test_migrate_with_memo() {
    let (env, client, admin) = setup_init();
    let memo = s(&env, "Restructured value encoding to CBOR");
    client.data_migrate_bump_version(&admin, &1, &Some(memo));
    assert_eq!(client.schema_version(), 1);
}

#[test]
#[should_panic]
fn test_migrate_same_version_panics() {
    let (_env, client, admin) = setup_init();
    client.data_migrate_bump_version(&admin, &0, &None); // same as current
}

#[test]
#[should_panic]
fn test_migrate_lower_version_panics() {
    let (_env, client, admin) = setup_init();
    client.data_migrate_bump_version(&admin, &5, &None);
    client.data_migrate_bump_version(&admin, &3, &None); // rollback attempt
}

#[test]
#[should_panic]
fn test_writer_cannot_migrate() {
    let (_env, client, _, writer) = setup_with_writer();
    client.data_migrate_bump_version(&writer, &1, &None);
}

#[test]
#[should_panic]
fn test_stranger_cannot_migrate() {
    let (env, client, _) = setup_init();
    let stranger = Address::generate(&env);
    client.data_migrate_bump_version(&stranger, &1, &None);
}

#[test]
#[should_panic]
fn test_migrate_memo_too_long_panics() {
    let (env, client, admin) = setup_init();
    let long_memo = string_of_len(&env, 129); // >128 bytes
    client.data_migrate_bump_version(&admin, &1, &Some(long_memo));
}

// ═══════════════════════════════════════════════════════
// 7. Writer management
// ═══════════════════════════════════════════════════════

#[test]
fn test_grant_writer_allows_write() {
    let (_env, client, _admin, writer) = setup_with_writer();
    assert!(client.is_writer(&writer));
}

#[test]
fn test_revoke_writer_removes_write_access() {
    let (_env, client, admin, writer) = setup_with_writer();
    client.revoke_writer(&admin, &writer);
    assert!(!client.is_writer(&writer));
}

#[test]
#[should_panic]
fn test_revoked_writer_cannot_save() {
    let (env, client, admin, writer) = setup_with_writer();
    client.revoke_writer(&admin, &writer);
    client.data_save(&writer, &s(&env, "k"), &b(&env, b"v")); // must panic
}

#[test]
fn test_grant_writer_idempotent() {
    let (_env, client, admin, writer) = setup_with_writer();
    client.grant_writer(&admin, &writer); // re-grant
    client.grant_writer(&admin, &writer); // and again
    assert!(client.is_writer(&writer)); // still a writer
}

#[test]
#[should_panic]
fn test_stranger_cannot_grant_writer() {
    let (env, client, _, _) = setup_with_writer();
    let stranger = Address::generate(&env);
    let target = Address::generate(&env);
    client.grant_writer(&stranger, &target);
}

#[test]
fn test_admin_is_always_a_writer() {
    let (_env, client, admin) = setup_init();
    assert!(client.is_writer(&admin));
}

#[test]
fn test_revoke_nonexistent_writer_is_noop() {
    let (env, client, admin) = setup_init();
    let ghost = Address::generate(&env);
    client.revoke_writer(&admin, &ghost); // must not panic
}

// ═══════════════════════════════════════════════════════
// 8. Integration — combined scenario
// ═══════════════════════════════════════════════════════

#[test]
fn test_full_lifecycle_save_backup_migrate_restore() {
    let (env, client, admin) = setup_init();

    // Phase 1: populate store at v0
    client.data_save(&admin, &s(&env, "config.network"), &b(&env, b"mainnet"));
    client.data_save(&admin, &s(&env, "config.timeout"), &b(&env, b"30"));
    client.data_save(&admin, &s(&env, "config.retry"), &b(&env, b"3"));
    assert_eq!(client.entry_count(), 3);

    // Phase 2: backup v0 state
    client.data_backup(&admin, &s(&env, "v0_snapshot"));

    // Phase 3: migrate to v1 — change a value
    client.data_migrate_bump_version(&admin, &1, &Some(s(&env, "renamed timeout to deadline")));
    client.data_save(&admin, &s(&env, "config.deadline"), &b(&env, b"60"));
    assert_eq!(client.schema_version(), 1);
    assert_eq!(client.entry_count(), 4);

    // Phase 4: something goes wrong — restore from v0
    client.data_restore(&admin, &s(&env, "v0_snapshot"));

    // Verify restored state
    assert_eq!(
        client.data_load(&s(&env, "config.network")),
        b(&env, b"mainnet")
    );
    assert_eq!(client.data_load(&s(&env, "config.timeout")), b(&env, b"30"));
    assert_eq!(client.entry_count(), 3);
    assert!(!client.key_exists(&s(&env, "config.deadline")));

    // Schema version is NOT rolled back by restore — it's a separate concern
    assert_eq!(client.schema_version(), 1);
}

#[test]
fn test_multiple_writers_independent() {
    let (env, client, admin) = setup_init();
    let writer_a = Address::generate(&env);
    let writer_b = Address::generate(&env);

    client.grant_writer(&admin, &writer_a);
    client.grant_writer(&admin, &writer_b);

    client.data_save(&writer_a, &s(&env, "a_key"), &b(&env, b"a_val"));
    client.data_save(&writer_b, &s(&env, "b_key"), &b(&env, b"b_val"));

    assert_eq!(client.data_load(&s(&env, "a_key")), b(&env, b"a_val"));
    assert_eq!(client.data_load(&s(&env, "b_key")), b(&env, b"b_val"));

    // Revoke writer_a — writer_b still works
    client.revoke_writer(&admin, &writer_a);
    client.data_save(&writer_b, &s(&env, "b_key"), &b(&env, b"b_val2"));
    assert_eq!(client.data_load(&s(&env, "b_key")), b(&env, b"b_val2"));
}

#[test]
fn test_backup_then_add_then_restore_removes_new_key() {
    let (env, client, admin) = setup_init();

    client.data_save(&admin, &s(&env, "old"), &b(&env, b"exists"));
    client.data_backup(&admin, &s(&env, "snap"));

    // Add a key AFTER backup
    client.data_save(&admin, &s(&env, "new"), &b(&env, b"added_after"));
    assert_eq!(client.entry_count(), 2);
    assert!(client.key_exists(&s(&env, "new")));

    // Restore wipes the new key
    client.data_restore(&admin, &s(&env, "snap"));
    assert_eq!(client.entry_count(), 1);
    assert!(!client.key_exists(&s(&env, "new")));
    assert!(client.key_exists(&s(&env, "old")));
}

// ═══════════════════════════════════════════════════════
// 9. Capacity / StoreFull boundary
// ═══════════════════════════════════════════════════════

#[test]
#[should_panic]
fn test_store_full_panics_on_new_key() {
    let (env, client, admin) = setup_init();

    // Fill the store to MAX_ENTRIES (1 000)
    for i in 0u32..1_000 {
        let key = String::from_str(&env, &format!("k{i:04}"));
        client.data_save(&admin, &key, &b(&env, b"v"));
    }
    assert_eq!(client.entry_count(), 1_000);

    // One more new key must panic with StoreFull
    client.data_save(&admin, &s(&env, "overflow"), &b(&env, b"x"));
}

#[test]
fn test_overwrite_at_capacity_does_not_panic() {
    let (env, client, admin) = setup_init();

    // Fill to MAX_ENTRIES
    for i in 0u32..1_000 {
        let key = String::from_str(&env, &format!("k{i:04}"));
        client.data_save(&admin, &key, &b(&env, b"v"));
    }
    assert_eq!(client.entry_count(), 1_000);

    // Overwriting an existing key at capacity must succeed
    client.data_save(&admin, &s(&env, "k0000"), &b(&env, b"overwrite"));
    assert_eq!(client.entry_count(), 1_000);
    assert_eq!(client.data_load(&s(&env, "k0000")), b(&env, b"overwrite"));
}

// ═══════════════════════════════════════════════════════
// 10. Multiple independent backups
// ═══════════════════════════════════════════════════════

#[test]
fn test_two_backups_are_independent() {
    let (env, client, admin) = setup_init();

    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v1"));
    client.data_backup(&admin, &s(&env, "snap_a"));

    client.data_save(&admin, &s(&env, "k"), &b(&env, b"v2"));
    client.data_backup(&admin, &s(&env, "snap_b"));

    // Restoring snap_a should give v1
    client.data_restore(&admin, &s(&env, "snap_a"));
    assert_eq!(client.data_load(&s(&env, "k")), b(&env, b"v1"));

    // Restoring snap_b should give v2
    client.data_restore(&admin, &s(&env, "snap_b"));
    assert_eq!(client.data_load(&s(&env, "k")), b(&env, b"v2"));
}

#[test]
fn test_restore_then_backup_captures_restored_state() {
    let (env, client, admin) = setup_init();

    client.data_save(&admin, &s(&env, "x"), &b(&env, b"original"));
    client.data_backup(&admin, &s(&env, "v1"));

    // Mutate and restore
    client.data_save(&admin, &s(&env, "x"), &b(&env, b"changed"));
    client.data_restore(&admin, &s(&env, "v1"));

    // Taking a new backup after restore captures the restored state
    client.data_backup(&admin, &s(&env, "v2"));
    client.data_save(&admin, &s(&env, "x"), &b(&env, b"again"));
    client.data_restore(&admin, &s(&env, "v2"));

    assert_eq!(client.data_load(&s(&env, "x")), b(&env, b"original"));
}
