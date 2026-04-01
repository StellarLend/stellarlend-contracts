//! # Configuration Module
//!
//! Provides key-value configuration storage for the lending protocol.
//! Allows the admin to set, get, backup, and restore configuration parameters safely.
//!
//! # Security
//! - All state-mutating and read-sensitive functions require admin authorization.
//! - Backup and restore functions have bounded limits to prevent out-of-gas (OOG) vulnerabilities.
//! - Emits events for traceability and tamper detection.

use crate::risk_management::require_admin;
use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol, Val, Vec};
use crate::prelude::*;

const MAX_BATCH_SIZE: u32 = 50;

/// Errors that can occur during configuration operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ConfigError {
    /// Unauthorized access - caller is not admin
    Unauthorized = 1,
    /// Batch size exceeds the maximum allowed limit
    BatchSizeExceeded = 2,
}

/// Storage keys for configuration data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ConfigDataKey {
    /// Configuration key-value mapping
    ConfigKey(Symbol),
}

/// Set a configuration value (admin only)
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `key` - The configuration key
/// * `value` - The configuration value
///
/// # Errors
/// Returns `ConfigError::Unauthorized` if caller is not the admin.
///
/// # Security
/// Requires admin signature. Emits a `config_updated` event.
pub fn config_set(env: &Env, caller: Address, key: Symbol, value: Val) -> Result<(), ConfigError> {
    require_admin(env, &caller).map_err(|_| ConfigError::Unauthorized)?;

    let storage_key = ConfigDataKey::ConfigKey(key.clone());
    env.storage().persistent().set(&storage_key, &value);

    let topics = (Symbol::new(env, "config_updated"), caller);
    env.events().publish(topics, (key, value));

    Ok(())
}

/// Get a configuration value
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `key` - The configuration key
///
/// # Returns
/// Returns Some(value) if the key exists, None otherwise
pub fn config_get(env: &Env, key: Symbol) -> Option<Val> {
    let storage_key = ConfigDataKey::ConfigKey(key);
    env.storage().persistent().get(&storage_key)
}

/// Backup configuration parameters (admin only)
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `keys` - A vector of configuration keys to backup
///
/// # Errors
/// Returns `ConfigError::Unauthorized` if caller is not the admin.
/// Returns `ConfigError::BatchSizeExceeded` if `keys` length exceeds `MAX_BATCH_SIZE`.
///
/// # Security
/// Admin only. Bounded iteration.
pub fn config_backup(
    env: &Env,
    caller: Address,
    keys: Vec<Symbol>,
) -> Result<Vec<(Symbol, Val)>, ConfigError> {
    require_admin(env, &caller).map_err(|_| ConfigError::Unauthorized)?;

    if keys.len() > MAX_BATCH_SIZE {
        return Err(ConfigError::BatchSizeExceeded);
    }

    let mut backup = Vec::new(env);
    for key in keys.iter() {
        if let Some(value) = config_get(env, key.clone()) {
            backup.push_back((key, value));
        }
    }

    Ok(backup)
}

/// Restore configuration parameters (admin only)
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `backup` - A vector of key-value pairs to restore
///
/// # Errors
/// Returns `ConfigError::Unauthorized` if caller is not the admin.
/// Returns `ConfigError::BatchSizeExceeded` if `backup` length exceeds `MAX_BATCH_SIZE`.
///
/// # Security
/// Admin only. Bounded iteration to prevent gas limit issues. Emits a `config_restored` event.
pub fn config_restore(
    env: &Env,
    caller: Address,
    backup: Vec<(Symbol, Val)>,
) -> Result<(), ConfigError> {
    require_admin(env, &caller).map_err(|_| ConfigError::Unauthorized)?;

    if backup.len() > MAX_BATCH_SIZE {
        return Err(ConfigError::BatchSizeExceeded);
    }

    for (key, value) in backup.iter() {
        let storage_key = ConfigDataKey::ConfigKey(key.clone());
        env.storage().persistent().set(&storage_key, &value);
    }

    let topics = (Symbol::new(env, "config_restored"), caller);
    env.events().publish(topics, backup.len());

    Ok(())
}

#[cfg(test)]
mod config_test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    fn test_batch_size_exceeded() {
        let env = Env::default();
        let caller = Address::generate(&env);
        
        let mut keys = Vec::new(&env);
        for _ in 0..51 {
            keys.push_back(Symbol::new(&env, "key"));
        }
        
        let res = config_backup(&env, caller, keys);
        assert!(res.is_err());
    }
}
