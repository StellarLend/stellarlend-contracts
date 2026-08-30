#![cfg(test)]

use crate::upgrade::{
    UpgradeProposalStatus, DEFAULT_PROPOSAL_EXPIRY_LEDGERS, MIN_THRESHOLD_DELAY_LEDGERS,
};
use crate::{LendingContract, LendingContractClient, LendingError};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, BytesN, Env};

fn wasm_hash(env: &Env, byte: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = byte;
    BytesN::from_array(env, &bytes)
}

fn setup_upgrade(
    required_approvals: u32,
) -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let approver = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.initialize(&admin);
    client.upgrade_init(&admin, &wasm_hash(&env, 1), &required_approvals);
    if required_approvals > 1 {
        client.upgrade_add_approver(&admin, &approver);
    }
    (env, client, admin, approver, stranger)
}

fn advance_to_eta(env: &Env, eta_ledger: u32) {
    env.ledger().set_sequence_number(eta_ledger);
}

#[test]
fn upgrade_init_records_version_and_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let hash = wasm_hash(&env, 9);
    client.initialize(&admin);
    client.upgrade_init(&admin, &hash, &2);
    assert_eq!(client.current_version(), 0);
    assert_eq!(client.current_wasm_hash(), hash);
    assert_eq!(client.get_required_approvals(), 2);
}

#[test]
fn propose_approve_execute_happy_path_with_threshold_one() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let new_hash = wasm_hash(&env, 2);
    let proposal_id = client.upgrade_propose(&admin, &new_hash, &1);
    assert_eq!(client.upgrade_approve(&admin, &proposal_id), 1);

    let status = client.upgrade_status(&proposal_id);
    assert_eq!(status.status, UpgradeProposalStatus::Pending);
    assert_eq!(status.approval_count, 1);

    advance_to_eta(&env, status.proposal.eta_ledger);
    client.upgrade_execute(&admin, &proposal_id);

    assert_eq!(client.current_version(), 1);
    assert_eq!(client.current_wasm_hash(), new_hash);
    assert_eq!(
        client.upgrade_status(&proposal_id).status,
        UpgradeProposalStatus::Executed
    );
}

#[test]
fn execute_before_timelock_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 3), &1);
    client.upgrade_approve(&admin, &proposal_id);

    let res = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(res, Err(Ok(LendingError::ProposalNotReady))));
}

#[test]
fn execute_without_enough_approvals_is_rejected() {
    let (env, client, admin, approver, _) = setup_upgrade(2);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 4), &1);

    let eta = client.upgrade_status(&proposal_id).proposal.eta_ledger;
    advance_to_eta(&env, eta);

    let res = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::InsufficientUpgradeApprovals))
    ));

    client.upgrade_approve(&admin, &proposal_id);
    let res = client.try_upgrade_execute(&approver, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::InsufficientUpgradeApprovals))
    ));
}

#[test]
fn expired_proposal_cannot_be_approved_or_executed() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 5), &1);
    let expires = client
        .upgrade_status(&proposal_id)
        .proposal
        .expires_at_ledger;
    env.ledger().set_sequence_number(expires.saturating_add(1));

    let approve = client.try_upgrade_approve(&admin, &proposal_id);
    assert!(matches!(approve, Err(Ok(LendingError::ProposalExpired))));

    let execute = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(execute, Err(Ok(LendingError::ProposalExpired))));
}

#[test]
fn duplicate_approval_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 6), &1);
    client.upgrade_approve(&admin, &proposal_id);
    let res = client.try_upgrade_approve(&admin, &proposal_id);
    assert!(matches!(res, Err(Ok(LendingError::AlreadyApproved))));
}

#[test]
fn double_execute_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 3), &1);
    client.upgrade_approve(&admin, &proposal_id);
    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    client.upgrade_execute(&admin, &proposal_id);

    let res = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::ProposalAlreadyExecuted))
    ));
}

#[test]
fn unauthorized_caller_cannot_approve_or_execute() {
    let (env, client, admin, _, stranger) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 7), &1);

    let approve = client.try_upgrade_approve(&stranger, &proposal_id);
    assert!(matches!(approve, Err(Ok(LendingError::Unauthorized))));

    client.upgrade_approve(&admin, &proposal_id);
    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );

    let execute = client.try_upgrade_execute(&stranger, &proposal_id);
    assert!(matches!(execute, Err(Ok(LendingError::Unauthorized))));
}

