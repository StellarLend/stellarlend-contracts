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
    let (env, client, admin, approver, _) = setup_upgrade(1);
    let new_hash = wasm_hash(&env, 8);
    let proposal_id = client.upgrade_propose(&admin, &new_hash, &1);
    assert_eq!(
        client
            .upgrade_status(&proposal_id)
            .proposal
            .required_approvals,
        1
    );

    client.upgrade_add_approver(&admin, &approver);
    client.upgrade_set_required_approvals(&admin, &2);

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
