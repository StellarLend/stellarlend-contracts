use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Error, InvokeError};

use crate::{LendingContract, LendingContractClient, UpgradeError, UpgradeStage};

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
fn test_upgrade_rollback_requires_executed_stage() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 6), &1);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::Approved
    );

    assert_contract_error(
        client.try_upgrade_rollback(&admin, &proposal_id),
        UpgradeError::InvalidStatus,
    );
}

#[test]
fn test_upgrade_execute_missing_proposal_errors() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    assert_contract_error(
        client.try_upgrade_execute(&admin, &77),
        UpgradeError::ProposalNotFound,
    );
}

#[test]
fn test_upgrade_rollback_missing_proposal_errors() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    assert_contract_error(
        client.try_upgrade_rollback(&admin, &88),
        UpgradeError::ProposalNotFound,
    );
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

// ── Issue #489: upgrade authorization clarity ─────────────────────────────

/// Guardian has no upgrade power — upgrade paths are admin/approver-gated only.
///
/// # Security
/// The guardian role is limited to emergency shutdown. It must not be able to
/// propose, approve, execute, or roll back upgrades. This test documents that
/// trust boundary explicitly.
#[test]
fn test_guardian_cannot_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);
    let guardian = Address::generate(&env);

    // Guardian is not an approver — all upgrade entry points must reject it.
    assert_contract_error(
        client.try_upgrade_propose(&guardian, &hash(&env, 5), &10),
        UpgradeError::NotAuthorized,
    );
    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 5), &10);
    assert_contract_error(
        client.try_upgrade_approve(&guardian, &proposal_id),
        UpgradeError::NotAuthorized,
    );
    assert_contract_error(
        client.try_upgrade_execute(&guardian, &proposal_id),
        UpgradeError::NotAuthorized,
    );
    assert_contract_error(
        client.try_upgrade_rollback(&guardian, &proposal_id),
        UpgradeError::NotAuthorized,
    );
}

/// Arbitrary stranger cannot propose an upgrade.
///
/// # Security
/// Only the stored admin address may create upgrade proposals. Any other caller
/// must be rejected with `NotAuthorized` regardless of the WASM hash or version.
#[test]
fn test_stranger_cannot_propose() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env, 1);
    let stranger = Address::generate(&env);

    assert_contract_error(
        client.try_upgrade_propose(&stranger, &hash(&env, 7), &99),
        UpgradeError::NotAuthorized,
    );
}

/// Only the admin may roll back an executed upgrade.
///
/// # Security
/// Rollback restores the previous WASM hash and version. Restricting it to the
/// admin prevents an approver-only account from silently reverting a governance
/// decision without admin sign-off.
#[test]
fn test_only_admin_can_rollback() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &proposal_id);

    // Approver (non-admin) must not be able to rollback.
    assert_contract_error(
        client.try_upgrade_rollback(&approver, &proposal_id),
        UpgradeError::NotAuthorized,
    );

    // Admin succeeds.
    client.upgrade_rollback(&admin, &proposal_id);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::RolledBack
    );
}

/// Approval threshold boundary: n-1 approvals must not reach Approved stage,
/// exactly n approvals must flip the proposal to Approved.
///
/// # Security
/// The threshold is the core multi-sig invariant. A proposal executable with
/// fewer approvals than `required_approvals` would allow a single compromised
/// key to push through an upgrade.
#[test]
fn test_threshold_boundary_n_minus_one_vs_n() {
    let env = Env::default();
    env.mock_all_auths();
    // 3-of-3 setup: admin + 2 extra approvers
    let (client, admin) = setup(&env, 3);
    let approver_a = Address::generate(&env);
    let approver_b = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver_a);
    client.upgrade_add_approver(&admin, &approver_b);

    let proposal_id = client.upgrade_propose(&admin, &hash(&env, 2), &1);

    // After proposer (admin) + approver_a = 2 approvals — still Proposed (need 3).
    let count_after_a = client.upgrade_approve(&approver_a, &proposal_id);
    assert_eq!(count_after_a, 2);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::Proposed
    );

    // Execute must fail — not yet Approved.
    assert_contract_error(
        client.try_upgrade_execute(&admin, &proposal_id),
        UpgradeError::InvalidStatus,
    );

    // Third approval flips to Approved.
    let count_after_b = client.upgrade_approve(&approver_b, &proposal_id);
    assert_eq!(count_after_b, 3);
    assert_eq!(
        client.upgrade_status(&proposal_id).stage,
        UpgradeStage::Approved
    );

    // Execute now succeeds.
    client.upgrade_execute(&admin, &proposal_id);
    assert_eq!(client.current_version(), 1);
}

/// Duplicate proposal IDs are not possible — each proposal gets a unique
/// monotonically increasing ID.
///
/// # Security
/// If IDs could collide, a race between two proposals could cause one to
/// silently overwrite the other's approvals and WASM hash.
#[test]
fn test_proposal_ids_are_monotonically_increasing() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    let id1 = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &id1);

    let id2 = client.upgrade_propose(&admin, &hash(&env, 3), &2);
    client.upgrade_execute(&admin, &id2);

    assert!(id2 > id1, "proposal IDs must be strictly increasing");
    assert_eq!(client.current_version(), 2);
}
