// contracts/data_store/src/data_store.rs
//
// # DataStore — Soroban Key-Value Storage Contract
//
// Provides five entry points for secure, versioned, admin-gated persistent
// key-value storage on the Stellar / Soroban network.
//
// ## Entry points
//
// | Function                    | Who can call              | Description                     |
// |-----------------------------|---------------------------|---------------------------------|
// | `data_save`                 | Admin or Writer           | Write or overwrite a key-value  |
// | `data_load`                 | Anyone                    | Read a value by key             |
// | `data_backup`               | Admin or Writer           | Snapshot all entries            |
// | `data_restore`              | Admin only                | Restore from a named snapshot   |
// | `data_migrate_bump_version` | Admin only                | Atomically bump schema version  |
//
// ## Security model
//
// - Only the admin (set at `init`) or addresses explicitly granted `Writer`
//   access may call `data_save`, `data_backup`, or `data_restore`.
// - Reads (`data_load`) are public.
// - All inputs are bounds-checked to prevent DoS via oversized payloads.
// - The admin is the sole address allowed to call `data_restore` and
//   `data_migrate_bump_version`.
//
// ## Size limits (compile-time constants)
//
// | Limit              | Value  | Rationale                                      |
// |--------------------|--------|------------------------------------------------|
// | `MAX_KEY_LEN`      | 64 B   | Soroban symbol/string practical limit          |
// | `MAX_VALUE_LEN`    | 4 096 B| Reasonable payload; prevents ledger bloat      |
// | `MAX_ENTRIES`      | 1 000  | Caps iteration cost in backup/restore          |
// | `MAX_BACKUP_NAME`  | 32 B   | Short identifier, avoids key-space collision   |

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, Address,
    Bytes, Env, String, Vec,
};

// ═══════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════

/// Maximum byte length of a storage key.
pub const MAX_KEY_LEN: u32 = 64;

/// Maximum byte length of a stored value.
pub const MAX_VALUE_LEN: u32 = 4_096;

/// Maximum number of key-value entries the store may hold.
pub const MAX_ENTRIES: u32 = 1_000;

/// Maximum byte length of a backup name.
pub const MAX_BACKUP_NAME: u32 = 32;

// ═══════════════════════════════════════════════════════
// Error codes
// ═══════════════════════════════════════════════════════

/// All errors emitted by the DataStore contract.
///
/// Errors are surfaced as `u32` codes in the Soroban result envelope so
/// that callers can pattern-match them programmatically.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DataStoreError {
    /// Contract has already been initialised; `init` cannot be called again.
    AlreadyInitialized = 1,
    /// Caller is neither the admin nor a granted writer.
    NotAuthorized = 2,
    /// Key exceeds `MAX_KEY_LEN` bytes.
    KeyTooLong = 3,
    /// Value exceeds `MAX_VALUE_LEN` bytes.
    ValueTooLarge = 4,
    /// The requested key does not exist in the store.
    KeyNotFound = 5,
    /// The requested backup snapshot does not exist.
    BackupNotFound = 6,
    /// Adding another entry would exceed `MAX_ENTRIES`.
    StoreFull = 7,
    /// Backup name exceeds `MAX_BACKUP_NAME` bytes.
    BackupNameTooLong = 8,
    /// Contract has not been initialised yet.
    NotInitialized = 9,
    /// New schema version must be strictly greater than the current one.
    InvalidVersion = 10,
}

// ═══════════════════════════════════════════════════════
// Storage key types
// ═══════════════════════════════════════════════════════

/// Top-level storage keys used by the DataStore contract.
///
/// Soroban persistent storage is a flat key-value map; we namespace all
/// our entries under typed variants of this enum to avoid collisions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreKey {
    /// Marks the contract as initialised; value is the admin `Address`.
    Admin,
    /// Set of addresses granted write access by the admin.
    Writers,
    /// Current schema/migration version (u32).
    SchemaVersion,
    /// Counter: number of live key-value entries.
    EntryCount,
    /// A single user-defined key-value entry.
    Entry(String),
    /// A named backup snapshot (Map<String, Bytes>).
    Backup(String),
    /// Index of all live entry keys, for backup enumeration.
    KeyIndex,
}

