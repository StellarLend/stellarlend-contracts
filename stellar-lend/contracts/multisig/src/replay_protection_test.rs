//! Replay-protection and security-metadata tests for issue #1900.
//!
//! These tests exercise every lifecycle edge that can accidentally turn a
//! valid approval into authorization for a different execution context:
//! nonce allocation/consumption, signer-set rotation, reordered approvals,
//! expiry, dispatch failure, and legacy-record migration.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Bytes, Env, Vec};

fn env_with_auth() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup(env: &Env) -> (MultisigContractClient<'_>, Vec<Address>) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);
    let mut signers = Vec::new(env);
    signers.push_back(Address::generate(env));
    signers.push_back(Address::generate(env));
    signers.push_back(Address::generate(env));
    client.initialize(&signers, &2u32);
    (client, signers)
}

fn payload(env: &Env, tag: &[u8]) -> Bytes {
    Bytes::from_slice(env, tag)
}

fn proposal(
    client: &MultisigContractClient,
    proposer: &Address,
    action: &ProposalAction,
    hash: &Bytes,
    ttl: u64,
) -> u64 {
    client.create_proposal(proposer, action, hash, &ttl)
}

fn approve_two(client: &MultisigContractClient, signers: &Vec<Address>, id: u64) {
    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.approve_proposal(&signers.get(1).unwrap(), &id);
}

#[test]
fn allocates_monotonic_nonces_and_consumes_exactly_once() {
    let env = env_with_auth();
    let (client, signers) = setup(&env);
    let hash = payload(&env, b"nonce");

    let first = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        100,
    );
    let second = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        100,
    );

    assert_eq!(client.get_proposal_nonce(&first), 0);
    assert_eq!(client.get_proposal_nonce(&second), 1);
    assert!(!client.is_nonce_consumed(&0));

    approve_two(&client, &signers, first);
    client.execute_proposal(&signers.get(0).unwrap(), &first, &hash);
    assert!(client.is_nonce_consumed(&0));

    assert_eq!(
        client.try_execute_proposal(&signers.get(1).unwrap(), &first, &hash),
        Err(Ok(MultisigError::AlreadyExecuted))
    );
    assert!(!client.is_nonce_consumed(&1));
}

#[test]
fn approvals_are_order_independent_but_still_reach_quorum() {
    let env = env_with_auth();
    let (client, signers) = setup(&env);
    let hash = payload(&env, b"reordered");
    let id = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        100,
    );

    client.approve_proposal(&signers.get(1).unwrap(), &id);
    client.approve_proposal(&signers.get(0).unwrap(), &id);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    client.execute_proposal(&signers.get(2).unwrap(), &id, &hash);

    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
    assert!(client.is_nonce_consumed(&client.get_proposal_nonce(&id)));
}

#[test]
fn signer_rotation_invalidates_approvals_for_the_old_context() {
    let env = env_with_auth();
    let (client, signers) = setup(&env);
    let old_hash = payload(&env, b"old-context");
    let old_id = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &old_hash,
        500,
    );
    let old_binding = client.approval_binding_hash(&old_id, &signers.get(0).unwrap());
    let old_set_hash = client.get_signer_set_hash();

    let replacement = Address::generate(&env);
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(signers.get(0).unwrap());
    new_signers.push_back(signers.get(1).unwrap());
    new_signers.push_back(replacement.clone());
    let rotation_hash = payload(&env, b"rotate");
    let rotation_id = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::RotateSigners(new_signers.clone()),
        &rotation_hash,
        500,
    );
    approve_two(&client, &signers, rotation_id);
    client.execute_proposal(&signers.get(0).unwrap(), &rotation_id, &rotation_hash);

    let new_set_hash = client.get_signer_set_hash();
    assert_ne!(old_set_hash, new_set_hash);
    assert_eq!(
        client.approval_binding_hash(&old_id, &signers.get(0).unwrap()),
        old_binding
    );

    // The replacement signer is valid for the current set but cannot approve
    // a proposal whose security metadata was captured before rotation.
    assert_eq!(
        client.try_approve_proposal(&replacement, &old_id),
        Err(Ok(MultisigError::SignerSetChanged))
    );
}

#[test]
fn expiry_is_checked_before_execution_and_nonce_remains_unconsumed() {
    let env = env_with_auth();
    let (client, signers) = setup(&env);
    let hash = payload(&env, b"expiry");
    let start = env.ledger().sequence();
    let id = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        1,
    );
    approve_two(&client, &signers, id);
    env.ledger().set_sequence_number(start + 2);

    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::ProposalExpired))
    );
    assert!(!client.is_nonce_consumed(&client.get_proposal_nonce(&id)));
}

#[test]
fn failed_dispatch_does_not_consume_nonce_or_mark_execution() {
    let env = env_with_auth();
    let (client, signers) = setup(&env);
    let hash = payload(&env, b"failed-dispatch");
    let id = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(0),
        &hash,
        100,
    );
    approve_two(&client, &signers, id);

    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::InvalidThreshold))
    );
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    assert!(!client.is_nonce_consumed(&client.get_proposal_nonce(&id)));
}

#[test]
fn legacy_security_metadata_can_be_migrated_without_changing_proposal_layout() {
    let env = env_with_auth();
    let (client, signers) = setup(&env);
    let contract_id = client.address.clone();
    let hash = payload(&env, b"legacy");
    let id = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        100,
    );

    // Simulate a record written by the pre-nonce version. The Proposal value
    // stays intact; only the new metadata keys are absent.
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&MultisigDataKey::ProposalNonce(id));
        env.storage()
            .persistent()
            .remove(&MultisigDataKey::ProposalSignerSetHash(id));
    });
    assert_eq!(
        client.try_approve_proposal(&signers.get(0).unwrap(), &id),
        Err(Ok(MultisigError::LegacyProposal))
    );

    client.migrate_proposal_security(&signers.get(0).unwrap(), &id);
    assert!(client.get_proposal_signer_set_hash(&id).is_some());
    approve_two(&client, &signers, id);
    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash);
    assert!(client.is_nonce_consumed(&client.get_proposal_nonce(&id)));
}

#[test]
fn legacy_proposal_with_removed_approver_is_not_silently_rebound() {
    let env = env_with_auth();
    let (client, signers) = setup(&env);
    let contract_id = client.address.clone();
    let hash = payload(&env, b"legacy-removed");
    let id = proposal(
        &client,
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        100,
    );
    client.approve_proposal(&signers.get(0).unwrap(), &id);
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&MultisigDataKey::ProposalNonce(id));
        env.storage()
            .persistent()
            .remove(&MultisigDataKey::ProposalSignerSetHash(id));
    });

    let replacement = Address::generate(&env);
    let mut rotated = Vec::new(&env);
    rotated.push_back(signers.get(1).unwrap());
    rotated.push_back(signers.get(2).unwrap());
    rotated.push_back(replacement);
    let rotate_hash = payload(&env, b"legacy-rotate");
    let rotate_id = proposal(
        &client,
        &signers.get(1).unwrap(),
        &ProposalAction::RotateSigners(rotated),
        &rotate_hash,
        100,
    );
    approve_two(&client, &signers, rotate_id);
    client.execute_proposal(&signers.get(1).unwrap(), &rotate_id, &rotate_hash);

    assert_eq!(
        client.try_migrate_proposal_security(&signers.get(1).unwrap(), &id),
        Err(Ok(MultisigError::SignerSetChanged))
    );
}
