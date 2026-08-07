#![cfg(test)]

use crate::{
    MultisigContract, MultisigContractClient, MultisigError, ProposalAction, ProposalStatus,
};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Bytes, Env, Vec};

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_bytes(env: &Env, data: &[u8]) -> Bytes {
    Bytes::from_slice(env, data)
}

fn setup_multisig(env: &Env, threshold: u32, signer_count: usize) -> (Address, Vec<Address>) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);

    let mut signers = Vec::new(env);
    for _ in 0..signer_count {
        signers.push_back(Address::generate(env));
    }

    client.initialize(&signers, &threshold);
    (contract_id, signers)
}

#[test]
fn test_revoke_approval_removes_signer_and_reduces_quorum() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env, 2, 2);
    let client = MultisigContractClient::new(&env, &contract_id);

    let signer_a = signers.get(0).unwrap();
    let signer_b = signers.get(1).unwrap();
    let hash = make_bytes(&env, b"payload_hash");

    let proposal_id = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(2),
        &hash,
        &500u64,
    );

    client.approve_proposal(&signer_a, &proposal_id);
    let proposal_mid = client.get_proposal(&proposal_id);
    assert_eq!(proposal_mid.status, ProposalStatus::Active);

    client.approve_proposal(&signer_b, &proposal_id);
    let proposal_passed = client.get_proposal(&proposal_id);
    assert_eq!(proposal_passed.status, ProposalStatus::Passed);

    client.revoke_approval(&signer_b, &proposal_id);

    let proposal_after = client.get_proposal(&proposal_id);
    assert!(!proposal_after.approvals.contains(&signer_b));
    assert!(proposal_after.approvals.contains(&signer_a));
    assert_eq!(proposal_after.approvals.len(), 1);
    assert_eq!(proposal_after.status, ProposalStatus::Active);

    // Attempting execution fails because status returned to Active (QuorumNotReached)
    assert_eq!(
        client.try_execute_proposal(&signer_a, &proposal_id, &hash),
        Err(Ok(MultisigError::QuorumNotReached))
    );
}

#[test]
fn test_revoke_nonexistent_approval_returns_error() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env, 1, 1);
    let client = MultisigContractClient::new(&env, &contract_id);

    let signer_a = signers.get(0).unwrap();
    let hash = make_bytes(&env, b"payload_hash");

    let proposal_id = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(1),
        &hash,
        &500u64,
    );

    assert_eq!(
        client.try_revoke_approval(&signer_a, &proposal_id),
        Err(Ok(MultisigError::ApprovalNotFound))
    );
}

#[test]
fn test_revoke_by_non_approver_is_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env, 1, 2);
    let client = MultisigContractClient::new(&env, &contract_id);

    let signer_a = signers.get(0).unwrap();
    let signer_b = signers.get(1).unwrap();
    let hash = make_bytes(&env, b"payload_hash");

    let proposal_id = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(1),
        &hash,
        &500u64,
    );

    client.approve_proposal(&signer_a, &proposal_id);

    assert_eq!(
        client.try_revoke_approval(&signer_b, &proposal_id),
        Err(Ok(MultisigError::ApprovalNotFound))
    );
}

#[test]
fn test_revoke_by_unauthorized_non_signer_is_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env, 1, 1);
    let client = MultisigContractClient::new(&env, &contract_id);

    let signer_a = signers.get(0).unwrap();
    let outsider = Address::generate(&env);
    let hash = make_bytes(&env, b"payload_hash");

    let proposal_id = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(1),
        &hash,
        &500u64,
    );

    client.approve_proposal(&signer_a, &proposal_id);

    assert_eq!(
        client.try_revoke_approval(&outsider, &proposal_id),
        Err(Ok(MultisigError::Unauthorized))
    );
}

#[test]
fn test_revoke_on_expired_cancelled_executed_proposal_is_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env, 1, 1);
    let client = MultisigContractClient::new(&env, &contract_id);

    let signer_a = signers.get(0).unwrap();
    let hash = make_bytes(&env, b"payload_hash");

    // 1. Expired proposal test
    let proposal_id = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(1),
        &hash,
        &10u64,
    );
    client.approve_proposal(&signer_a, &proposal_id);

    env.ledger().set_sequence_number(env.ledger().sequence() + 20);
    assert_eq!(
        client.try_revoke_approval(&signer_a, &proposal_id),
        Err(Ok(MultisigError::ProposalExpired))
    );

    // 2. Executed proposal test
    let proposal_id_2 = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(1),
        &hash,
        &500u64,
    );
    client.approve_proposal(&signer_a, &proposal_id_2);
    client.execute_proposal(&signer_a, &proposal_id_2, &hash);

    assert_eq!(
        client.try_revoke_approval(&signer_a, &proposal_id_2),
        Err(Ok(MultisigError::AlreadyExecuted))
    );

    // 3. Cancelled proposal test
    let proposal_id_3 = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(1),
        &hash,
        &500u64,
    );
    client.approve_proposal(&signer_a, &proposal_id_3);
    client.cancel_proposal(&signer_a, &proposal_id_3);

    assert_eq!(
        client.try_revoke_approval(&signer_a, &proposal_id_3),
        Err(Ok(MultisigError::AlreadyCancelled))
    );
}

#[test]
fn test_revoke_then_reapprove_restores_quorum() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env, 2, 2);
    let client = MultisigContractClient::new(&env, &contract_id);

    let signer_a = signers.get(0).unwrap();
    let signer_b = signers.get(1).unwrap();
    let hash = make_bytes(&env, b"payload_hash");

    let proposal_id = client.create_proposal(
        &signer_a,
        &ProposalAction::SetThreshold(2),
        &hash,
        &500u64,
    );

    client.approve_proposal(&signer_a, &proposal_id);
    client.approve_proposal(&signer_b, &proposal_id);
    let prop_passed = client.get_proposal(&proposal_id);
    assert_eq!(prop_passed.status, ProposalStatus::Passed);

    client.revoke_approval(&signer_b, &proposal_id);
    let prop_active = client.get_proposal(&proposal_id);
    assert_eq!(prop_active.status, ProposalStatus::Active);

    client.approve_proposal(&signer_b, &proposal_id);

    let prop_restored = client.get_proposal(&proposal_id);
    assert!(prop_restored.approvals.contains(&signer_b));
    assert_eq!(prop_restored.status, ProposalStatus::Passed);
}