#[test]
fn threshold_snapshot_is_fixed_at_propose_time() {
    // Issue #1940: the approval *threshold* is snapshot at propose time so
    // later `required_approvals` configuration changes cannot retroactively
    // weaken or strengthen an in-flight vote. We only mutate the threshold
    // here; rotating the approver set would now (correctly) trip the new
    // `ApproverSetChanged` invariant and is covered by a dedicated test.
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let approver1 = Address::generate(&env);
    let approver2 = Address::generate(&env);
    let approver3 = Address::generate(&env);
    client.initialize(&admin);
    client.upgrade_init(&admin, &wasm_hash(&env, 1), &3);
    client.upgrade_add_approver(&admin, &approver1);
    client.upgrade_add_approver(&admin, &approver2);
    client.upgrade_add_approver(&admin, &approver3);

    let new_hash = wasm_hash(&env, 8);
    let proposal_id = client.upgrade_propose(&admin, &new_hash, &1);
    assert_eq!(
        client
            .upgrade_status(&proposal_id)
            .proposal
            .required_approvals,
        1
    );

    // Lower the live threshold to 1. The proposal must still require 3
    // approvals because the threshold is snapshot at propose time. We do
    // NOT add or remove approvers here so the `ApproverSetChanged` guard
    // does not fire (covered by a separate test).
    client.upgrade_set_required_approvals(&admin, &1);

    // One approval is not enough under the snapshotted threshold of 3.
    client.upgrade_approve(&admin, &proposal_id);
    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    client.upgrade_execute(&admin, &proposal_id);
    assert_eq!(client.current_version(), 1);
}

#[test]
fn propose_rejects_non_monotonic_version() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let res = client.try_upgrade_propose(&admin, &wasm_hash(&env, 10), &0);
    assert!(matches!(res, Err(Ok(LendingError::InvalidUpgradeVersion))));
}

#[test]
fn proposal_records_expected_timelock_and_expiry() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let start = env.ledger().sequence();
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 11), &1);
    let proposal = client.upgrade_status(&proposal_id).proposal;
    assert_eq!(proposal.eta_ledger, start + MIN_THRESHOLD_DELAY_LEDGERS);
    assert_eq!(
        proposal.expires_at_ledger,
        start + DEFAULT_PROPOSAL_EXPIRY_LEDGERS
    );
}

#[test]
fn upgrade_init_is_idempotent_guarded() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.upgrade_init(&admin, &wasm_hash(&env, 12), &1);
    let res = client.try_upgrade_init(&admin, &wasm_hash(&env, 13), &1);
    assert!(matches!(res, Err(Ok(LendingError::AlreadyInitialized))));
}

// ---------------------------------------------------------------------------
// Issue #1940 - upgrade governance invariants & recovery tests.
//
// These tests cover the success / rejection / cancellation / retry path
// state machine, nonce-bound approval bindings, approver-set rotation
// invalidating stale approvals, and idempotent execution rollback safety.
// ---------------------------------------------------------------------------

#[test]
fn cancel_pending_proposal_marks_cancelled_status() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 20), &1);
    client.upgrade_cancel(&admin, &proposal_id);

    assert!(client.is_upgrade_proposal_cancelled(&proposal_id));
    assert_eq!(
        client.upgrade_status(&proposal_id).status,
        UpgradeProposalStatus::Cancelled
    );
}

#[test]
fn cancelled_proposal_cannot_be_approved_or_executed() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 21), &1);
    client.upgrade_cancel(&admin, &proposal_id);

    let approve = client.try_upgrade_approve(&admin, &proposal_id);
    assert!(matches!(
        approve,
        Err(Ok(LendingError::UpgradeProposalCancelled))
    ));

    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    let execute = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        execute,
        Err(Ok(LendingError::UpgradeProposalCancelled))
    ));
}

#[test]
fn double_cancel_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 22), &1);
    client.upgrade_cancel(&admin, &proposal_id);
    let res = client.try_upgrade_cancel(&admin, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::UpgradeProposalCancelled))
    ));
}

#[test]
fn cancel_executed_proposal_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 23), &1);
    client.upgrade_approve(&admin, &proposal_id);
    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    client.upgrade_execute(&admin, &proposal_id);
    let res = client.try_upgrade_cancel(&admin, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::ProposalAlreadyExecuted))
    ));
}

#[test]
fn cancel_expired_proposal_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 24), &1);
    let expires = client
        .upgrade_status(&proposal_id)
        .proposal
        .expires_at_ledger;
    env.ledger().set_sequence_number(expires.saturating_add(1));
    let res = client.try_upgrade_cancel(&admin, &proposal_id);
    assert!(matches!(res, Err(Ok(LendingError::ProposalExpired))));
}

#[test]
fn cancel_requires_admin() {
    let (env, client, admin, _, stranger) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 25), &1);
    let res = client.try_upgrade_cancel(&stranger, &proposal_id);
    assert!(matches!(res, Err(Ok(LendingError::Unauthorized))));
    assert!(!client.is_upgrade_proposal_cancelled(&proposal_id));
}

