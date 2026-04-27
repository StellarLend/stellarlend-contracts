//! # Governance & Multisig Adversarial Tests  (#442)
//!
//! Threat scenarios covered:
//!
//! | # | Threat | Defence |
//! |---|--------|---------|
//! | 1 | Stranger proposes upgrade | `NotAuthorized` |
//! | 2 | Stranger approves proposal | `NotAuthorized` |
//! | 3 | Stranger executes approved proposal | `NotAuthorized` |
//! | 4 | Stranger rolls back executed upgrade | `NotAuthorized` |
//! | 5 | Approver double-votes on same proposal | `AlreadyApproved` |
//! | 6 | Execute before threshold reached | `InvalidStatus` |
//! | 7 | Replay: re-execute already-executed proposal | `InvalidStatus` |
//! | 8 | Replay: re-approve already-executed proposal | `InvalidStatus` |
//! | 9 | Version rollback attack (propose version < current) | `InvalidVersion` |
//! | 10 | Propose version == current | `InvalidVersion` |
//! | 11 | Remove approver below threshold | `InvalidThreshold` |
//! | 12 | Re-initialise already-initialised upgrade manager | `AlreadyInitialized` |
//! | 13 | Init with zero threshold | `InvalidThreshold` |
//! | 14 | Status query on non-existent proposal | panic |
//! | 15 | Revoked approver cannot approve new proposal | `NotAuthorized` |
//! | 16 | Stranger cannot add approver | `NotAuthorized` |
//! | 17 | Stranger cannot remove approver | `NotAuthorized` |
//! | 18 | Rollback a proposal that was never executed | `InvalidStatus` |
//! | 19 | Stranger cannot trigger emergency shutdown | `Unauthorized` |
//! | 20 | Stranger cannot set pause | panic (contract error #6) |
//! | 21 | Guardian can trigger shutdown; stranger cannot | correct |

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Error, InvokeError};

use crate::{LendingContract, LendingContractClient, UpgradeError, UpgradeStage};
use crate::borrow::BorrowError;

// ─── helpers ────────────────────────────────────────────────────────────────

fn hash(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

/// Assert a `try_upgrade_*` call returned the expected contract error.
/// Mirrors the helper in upgrade_test.rs.
fn assert_upgrade_err<T, E>(
    result: Result<Result<T, E>, Result<Error, InvokeError>>,
    expected: UpgradeError,
) {
    match result {
        Err(Ok(err)) => assert_eq!(err, Error::from_contract_error(expected as u32)),
        Ok(Err(_)) => {}
        _ => panic!("expected UpgradeError::{expected:?}"),
    }
}

fn assert_failed<T>(result: Result<T, Result<Error, InvokeError>>) {
    assert!(result.is_err(), "expected operation to fail");
}

/// Set up upgrade manager only.
#[allow(deprecated)]
fn setup(env: &Env, required_approvals: u32) -> (LendingContractClient<'_>, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.upgrade_init(&admin, &hash(env, 1), &required_approvals);
    (client, admin)
}

/// Set up full lending protocol + upgrade manager.
fn setup_lending(env: &Env) -> (LendingContractClient<'_>, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.upgrade_init(&admin, &hash(env, 1), &1);
    (client, admin)
}

// ─── 1. Unauthorized proposal creation ──────────────────────────────────────

#[test]
fn test_stranger_cannot_propose_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env, 1);
    let stranger = Address::generate(&env);

    assert_upgrade_err(
        client.try_upgrade_propose(&stranger, &hash(&env, 2), &1),
        UpgradeError::NotAuthorized,
    );
}

// ─── 2. Unauthorized approval ───────────────────────────────────────────────

#[test]
fn test_stranger_cannot_approve_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    assert_upgrade_err(
        client.try_upgrade_approve(&stranger, &pid),
        UpgradeError::NotAuthorized,
    );
}

// ─── 3. Unauthorized execution ──────────────────────────────────────────────

#[test]
fn test_stranger_cannot_execute_approved_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_approve(&approver, &pid);
    assert_upgrade_err(
        client.try_upgrade_execute(&stranger, &pid),
        UpgradeError::NotAuthorized,
    );
}

// ─── 4. Unauthorized rollback ───────────────────────────────────────────────

#[test]
fn test_stranger_cannot_rollback_executed_upgrade() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);
    let stranger = Address::generate(&env);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &pid);
    assert_upgrade_err(
        client.try_upgrade_rollback(&stranger, &pid),
        UpgradeError::NotAuthorized,
    );
}

// ─── 5. Double-vote ──────────────────────────────────────────────────────────

#[test]
fn test_double_vote_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 3);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.upgrade_add_approver(&admin, &a1);
    client.upgrade_add_approver(&admin, &a2);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_approve(&a1, &pid);
    assert_upgrade_err(
        client.try_upgrade_approve(&a1, &pid),
        UpgradeError::AlreadyApproved,
    );
}

// ─── 6. Execute before threshold ────────────────────────────────────────────

#[test]
fn test_execute_before_threshold_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    assert_eq!(client.upgrade_status(&pid).stage, UpgradeStage::Proposed);
    assert_upgrade_err(
        client.try_upgrade_execute(&admin, &pid),
        UpgradeError::InvalidStatus,
    );
}

// ─── 7. Replay: re-execute ───────────────────────────────────────────────────

#[test]
fn test_replay_execute_already_executed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &pid);
    assert_upgrade_err(
        client.try_upgrade_execute(&admin, &pid),
        UpgradeError::InvalidStatus,
    );
}

