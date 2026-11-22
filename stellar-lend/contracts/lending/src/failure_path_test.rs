//! Focused failure-path coverage for lending entrypoints.
//!
//! Boundary and guard matrix coverage lives in
//! `entrypoint_failure_boundary_test` and `initialization_guard_test`.

#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env, Val, Vec};

fn fresh_contract() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    (env, client, id)
}

#[test]
fn initialize_requires_caller_authorization() {
    let env = Env::default();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);

    let result = client.try_initialize(&admin);
    assert!(result.is_err(), "initialize must require admin authorization");
    assert_eq!(client.get_admin(), None);
}

#[test]
fn config_mutators_reject_before_initialize() {
    let (env, client, _id) = fresh_contract();
    let key = soroban_sdk::Symbol::new(&env, "risk");
    let value = Bytes::from_slice(&env, &[1, 2, 3]);
    let name = soroban_sdk::Symbol::new(&env, "backup");

    let set = client.try_config_set(&key, &value);
    assert!(
        matches!(set, Err(Ok(LendingError::NotInitialized))),
        "config_set: expected NotInitialized, got {:?}",
        set
    );
    let backup = client.try_config_backup(&name);
    assert!(
        matches!(backup, Err(Ok(LendingError::NotInitialized))),
        "config_backup: expected NotInitialized, got {:?}",
        backup
    );
    let restore = client.try_config_restore(&name);
    assert!(
        matches!(restore, Err(Ok(LendingError::NotInitialized))),
        "config_restore: expected NotInitialized, got {:?}",
        restore
    );
}

#[test]
fn receive_rejects_before_initialize_without_mutation() {
    let (env, client, id) = fresh_contract();
    let token = Address::generate(&env);
    let from = Address::generate(&env);
    let payload = Vec::<Val>::new(&env);

    let result = client.try_receive(&token, &from, &100, &payload);
    assert!(
        matches!(result, Err(Ok(LendingError::NotInitialized))),
        "expected NotInitialized, got {:?}",
        result
    );

    assert!(!env.as_contract(&id, || {
        env.storage().instance().has(&DataKey::Admin)
    }));
}

#[test]
fn uninitialized_upgrade_mutators_return_not_initialized() {
    let (env, client, _id) = fresh_contract();
    let caller = Address::generate(&env);
    let wasm = BytesN::<32>::from_array(&env, &[7; 32]);
    let approver = Address::generate(&env);

    let init = client.try_upgrade_init(&caller, &wasm, &1);
    assert!(
        matches!(init, Err(Ok(LendingError::NotInitialized))),
        "upgrade_init: expected NotInitialized, got {:?}",
        init
    );
    let propose = client.try_upgrade_propose(&caller, &wasm, &1);
    assert!(
        matches!(propose, Err(Ok(LendingError::NotInitialized))),
        "upgrade_propose: expected NotInitialized, got {:?}",
        propose
    );
    let approve = client.try_upgrade_approve(&caller, &1);
    assert!(
        matches!(approve, Err(Ok(LendingError::NotInitialized))),
        "upgrade_approve: expected NotInitialized, got {:?}",
        approve
    );
    let execute = client.try_upgrade_execute(&caller, &1);
    assert!(
        matches!(execute, Err(Ok(LendingError::NotInitialized))),
        "upgrade_execute: expected NotInitialized, got {:?}",
        execute
    );
    let cancel = client.try_upgrade_cancel(&caller, &1);
    assert!(
        matches!(cancel, Err(Ok(LendingError::NotInitialized))),
        "upgrade_cancel: expected NotInitialized, got {:?}",
        cancel
    );
    let threshold = client.try_upgrade_set_required_approvals(&caller, &1);
    assert!(
        matches!(threshold, Err(Ok(LendingError::NotInitialized))),
        "upgrade_set_required_approvals: expected NotInitialized, got {:?}",
        threshold
    );
    let add = client.try_upgrade_add_approver(&caller, &approver);
    assert!(
        matches!(add, Err(Ok(LendingError::NotInitialized))),
        "upgrade_add_approver: expected NotInitialized, got {:?}",
        add
    );
    let remove = client.try_upgrade_remove_approver(&caller, &approver);
    assert!(
        matches!(remove, Err(Ok(LendingError::NotInitialized))),
        "upgrade_remove_approver: expected NotInitialized, got {:?}",
        remove
    );
}
