//! Protocol configuration module — admin-gated key-value storage.
//!
//! Provides `config_set`, `config_get`, `config_backup`, and `config_restore`
//! backed by `persistent` storage under `ConfigDataKey::ConfigKey(Symbol)`.
//! Write operations require the caller to be the protocol admin.

use soroban_sdk::{contracttype, Env, Map, Symbol, Val, Vec};

use crate::admin::{require_admin, AdminError};

// ---------------------------------------------------------------------------
// Storage key
// ---------------------------------------------------------------------------

#[contracttype]
pub enum ConfigDataKey {
    /// Stores a single configuration value keyed by a `Symbol`.
    ConfigKey(Symbol),
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Set a configuration key to `val` (admin only).
///
/// # Errors
/// Returns [`AdminError::Unauthorized`] or [`AdminError::NotInitialized`] when
/// `caller` is not the current protocol admin.
pub fn config_set(
    env: &Env,
    caller: &soroban_sdk::Address,
    key: &Symbol,
    val: Val,
) -> Result<(), AdminError> {
    require_admin(env, caller)?;
    env.storage()
        .persistent()
        .set(&ConfigDataKey::ConfigKey(key.clone()), &val);
    Ok(())
}

/// Retrieve the value stored under `key`, or `None` if not set.
pub fn config_get(env: &Env, key: &Symbol) -> Option<Val> {
    env.storage()
        .persistent()
        .get(&ConfigDataKey::ConfigKey(key.clone()))
}

/// Return a map of key → value for every key in `keys` (admin only).
///
/// Keys that have no stored value are omitted from the result.
///
/// # Errors
/// Returns [`AdminError::Unauthorized`] or [`AdminError::NotInitialized`] when
/// `caller` is not the current protocol admin.
pub fn config_backup(
    env: &Env,
    caller: &soroban_sdk::Address,
    keys: &Vec<Symbol>,
) -> Result<Map<Symbol, Val>, AdminError> {
    require_admin(env, caller)?;
    let mut out: Map<Symbol, Val> = Map::new(env);
    for key in keys.iter() {
        if let Some(val) = config_get(env, &key) {
            out.set(key, val);
        }
    }
    Ok(out)
}

/// Restore a set of key-value pairs from a backup map (admin only).
///
/// Each entry in `entries` is written to persistent storage, overwriting any
/// existing value for that key.
///
/// # Errors
/// Returns [`AdminError::Unauthorized`] or [`AdminError::NotInitialized`] when
/// `caller` is not the current protocol admin.
pub fn config_restore(
    env: &Env,
    caller: &soroban_sdk::Address,
    entries: &Map<Symbol, Val>,
) -> Result<(), AdminError> {
    require_admin(env, caller)?;
    for (key, val) in entries.iter() {
        env.storage()
            .persistent()
            .set(&ConfigDataKey::ConfigKey(key), &val);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, Address, IntoVal};

    #[contract]
    struct TestHost;

    #[contractimpl]
    impl TestHost {
        pub fn init(env: Env, admin: Address) {
            crate::admin::set_admin(&env, admin, None).unwrap();
        }

        pub fn config_set(
            env: Env,
            caller: Address,
            key: Symbol,
            val: Val,
        ) -> Result<(), AdminError> {
            super::config_set(&env, &caller, &key, val)
        }

        pub fn config_get(env: Env, key: Symbol) -> Option<Val> {
            super::config_get(&env, &key)
        }

        pub fn config_backup(
            env: Env,
            caller: Address,
            keys: Vec<Symbol>,
        ) -> Result<Map<Symbol, Val>, AdminError> {
            super::config_backup(&env, &caller, &keys)
        }

        pub fn config_restore(
            env: Env,
            caller: Address,
            entries: Map<Symbol, Val>,
        ) -> Result<(), AdminError> {
            super::config_restore(&env, &caller, &entries)
        }
    }

    fn setup() -> (Env, TestHostClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(TestHost, ());
        let client = TestHostClient::new(&env, &id);
        let admin = Address::generate(&env);
        client.init(&admin);
        (env, client, admin)
    }

    #[test]
    fn test_set_and_get() {
        let (env, client, admin) = setup();
        let key = Symbol::new(&env, "fee_rate");
        let val: Val = 100_u32.into_val(&env);
        client.config_set(&admin, &key, &val);
        let got: Val = client.config_get(&key).unwrap();
        let got_u32: u32 = got.into_val(&env);
        assert_eq!(got_u32, 100_u32);
    }

    #[test]
    fn test_get_missing_returns_none() {
        let (env, client, _admin) = setup();
        let key = Symbol::new(&env, "missing");
        assert!(client.config_get(&key).is_none());
    }

    #[test]
    fn test_set_unauthorized() {
        let (env, client, _admin) = setup();
        let stranger = Address::generate(&env);
        let key = Symbol::new(&env, "x");
        let val: Val = 1_u32.into_val(&env);
        let result = client.try_config_set(&stranger, &key, &val);
        assert!(result.is_err());
    }

    #[test]
    fn test_backup_and_restore() {
        let (env, client, admin) = setup();
        let k1 = Symbol::new(&env, "a");
        let k2 = Symbol::new(&env, "b");
        let v1: Val = 10_u32.into_val(&env);
        let v2: Val = 20_u32.into_val(&env);
        client.config_set(&admin, &k1, &v1);
        client.config_set(&admin, &k2, &v2);

        let keys = vec![&env, k1.clone(), k2.clone()];
        let backup = client.config_backup(&admin, &keys).unwrap();
        assert_eq!(backup.len(), 2);

        // Overwrite then restore
        let new_val: Val = 99_u32.into_val(&env);
        client.config_set(&admin, &k1, &new_val);

        client.config_restore(&admin, &backup).unwrap();
        let restored: Val = client.config_get(&k1).unwrap();
        let restored_u32: u32 = restored.into_val(&env);
        assert_eq!(restored_u32, 10_u32);
    }

    #[test]
    fn test_backup_skips_missing_keys() {
        let (env, client, admin) = setup();
        let present = Symbol::new(&env, "present");
        let absent = Symbol::new(&env, "absent");
        let val: Val = 5_u32.into_val(&env);
        client.config_set(&admin, &present, &val);

        let keys = vec![&env, present.clone(), absent.clone()];
        let backup = client.config_backup(&admin, &keys).unwrap();
        assert_eq!(backup.len(), 1);
        assert!(backup.get(present).is_some());
    }
}