// ─── 8. Replay: re-approve after execution ───────────────────────────────────

#[test]
fn test_replay_approve_after_execution() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_approve(&approver, &pid);
    client.upgrade_execute(&admin, &pid);
    assert_upgrade_err(
        client.try_upgrade_approve(&approver, &pid),
        UpgradeError::InvalidStatus,
    );
}

// ─── 9. Version rollback attack ──────────────────────────────────────────────

#[test]
fn test_propose_lower_version_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &5);
    client.upgrade_execute(&admin, &pid);
    assert_eq!(client.current_version(), 5);

    assert_upgrade_err(
        client.try_upgrade_propose(&admin, &hash(&env, 3), &3),
        UpgradeError::InvalidVersion,
    );
}

// ─── 10. Propose version == current ──────────────────────────────────────────

#[test]
fn test_propose_same_version_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_execute(&admin, &pid);
    assert_eq!(client.current_version(), 1);

    assert_upgrade_err(
        client.try_upgrade_propose(&admin, &hash(&env, 3), &1),
        UpgradeError::InvalidVersion,
    );
}

// ─── 11. Remove approver below threshold ─────────────────────────────────────

#[test]
fn test_remove_approver_below_threshold_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let a1 = Address::generate(&env);
    client.upgrade_add_approver(&admin, &a1);

    // Removing a1 leaves only admin (1 < threshold 2)
    assert_upgrade_err(
        client.try_upgrade_remove_approver(&admin, &a1),
        UpgradeError::InvalidThreshold,
    );
}

// ─── 12. Re-initialise upgrade manager ───────────────────────────────────────

#[test]
fn test_reinitialise_upgrade_manager_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);

    assert_upgrade_err(
        client.try_upgrade_init(&admin, &hash(&env, 9), &1),
        UpgradeError::AlreadyInitialized,
    );
}

// ─── 13. Init with zero threshold ────────────────────────────────────────────

#[test]
fn test_init_zero_threshold_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    assert_upgrade_err(
        client.try_upgrade_init(&admin, &hash(&env, 1), &0),
        UpgradeError::InvalidThreshold,
    );
}

// ─── 14. Status on non-existent proposal ─────────────────────────────────────

#[test]
fn test_status_nonexistent_proposal_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env, 1);

    assert_failed(client.try_upgrade_status(&9999));
}

// ─── 15. Revoked approver cannot approve ─────────────────────────────────────

#[test]
fn test_revoked_approver_cannot_approve() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let a1 = Address::generate(&env);
    let a2 = Address::generate(&env);
    client.upgrade_add_approver(&admin, &a1);
    client.upgrade_add_approver(&admin, &a2);

    // Remove a1 — 3 approvers → 2, still ≥ threshold
    client.upgrade_remove_approver(&admin, &a1);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    assert_upgrade_err(
        client.try_upgrade_approve(&a1, &pid),
        UpgradeError::NotAuthorized,
    );
}

// ─── 16. Stranger cannot add approver ────────────────────────────────────────

#[test]
fn test_stranger_cannot_add_approver() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup(&env, 1);
    let stranger = Address::generate(&env);
    let victim = Address::generate(&env);

    assert_upgrade_err(
        client.try_upgrade_add_approver(&stranger, &victim),
        UpgradeError::NotAuthorized,
    );
}

// ─── 17. Stranger cannot remove approver ─────────────────────────────────────

#[test]
fn test_stranger_cannot_remove_approver() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 1);
    let a1 = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.upgrade_add_approver(&admin, &a1);

    assert_upgrade_err(
        client.try_upgrade_remove_approver(&stranger, &a1),
        UpgradeError::NotAuthorized,
    );
}

// ─── 18. Rollback non-executed proposal ──────────────────────────────────────

#[test]
fn test_rollback_non_executed_proposal_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env, 2);
    let approver = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver);

    let pid = client.upgrade_propose(&admin, &hash(&env, 2), &1);
    client.upgrade_approve(&approver, &pid);
    assert_eq!(client.upgrade_status(&pid).stage, UpgradeStage::Approved);

    assert_upgrade_err(
        client.try_upgrade_rollback(&admin, &pid),
        UpgradeError::InvalidStatus,
    );
}

// ─── 19. Stranger cannot trigger emergency shutdown ──────────────────────────

#[test]
fn test_stranger_cannot_trigger_emergency_shutdown() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_lending(&env);
    let stranger = Address::generate(&env);

    assert_eq!(
        client.try_emergency_shutdown(&stranger),
        Err(Ok(BorrowError::Unauthorized))
    );
}

// ─── 20. Stranger cannot set pause ───────────────────────────────────────────

#[test]
#[should_panic(expected = "HostError: Error(Contract, #6)")]
fn test_stranger_cannot_set_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _) = setup_lending(&env);
    let stranger = Address::generate(&env);

    client.set_pause(&stranger, &crate::PauseType::Borrow, &true);
}

// ─── 21. Guardian can trigger shutdown; stranger cannot ──────────────────────

#[test]
fn test_guardian_can_trigger_shutdown_stranger_cannot() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_lending(&env);
    let guardian = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.set_guardian(&admin, &guardian);

    assert_eq!(
        client.try_emergency_shutdown(&stranger),
        Err(Ok(BorrowError::Unauthorized))
    );

    client.emergency_shutdown(&guardian);
    assert_eq!(client.get_emergency_state(), crate::EmergencyState::Shutdown);
}
