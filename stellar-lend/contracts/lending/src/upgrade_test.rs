use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Error, InvokeError};

use crate::{LendingContract, LendingContractClient};
use stellarlend_common::upgrade::{UpgradeError, UpgradeStage};

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

#[allow(deprecated)]
fn setup(env: &Env, required_approvals: u32) -> (LendingContractClient<'_>, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.upgrade_init(&admin, &hash(env, 1), &required_approvals);
    (client, admin)
}

fn assert_contract_error<T, E>(
    result: Result<Result<T, E>, Result<Error, InvokeError>>,
    expected: UpgradeError,
) {
    match result {
        Err(Ok(err)) => assert_eq!(err, Error::from_contract_error(expected as u32)),
        Ok(Err(_)) => {}
        _ => panic!("expected contract error"),
    }
}

fn assert_failed<T>(result: Result<T, Result<Error, InvokeError>>) {
    assert!(result.is_err(), "expected operation to fail");
}

/// Verifies initialization and baseline status fields.
#[test]
fn test_init_sets_defaults() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env, 2);

    assert_eq!(client.current_version(), 0);
    assert_eq!(client.current_wasm_hash(), hash(&env, 1));
}

#[test]
fn test_init_rejects_zero_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    assert_contract_error(
        client.try_upgrade_init(&admin, &hash(&env, 1), &0),
        UpgradeError::InvalidThreshold,
    );
}

#[test]
fn test_add_approver_admin_only() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    let stranger = Address::generate(&env);

    let denied = client.try_upgrade_add_approver(&stranger, &approver);
    assert_contract_error(denied, UpgradeError::NotAuthorized);

    client.upgrade_add_approver(&admin, &approver);
}

#[test]
fn test_upgrade_propose_sets_initial_status() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    let status = client.upgrade_status(&proposal_id);
    assert_eq!(proposal_id, 1);
    assert_eq!(status.id, 1);
    assert_eq!(status.stage, UpgradeStage::Proposed);
    assert_eq!(status.approval_count, 1);
    assert_eq!(status.target_version, 1);
}

#[test]
fn test_upgrade_approve_flow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    let count = client.upgrade_approve(&approver, &proposal_id);
    assert_eq!(count, 2);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::Approved
    );
}

#[test]
fn test_upgrade_execute_updates_current_version_and_hash() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    let next_hash = hash(&env, 9);
    let proposal_id = client.upgrade_propose(&admin, &next_hash, &3);

    // In tests, update_current_contract_wasm might not actually swap the code in a visible way
    // without more setup, but we can verify the state updates.
    client.upgrade_execute(&admin, &proposal_id);

    assert_eq!(client.current_version(), 3);
    assert_eq!(client.current_wasm_hash(), next_hash);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::Executed
    );
}

#[test]
fn test_upgrade_rollback_restores_previous() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);
    let initial_hash = client.current_wasm_hash();

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 8), &5);
    client.upgrade_execute(&admin, &proposal_id);
    assert_eq!(client.current_version(), 5);

    client.upgrade_rollback(&admin, &proposal_id);
    assert_eq!(client.current_version(), 0);
    assert_eq!(client.current_wasm_hash(), initial_hash);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::RolledBack
    );

    let repeated = client.try_upgrade_rollback(&admin, &proposal_id);
    assert_failed(repeated);
}

#[test]
fn test_upgrade_status_missing_proposal_errors() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env, 1);

    let result = client.try_upgrade_status(&42);
    assert_failed(result);
}

#[test]
fn test_upgrade_rejects_unauthorized_approve_and_execute() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    assert_contract_error(
        client.try_upgrade_approve(&stranger, &proposal_id),
        UpgradeError::NotAuthorized,
    );

    client.upgrade_approve(&approver, &proposal_id);
    assert_contract_error(
        client.try_upgrade_execute(&stranger, &proposal_id),
        UpgradeError::NotAuthorized,
    );
}

#[test]
fn test_upgrade_rotation_revokes_old_approver() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let old_approver = Address::generate(&env);
    let new_approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &old_approver);
    client.upgrade_add_approver(&admin, &new_approver);

    let first_upgrade = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_approve(&old_approver, &first_upgrade);
    client.upgrade_execute(&old_approver, &first_upgrade);
    assert_eq!(client.current_version(), 1);

    client.upgrade_remove_approver(&admin, &old_approver);

    let second_upgrade = client.upgrade_propose(&admin, &hash(&env, 3), &2);
    assert_contract_error(
        client.try_upgrade_approve(&old_approver, &second_upgrade),
        UpgradeError::NotAuthorized,
    );
    client.upgrade_approve(&new_approver, &second_upgrade);
    assert_contract_error(
        client.try_upgrade_execute(&old_approver, &second_upgrade),
        UpgradeError::NotAuthorized,
    );
    client.upgrade_execute(&new_approver, &second_upgrade);
    assert_eq!(client.current_version(), 2);
}

#[test]
fn test_upgrade_remove_approver_enforces_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    assert_contract_error(
        client.try_upgrade_remove_approver(&admin, &approver),
        UpgradeError::InvalidThreshold,
    );
}

#[test]
fn test_upgrade_invalid_attempts() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    assert_contract_error(
        client.try_upgrade_execute(&approver, &proposal_id),
        UpgradeError::InvalidStatus,
    );
    client.upgrade_approve(&approver, &proposal_id);
    assert_contract_error(
        client.try_upgrade_approve(&approver, &proposal_id),
        UpgradeError::AlreadyApproved,
    );
    assert_contract_error(
        client.try_upgrade_propose(&admin, &hash(&env, 3), &0),
        UpgradeError::InvalidVersion,
    );
}
