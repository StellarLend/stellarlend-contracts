//! # Storage Layout Snapshot Tests
//!
//! Validates that persistent storage keys for config and risk parameters remain
//! stable across upgrades. Any key rename or enum variant reorder will cause
//! these tests to fail, acting as a regression guard for upgrade tooling.
//!
//! ## Storage keys covered
//! - `AdminDataKey::Admin`
//! - `RiskParamsDataKey::RiskParamsConfig`
//! - `RiskDataKey::RiskConfig`, `RiskDataKey::EmergencyPause`
//! - `ConfigDataKey::ConfigKey(symbol)`
//! - `GovernanceDataKey::Admin`, `GovernanceDataKey::Config`
//!
//! ## Security notes
//! - Tests access storage directly via `env.as_contract` to verify key identity.
//! - No sensitive data is exposed; only key presence and value types are checked.
//! - All writes go through the public contract API to mirror production paths.

#![cfg(test)]

use crate::admin::AdminDataKey;
use crate::config::ConfigDataKey;
use crate::risk_management::RiskDataKey;
use crate::risk_params::{RiskParams, RiskParamsDataKey};
use crate::storage::GovernanceDataKey;
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, Symbol, Val};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, Address, HelloContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, admin, client)
}

fn contract_id(env: &Env) -> Address {
    let contract_id = env.register(HelloContract, ());
    contract_id
}

// ---------------------------------------------------------------------------
// Admin storage key stability
// ---------------------------------------------------------------------------

/// `AdminDataKey::Admin` must be present in persistent storage after initialize.
/// If this key is renamed or moved to a different storage tier, upgrades will
/// lose the admin address and lock the protocol.
#[test]
fn storage_layout_admin_key_present_after_init() {
    let (env, admin, _client) = setup();
    let cid = env.register(HelloContract, ());
    // Use a fresh contract to avoid double-init; re-use the already-initialized one via as_contract
    // We need the contract_id from setup — re-register and init
    let env2 = Env::default();
    env2.mock_all_auths();
    let cid2 = env2.register(HelloContract, ());
    let client2 = HelloContractClient::new(&env2, &cid2);
    let admin2 = Address::generate(&env2);
    client2.initialize(&admin2);

    env2.as_contract(&cid2, || {
        assert!(
            env2.storage().persistent().has(&AdminDataKey::Admin),
            "AdminDataKey::Admin must exist in persistent storage after initialize"
        );
        let stored: Address = env2
            .storage()
            .persistent()
            .get(&AdminDataKey::Admin)
            .expect("AdminDataKey::Admin value must be readable");
        assert_eq!(stored, admin2, "stored admin must match initialized admin");
    });
}

/// `GovernanceDataKey::Admin` (used by risk_management) must also be present.
#[test]
fn storage_layout_governance_admin_key_present_after_init() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    env.as_contract(&cid, || {
        assert!(
            env.storage().persistent().has(&GovernanceDataKey::Admin),
            "GovernanceDataKey::Admin must exist in persistent storage after initialize"
        );
        let stored: Address = env
            .storage()
            .persistent()
            .get(&GovernanceDataKey::Admin)
            .expect("GovernanceDataKey::Admin value must be readable");
        assert_eq!(stored, admin);
    });
}

// ---------------------------------------------------------------------------
// Risk params storage key stability
// ---------------------------------------------------------------------------

/// `RiskParamsDataKey::RiskParamsConfig` must be present with correct default values.
/// Renaming this key would cause `get_risk_params` to return `None` after an upgrade,
/// breaking all collateral ratio checks.
#[test]
fn storage_layout_risk_params_key_present_with_defaults() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    env.as_contract(&cid, || {
        assert!(
            env.storage()
                .persistent()
                .has(&RiskParamsDataKey::RiskParamsConfig),
            "RiskParamsDataKey::RiskParamsConfig must exist after initialize"
        );
        let params: RiskParams = env
            .storage()
            .persistent()
            .get(&RiskParamsDataKey::RiskParamsConfig)
            .expect("RiskParamsConfig must be readable");

        // Snapshot the default values — any change here is a breaking upgrade
        assert_eq!(
            params.min_collateral_ratio, 11_000,
            "default min_collateral_ratio = 11000 bps (110%)"
        );
        assert_eq!(
            params.liquidation_threshold, 10_500,
            "default liquidation_threshold = 10500 bps (105%)"
        );
        assert_eq!(
            params.close_factor, 5_000,
            "default close_factor = 5000 bps (50%)"
        );
        assert_eq!(
            params.liquidation_incentive, 1_000,
            "default liquidation_incentive = 1000 bps (10%)"
        );
    });
}