// ═══════════════════════════════════════════════════════
// Contract
// ═══════════════════════════════════════════════════════

#[contract]
pub struct DataStore;

#[contractimpl]
impl DataStore {
    // ───────────────────────────────────────────────────
    // Initialisation
    // ───────────────────────────────────────────────────

    /// Initialise the contract and designate the first admin.
    ///
    /// # Arguments
    /// * `admin` — The address that will hold full administrative control.
    ///
    /// # Errors
    /// * `AlreadyInitialized` — if `init` has already been called.
    ///
    /// # Authorization
    /// `admin` must sign the transaction.
    #[allow(deprecated)]
    pub fn init(env: Env, admin: Address) {
        admin.require_auth();

        if env.storage().persistent().has(&StoreKey::Admin) {
            panic_with_error!(&env, DataStoreError::AlreadyInitialized);
        }

        env.storage().persistent().set(&StoreKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&StoreKey::SchemaVersion, &0u32);
        env.storage().persistent().set(&StoreKey::EntryCount, &0u32);

        // Initialise empty writers set and key index
        let writers: Vec<Address> = Vec::new(&env);
        env.storage().persistent().set(&StoreKey::Writers, &writers);

        let key_index: Vec<String> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&StoreKey::KeyIndex, &key_index);

        env.events()
            .publish((symbol_short!("ds_init"), admin.clone()), ());
    }

    // ───────────────────────────────────────────────────
    // Writer management
    // ───────────────────────────────────────────────────

    /// Grant write access to `writer`.
    ///
    /// # Authorization
    /// Only the admin may grant writers.
    #[allow(deprecated)]
    pub fn grant_writer(env: Env, caller: Address, writer: Address) {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let mut writers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StoreKey::Writers)
            .unwrap_or_else(|| Vec::new(&env));

        // Idempotent: skip if already present
        if !writers.contains(&writer) {
            writers.push_back(writer.clone());
            env.storage().persistent().set(&StoreKey::Writers, &writers);
        }

        env.events()
            .publish((symbol_short!("writer"), caller, writer), ());
    }

    /// Revoke write access from `writer`.
    ///
    /// # Authorization
    /// Only the admin may revoke writers.
    #[allow(deprecated)]
    pub fn revoke_writer(env: Env, caller: Address, writer: Address) {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let writers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StoreKey::Writers)
            .unwrap_or_else(|| Vec::new(&env));

        let mut new_writers: Vec<Address> = Vec::new(&env);
        for w in writers.iter() {
            if w != writer {
                new_writers.push_back(w);
            }
        }
        env.storage()
            .persistent()
            .set(&StoreKey::Writers, &new_writers);

        env.events()
            .publish((symbol_short!("writer"), caller, writer), ());
    }

    // ───────────────────────────────────────────────────
    // Entry point 1: data_save
    // ───────────────────────────────────────────────────

    /// Store `value` under `key`, creating or overwriting the entry.
    ///
    /// # Arguments
    /// * `caller` — The address authorising the write (admin or writer).
    /// * `key`    — UTF-8 string key, max `MAX_KEY_LEN` bytes.
    /// * `value`  — Arbitrary bytes, max `MAX_VALUE_LEN` bytes.
    ///
    /// # Errors
    /// * `NotAuthorized`  — caller lacks write permission.
    /// * `KeyTooLong`     — key exceeds `MAX_KEY_LEN`.
    /// * `ValueTooLarge`  — value exceeds `MAX_VALUE_LEN`.
    /// * `StoreFull`      — store already has `MAX_ENTRIES` entries and this
    ///                      is a new key.
    ///
    /// # Events
    /// Emits `(ds_save, caller, key)` on success.
    ///
    /// # Authorization
    /// `caller` must sign the transaction.
    #[allow(deprecated)]
    pub fn data_save(env: Env, caller: Address, key: String, value: Bytes) {
        caller.require_auth();
        Self::assert_initialized(&env);
        Self::assert_can_write(&env, &caller);

        // Bounds checks
        if key.len() > MAX_KEY_LEN {
            panic_with_error!(&env, DataStoreError::KeyTooLong);
        }
        if value.len() > MAX_VALUE_LEN {
            panic_with_error!(&env, DataStoreError::ValueTooLarge);
        }

        let store_key = StoreKey::Entry(key.clone());
        let is_new = !env.storage().persistent().has(&store_key);

        if is_new {
            // Capacity guard
            let count: u32 = env
                .storage()
                .persistent()
                .get(&StoreKey::EntryCount)
                .unwrap_or(0);

            if count >= MAX_ENTRIES {
                panic_with_error!(&env, DataStoreError::StoreFull);
            }

            // Update key index
            let mut key_index: Vec<String> = env
                .storage()
                .persistent()
                .get(&StoreKey::KeyIndex)
                .unwrap_or_else(|| Vec::new(&env));
            key_index.push_back(key.clone());
            env.storage()
                .persistent()
                .set(&StoreKey::KeyIndex, &key_index);

            env.storage()
                .persistent()
                .set(&StoreKey::EntryCount, &(count + 1));
        }

        env.storage().persistent().set(&store_key, &value);

        env.events()
            .publish((symbol_short!("ds_save"), caller, key), value.len());
    }

    // ───────────────────────────────────────────────────
    // Entry point 2: data_load
    // ───────────────────────────────────────────────────

    /// Load the value stored under `key`.
    ///
    /// # Arguments
    /// * `key` — The key to look up.
    ///
    /// # Returns
    /// The stored `Bytes` value.
    ///
    /// # Errors
    /// * `KeyNotFound` — no entry exists for `key`.
    ///
    /// # Authorization
    /// None — reads are public.
    pub fn data_load(env: Env, key: String) -> Bytes {
        Self::assert_initialized(&env);

        env.storage()
            .persistent()
            .get(&StoreKey::Entry(key.clone()))
            .unwrap_or_else(|| panic_with_error!(&env, DataStoreError::KeyNotFound))
    }

    // ───────────────────────────────────────────────────
    // Entry point 3: data_backup
    // ───────────────────────────────────────────────────

    /// Snapshot all current entries into a named backup.
    ///
    /// The backup is stored persistently under `Backup(backup_name)` as a
    /// `Vec<(String, Bytes)>` pair list, enabling exact restoration.
    ///
    /// Overwriting an existing backup name replaces the previous snapshot.
    ///
    /// # Arguments
    /// * `caller`      — Admin or writer.
    /// * `backup_name` — Identifier for this snapshot, max `MAX_BACKUP_NAME` bytes.
    ///
    /// # Errors
    /// * `NotAuthorized`     — caller lacks write permission.
    /// * `BackupNameTooLong` — name exceeds `MAX_BACKUP_NAME`.
    ///
    /// # Events
    /// Emits `(ds_bkup, caller, backup_name)` on success.
    ///
    /// # Authorization
    /// `caller` must sign the transaction.
    #[allow(deprecated)]
    pub fn data_backup(env: Env, caller: Address, backup_name: String) {
        caller.require_auth();
        Self::assert_initialized(&env);
        Self::assert_can_write(&env, &caller);

        if backup_name.len() > MAX_BACKUP_NAME {
            panic_with_error!(&env, DataStoreError::BackupNameTooLong);
        }

        // Walk the key index and snapshot each entry
        let key_index: Vec<String> = env
            .storage()
            .persistent()
            .get(&StoreKey::KeyIndex)
            .unwrap_or_else(|| Vec::new(&env));

        // Store as parallel Vec<String> keys + Vec<Bytes> values
        let mut snap_keys: Vec<String> = Vec::new(&env);
        let mut snap_vals: Vec<Bytes> = Vec::new(&env);

        for k in key_index.iter() {
            if let Some(v) = env
                .storage()
                .persistent()
                .get::<StoreKey, Bytes>(&StoreKey::Entry(k.clone()))
            {
                snap_keys.push_back(k);
                snap_vals.push_back(v);
            }
        }

        // Pack as (Vec<String>, Vec<Bytes>) tuple stored under one key
        env.storage().persistent().set(
            &StoreKey::Backup(backup_name.clone()),
            &(snap_keys, snap_vals),
        );

        env.events().publish(
            (symbol_short!("ds_bkup"), caller, backup_name),
            key_index.len(),
        );
    }

    // ───────────────────────────────────────────────────
    // Entry point 4: data_restore
    // ───────────────────────────────────────────────────

    /// Restore the store to the state captured in a named backup.
    ///
    /// **All current live entries are replaced** by the snapshot contents.
    /// The backup itself is preserved so it can be re-applied.
    ///
    /// # Arguments
    /// * `caller`      — Must be the admin.
    /// * `backup_name` — Name of the backup to restore from.
    ///
    /// # Errors
    /// * `NotAuthorized`  — caller is not the admin.
    /// * `BackupNotFound` — no backup with that name exists.
    ///
    /// # Events
    /// Emits `(ds_rest, caller, backup_name)` on success.
    ///
    /// # Security note
    /// Only the admin can restore — this operation is destructive and
    /// cannot be undone without another backup.
    #[allow(deprecated)]
    pub fn data_restore(env: Env, caller: Address, backup_name: String) {
        caller.require_auth();
        Self::assert_initialized(&env);
        Self::assert_admin(&env, &caller);

        let snapshot: Option<(Vec<String>, Vec<Bytes>)> = env
            .storage()
            .persistent()
            .get(&StoreKey::Backup(backup_name.clone()));

        let (snap_keys, snap_vals) = match snapshot {
            Some(s) => s,
            None => panic_with_error!(&env, DataStoreError::BackupNotFound),
        };

        // 1. Remove all existing live entries
        let old_key_index: Vec<String> = env
            .storage()
            .persistent()
            .get(&StoreKey::KeyIndex)
            .unwrap_or_else(|| Vec::new(&env));

        for k in old_key_index.iter() {
            env.storage().persistent().remove(&StoreKey::Entry(k));
        }

        // 2. Write snapshot entries
        let mut new_key_index: Vec<String> = Vec::new(&env);
        let snap_len = snap_keys.len();

        for i in 0..snap_len {
            let k = snap_keys.get(i).unwrap();
            let v = snap_vals.get(i).unwrap();
            env.storage()
                .persistent()
                .set(&StoreKey::Entry(k.clone()), &v);
            new_key_index.push_back(k);
        }

        // 3. Update metadata
        env.storage()
            .persistent()
            .set(&StoreKey::KeyIndex, &new_key_index);
        env.storage()
            .persistent()
            .set(&StoreKey::EntryCount, &snap_len);

        env.events()
            .publish((symbol_short!("ds_rest"), caller, backup_name), snap_len);
    }

    // ───────────────────────────────────────────────────
    // Entry point 5: data_migrate_bump_version
    // ───────────────────────────────────────────────────

    /// Atomically bump the schema version.
    ///
    /// Used during migrations to record that the data layout has advanced
    /// to a new version. The new version must be strictly greater than the
    /// current one to prevent accidental rollbacks.
    ///
    /// An optional memo string can document what changed in this migration.
    ///
    /// # Arguments
    /// * `caller`      — Must be the admin.
    /// * `new_version` — Must be `> current_version`.
    /// * `memo`        — Optional migration description (max 128 bytes).
    ///
    /// # Errors
    /// * `NotAuthorized`  — caller is not the admin.
    /// * `InvalidVersion` — `new_version` ≤ current version.
    ///
    /// # Events
    /// Emits `(ds_migr, caller, new_version)` on success.
    #[allow(deprecated)]
    pub fn data_migrate_bump_version(
        env: Env,
        caller: Address,
        new_version: u32,
        memo: Option<String>,
    ) {
        caller.require_auth();
        Self::assert_initialized(&env);
        Self::assert_admin(&env, &caller);

        let current: u32 = env
            .storage()
            .persistent()
            .get(&StoreKey::SchemaVersion)
            .unwrap_or(0);

        if new_version <= current {
            panic_with_error!(&env, DataStoreError::InvalidVersion);
        }

        // Optional memo size guard (128 bytes max)
        if let Some(ref m) = memo {
            if m.len() > 128 {
                panic_with_error!(&env, DataStoreError::ValueTooLarge);
            }
        }

        env.storage()
            .persistent()
            .set(&StoreKey::SchemaVersion, &new_version);

        env.events()
            .publish((symbol_short!("ds_migr"), caller, new_version), memo);
    }

    // ───────────────────────────────────────────────────
    // Read-only helpers / queries
    // ───────────────────────────────────────────────────

    /// Return the current schema version.
    pub fn schema_version(env: Env) -> u32 {
        Self::assert_initialized(&env);
        env.storage()
            .persistent()
            .get(&StoreKey::SchemaVersion)
            .unwrap_or(0)
    }

    /// Return the current number of live entries.
    pub fn entry_count(env: Env) -> u32 {
        Self::assert_initialized(&env);
        env.storage()
            .persistent()
            .get(&StoreKey::EntryCount)
            .unwrap_or(0)
    }

    /// Return `true` if `key` exists in the store.
    pub fn key_exists(env: Env, key: String) -> bool {
        Self::assert_initialized(&env);
        env.storage().persistent().has(&StoreKey::Entry(key))
    }

    /// Return the admin address.
    #[allow(dead_code)]
    pub fn get_admin(env: Env) -> Address {
        Self::assert_initialized(&env);
        env.storage()
            .persistent()
            .get(&StoreKey::Admin)
            .unwrap_or_else(|| panic_with_error!(&env, DataStoreError::NotInitialized))
    }

    /// Return `true` if `address` is the admin or a granted writer.
    #[allow(dead_code)]
    pub fn is_writer(env: Env, address: Address) -> bool {
        if !env.storage().persistent().has(&StoreKey::Admin) {
            return false;
        }
        if Self::get_admin(env.clone()) == address {
            return true;
        }
        let writers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StoreKey::Writers)
            .unwrap_or_else(|| Vec::new(&env));
        writers.contains(&address)
    }

    // ═══════════════════════════════════════════════════
    // Private guard helpers
    // ═══════════════════════════════════════════════════

    /// Panic with `NotInitialized` if the contract has not been `init`-ed.
    fn assert_initialized(env: &Env) {
        if !env.storage().persistent().has(&StoreKey::Admin) {
            panic_with_error!(env, DataStoreError::NotInitialized);
        }
    }

    /// Panic with `NotAuthorized` if `caller` is not the admin.
    fn assert_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&StoreKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, DataStoreError::NotInitialized));

        if *caller != admin {
            panic_with_error!(env, DataStoreError::NotAuthorized);
        }
    }

    /// Panic with `NotAuthorized` if `caller` is neither admin nor writer.
    fn assert_can_write(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&StoreKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, DataStoreError::NotInitialized));

        if *caller == admin {
            return; // admin always can write
        }

        let writers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&StoreKey::Writers)
            .unwrap_or_else(|| Vec::new(env));

        if !writers.contains(caller) {
            panic_with_error!(env, DataStoreError::NotAuthorized);
        }
    }
}