#[test]
fn approver_rotation_invalidates_in_flight_approval() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let approver1 = Address::generate(&env);
    let approver2 = Address::generate(&env);
    client.initialize(&admin);
    client.upgrade_init(&admin, &wasm_hash(&env, 1), &1);
    client.upgrade_add_approver(&admin, &approver1);
    client.upgrade_add_approver(&admin, &approver2);

    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 26), &1);
    client.upgrade_approve(&admin, &proposal_id);

    // Admin removes one approver - this rotates the live approver set, so
    // the snapshot captured at propose time no longer matches.
    client.upgrade_remove_approver(&admin, &approver1);

    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    let execute = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(execute, Err(Ok(LendingError::ApproverSetChanged))));

    // Even attempting to record an additional approval must fail so a
    // stale response cannot create contradictory client state.
    let approve = client.try_upgrade_approve(&admin, &proposal_id);
    assert!(matches!(approve, Err(Ok(LendingError::ApproverSetChanged))));
}

#[test]
fn proposal_approver_set_hash_is_snapshot_at_propose() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let live_before = client.get_upgrade_approver_set_hash();
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 27), &1);
    assert_eq!(
        client.get_upgrade_proposal_signer_hash(&proposal_id),
        Some(live_before)
    );
}

#[test]
fn approve_binding_is_recorded_and_distinct_per_proposal() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let first = client.upgrade_propose(&admin, &wasm_hash(&env, 28), &1);
    let second = client.upgrade_propose(&admin, &wasm_hash(&env, 29), &2);

    client.upgrade_approve(&admin, &first);
    client.upgrade_approve(&admin, &second);

    let binding_first = client.get_upgrade_approval_binding(&first, &admin);
    let binding_second = client.get_upgrade_approval_binding(&second, &admin);
    assert!(binding_first.is_some());
    assert!(binding_second.is_some());
    assert_ne!(binding_first, binding_second);
}

#[test]
fn removed_approver_cannot_approve_after_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let approver1 = Address::generate(&env);
    let approver2 = Address::generate(&env);
    client.initialize(&admin);
    client.upgrade_init(&admin, &wasm_hash(&env, 1), &1);
    client.upgrade_add_approver(&admin, &approver1);
    client.upgrade_add_approver(&admin, &approver2);

    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 30), &1);
    client.upgrade_remove_approver(&admin, &approver1);

    // The removed approver is no longer in the live set, so the membership
    // check rejects them with `Unauthorized` rather than silently letting
    // their pre-existing approval count toward quorum.
    let res = client.try_upgrade_approve(&approver1, &proposal_id);
    assert!(matches!(res, Err(Ok(LendingError::Unauthorized))));
}

#[test]
fn duplicate_proposal_id_is_uniquely_allocated_by_chain() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let first = client.upgrade_propose(&admin, &wasm_hash(&env, 31), &1);
    let second = client.upgrade_propose(&admin, &wasm_hash(&env, 32), &2);
    let third = client.upgrade_propose(&admin, &wasm_hash(&env, 33), &3);
    assert!(first < second);
    assert!(second < third);
    assert_eq!(client.upgrade_status(&first).proposal.id, first);
    assert_eq!(client.upgrade_status(&second).proposal.id, second);
}

#[test]
fn double_execute_is_rejected_after_success() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 34), &1);
    client.upgrade_approve(&admin, &proposal_id);
    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    client.upgrade_execute(&admin, &proposal_id);
    let res = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::ProposalAlreadyExecuted))
    ));
}

#[test]
fn approve_after_execute_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 35), &1);
    client.upgrade_approve(&admin, &proposal_id);
    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    client.upgrade_execute(&admin, &proposal_id);

    let res = client.try_upgrade_approve(&admin, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::ProposalAlreadyExecuted))
    ));
}

#[test]
fn execute_after_cancel_is_rejected() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 36), &1);
    client.upgrade_approve(&admin, &proposal_id);
    client.upgrade_cancel(&admin, &proposal_id);

    advance_to_eta(
        &env,
        client.upgrade_status(&proposal_id).proposal.eta_ledger,
    );
    let res = client.try_upgrade_execute(&admin, &proposal_id);
    assert!(matches!(
        res,
        Err(Ok(LendingError::UpgradeProposalCancelled))
    ));
}

#[test]
fn cancel_does_not_bump_proposal_counter() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 37), &1);
    client.upgrade_cancel(&admin, &proposal_id);
    let next = client.upgrade_propose(&admin, &wasm_hash(&env, 38), &2);
    // Cancelling must not consume or skip an id: the next id is one above
    // the cancelled one, so client state stays consistent.
    assert!(next > proposal_id);
}

#[test]
fn expired_proposal_status_surfaces_via_view() {
    let (env, client, admin, _, _) = setup_upgrade(1);
    let proposal_id = client.upgrade_propose(&admin, &wasm_hash(&env, 39), &1);
    let expires = client
        .upgrade_status(&proposal_id)
        .proposal
        .expires_at_ledger;
    env.ledger().set_sequence_number(expires.saturating_add(1));
    assert_eq!(
        client.upgrade_status(&proposal_id).status,
        UpgradeProposalStatus::Expired
    );
}