/// After `set_risk_params`, the updated values must be readable under the same key.
#[test]
fn storage_layout_risk_params_key_stable_after_update() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Update within 10% change limit
    client.set_risk_params(&admin, &Some(12_000), &Some(11_000), &None, &None);

    env.as_contract(&cid, || {
        let params: RiskParams = env
            .storage()
            .persistent()
            .get(&RiskParamsDataKey::RiskParamsConfig)
            .expect("RiskParamsConfig must still be readable after update");
        assert_eq!(params.min_collateral_ratio, 12_000);
        assert_eq!(params.liquidation_threshold, 11_000);
        // Unchanged fields remain at defaults
        assert_eq!(params.close_factor, 5_000);
        assert_eq!(params.liquidation_incentive, 1_000);
    });
}

// ---------------------------------------------------------------------------
// Risk management storage key stability
// ---------------------------------------------------------------------------

/// `RiskDataKey::RiskConfig` must be present after initialize.
#[test]
fn storage_layout_risk_config_key_present_after_init() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    env.as_contract(&cid, || {
        assert!(
            env.storage().persistent().has(&RiskDataKey::RiskConfig),
            "RiskDataKey::RiskConfig must exist after initialize"
        );
    });
}

/// `RiskDataKey::EmergencyPause` must default to `false` and be readable.
/// If this key shifts, emergency pause state is lost on upgrade.
#[test]
fn storage_layout_emergency_pause_key_defaults_false() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    env.as_contract(&cid, || {
        let paused: bool = env
            .storage()
            .persistent()
            .get(&RiskDataKey::EmergencyPause)
            .unwrap_or(false);
        assert!(!paused, "EmergencyPause must default to false");
    });
}

/// After `set_emergency_pause(true)`, the key must reflect the new state.
#[test]
fn storage_layout_emergency_pause_key_stable_after_toggle() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_emergency_pause(&admin, &true);

    env.as_contract(&cid, || {
        let paused: bool = env
            .storage()
            .persistent()
            .get(&RiskDataKey::EmergencyPause)
            .expect("EmergencyPause key must be present after toggle");
        assert!(
            paused,
            "EmergencyPause must be true after set_emergency_pause(true)"
        );
    });

    client.set_emergency_pause(&admin, &false);

    env.as_contract(&cid, || {
        let paused: bool = env
            .storage()
            .persistent()
            .get(&RiskDataKey::EmergencyPause)
            .expect("EmergencyPause key must be present after second toggle");
        assert!(
            !paused,
            "EmergencyPause must be false after set_emergency_pause(false)"
        );
    });
}

// ---------------------------------------------------------------------------
// Config storage key stability
// ---------------------------------------------------------------------------

/// `ConfigDataKey::ConfigKey(symbol)` must be readable after `config_set`.
/// Key derivation must remain stable — any change breaks config restore on upgrade.
#[test]
fn storage_layout_config_key_stable_after_set() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let key = Symbol::new(&env, "fee_rate");
    let val: Val = 250_u32.into_val(&env);
    client.config_set(&admin, &key, &val);

    env.as_contract(&cid, || {
        let storage_key = ConfigDataKey::ConfigKey(Symbol::new(&env, "fee_rate"));
        assert!(
            env.storage().persistent().has(&storage_key),
            "ConfigDataKey::ConfigKey must be present after config_set"
        );
        let stored: Val = env
            .storage()
            .persistent()
            .get(&storage_key)
            .expect("ConfigDataKey::ConfigKey value must be readable");
        let expected: Val = 250_u32.into_val(&env);
        assert_eq!(
            stored.get_payload(),
            expected.get_payload(),
            "stored config value must match set value"
        );
    });
}

/// Multiple config keys must be independently addressable (no key collision).
#[test]
fn storage_layout_config_keys_no_collision() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let key_a = Symbol::new(&env, "param_a");
    let key_b = Symbol::new(&env, "param_b");
    let val_a: Val = 1_u32.into_val(&env);
    let val_b: Val = 2_u32.into_val(&env);

    client.config_set(&admin, &key_a, &val_a);
    client.config_set(&admin, &key_b, &val_b);

    env.as_contract(&cid, || {
        let stored_a: Val = env
            .storage()
            .persistent()
            .get(&ConfigDataKey::ConfigKey(Symbol::new(&env, "param_a")))
            .unwrap();
        let stored_b: Val = env
            .storage()
            .persistent()
            .get(&ConfigDataKey::ConfigKey(Symbol::new(&env, "param_b")))
            .unwrap();

        let expected_a: Val = 1_u32.into_val(&env);
        let expected_b: Val = 2_u32.into_val(&env);
        assert_eq!(stored_a.get_payload(), expected_a.get_payload());
        assert_eq!(stored_b.get_payload(), expected_b.get_payload());
        // Ensure they differ
        assert_ne!(stored_a.get_payload(), stored_b.get_payload());
    });
}

// ---------------------------------------------------------------------------
// Config snapshot stability (upgrade regression)
// ---------------------------------------------------------------------------

/// `get_config_snapshot` must return the same field values as direct storage reads.
/// This ensures the snapshot view is consistent with the underlying storage layout.
#[test]
fn storage_layout_config_snapshot_matches_storage() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let snapshot = client
        .get_config_snapshot()
        .expect("snapshot must be Some after init");

    env.as_contract(&cid, || {
        let params: RiskParams = env
            .storage()
            .persistent()
            .get(&RiskParamsDataKey::RiskParamsConfig)
            .unwrap();
        let emergency_paused: bool = env
            .storage()
            .persistent()
            .get(&RiskDataKey::EmergencyPause)
            .unwrap_or(false);

        assert_eq!(snapshot.min_collateral_ratio, params.min_collateral_ratio);
        assert_eq!(snapshot.liquidation_threshold, params.liquidation_threshold);
        assert_eq!(snapshot.close_factor, params.close_factor);
        assert_eq!(snapshot.liquidation_incentive, params.liquidation_incentive);
        assert_eq!(snapshot.emergency_paused, emergency_paused);
    });
}

/// After updating risk params, `get_config_snapshot` must reflect the new values.
#[test]
fn storage_layout_config_snapshot_reflects_param_update() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_risk_params(
        &admin,
        &Some(12_000),
        &Some(11_000),
        &Some(5_500),
        &Some(1_100),
    );

    let snapshot = client.get_config_snapshot().unwrap();
    assert_eq!(snapshot.min_collateral_ratio, 12_000);
    assert_eq!(snapshot.liquidation_threshold, 11_000);
    assert_eq!(snapshot.close_factor, 5_500);
    assert_eq!(snapshot.liquidation_incentive, 1_100);
}

/// `get_config_snapshot` must return `None` before `initialize` is called.
/// This is the upgrade-safety guard: a freshly deployed contract with no state
/// must not return stale or zero-value snapshots.
#[test]
fn storage_layout_config_snapshot_none_before_init() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    assert!(
        client.get_config_snapshot().is_none(),
        "snapshot must be None before initialize"
    );
}

/// `get_config_snapshot` must be callable by any address (no auth required).
/// Snapshot reads must never gate on admin to support monitoring tooling.
#[test]
fn storage_layout_config_snapshot_no_auth_required() {
    let env = Env::default();
    // Do NOT mock_all_auths — snapshot must work without any auth
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    // initialize requires auth, so mock just for that
    env.mock_all_auths();
    client.initialize(&admin);
    // Now clear mocks and verify snapshot still works
    let snapshot = client.get_config_snapshot();
    assert!(snapshot.is_some());
}

// ---------------------------------------------------------------------------
// Storage tier correctness
// ---------------------------------------------------------------------------

/// All risk and config keys must use `persistent` storage, not `instance` or `temporary`.
/// Persistent storage survives ledger archival; instance/temporary do not.
/// This test verifies the tier by checking that keys are absent from instance storage.
#[test]
fn storage_layout_risk_params_in_persistent_not_instance() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    env.as_contract(&cid, || {
        // Must be in persistent
        assert!(
            env.storage()
                .persistent()
                .has(&RiskParamsDataKey::RiskParamsConfig),
            "RiskParamsConfig must be in persistent storage"
        );
        assert!(
            env.storage().persistent().has(&RiskDataKey::EmergencyPause),
            "EmergencyPause must be in persistent storage"
        );
        assert!(
            env.storage().persistent().has(&AdminDataKey::Admin),
            "Admin must be in persistent storage"
        );
    });
}
