use crate::{
    ApprovalRevokedEvent, MultisigContract, MultisigContractClient, MultisigError, ProposalAction,
    ProposalStatus,
};
use soroban_sdk::{
    testutils::{Address as TestAddress, Events, Ledger},
    Address, Bytes, Env, Event, Vec,
};

fn setup(threshold: u32, signer_count: u32) -> (Env, Address, Vec<Address>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, MultisigContract);
    let client = MultisigContractClient::new(&env, &contract_id);
    let mut signers = Vec::new(&env);
    for _ in 0..signer_count {
        signers.push_back(Address::generate(&env));
    }
    client.initialize(&signers, &threshold);
    (env, contract_id, signers)
}

fn create_proposal(
    env: &Env,
    client: &MultisigContractClient,
    proposer: &Address,
    threshold: u32,
) -> (u64, Bytes) {
    let hash = Bytes::from_slice(env, b"revoke-test");
    let id = client.create_proposal(
        proposer,
        &ProposalAction::SetThreshold(threshold),
        &hash,
        &100u64,
    );
    (id, hash)
}

#[test]
fn revoke_removes_approval_binding_and_reverts_passed_status() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(1).unwrap();
    let (id, _) = create_proposal(&env, &client, &signers.get(0).unwrap(), 2);
    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.approve_proposal(&signer, &id);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    assert!(client.verify_approval_binding(&id, &signer));

    client.revoke_approval(&signer, &id);
    let revoke_events = env.events().all();

    let proposal = client.get_proposal(&id);
    assert_eq!(proposal.status, ProposalStatus::Active);
    assert!(!proposal.approvals.contains(&signer));
    assert_eq!(client.get_approval_binding(&id, &signer), None);
    assert_eq!(
        revoke_events,
        [ApprovalRevokedEvent {
            proposal_id: id,
            signer: signer.clone(),
        }
        .to_xdr(&env, &contract_id)]
    );
}

#[test]
fn revoke_then_reapprove_restores_binding_and_quorum() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(1).unwrap();
    let (id, _) = create_proposal(&env, &client, &signers.get(0).unwrap(), 2);
    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.approve_proposal(&signer, &id);

    client.revoke_approval(&signer, &id);
    assert!(!client.verify_approval_binding(&id, &signer));
    client.approve_proposal(&signer, &id);

    assert!(client.verify_approval_binding(&id, &signer));
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
}

#[test]
fn revoke_without_approval_and_revoke_twice_return_approval_not_found() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(1).unwrap();
    let (id, _) = create_proposal(&env, &client, &signers.get(0).unwrap(), 2);

    assert_eq!(
        client.try_revoke_approval(&signer, &id),
        Err(Ok(MultisigError::ApprovalNotFound))
    );

    client.approve_proposal(&signer, &id);
    client.revoke_approval(&signer, &id);
    assert_eq!(
        client.try_revoke_approval(&signer, &id),
        Err(Ok(MultisigError::ApprovalNotFound))
    );
}

#[test]
fn revoke_rejects_cancelled_executed_and_expired_proposals() {
    let (env, contract_id, signers) = setup(2, 2);
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(0).unwrap();

    let (cancelled_id, _) = create_proposal(&env, &client, &signer, 1);
    client.approve_proposal(&signer, &cancelled_id);
    client.cancel_proposal(&signer, &cancelled_id);
    assert_eq!(
        client.try_revoke_approval(&signer, &cancelled_id),
        Err(Ok(MultisigError::AlreadyCancelled))
    );

    let (executed_id, executed_hash) = create_proposal(&env, &client, &signer, 1);
    client.approve_proposal(&signer, &executed_id);
    client.approve_proposal(&signers.get(1).unwrap(), &executed_id);
    client.execute_proposal(&signer, &executed_id, &executed_hash);
    assert_eq!(
        client.try_revoke_approval(&signer, &executed_id),
        Err(Ok(MultisigError::AlreadyExecuted))
    );

    let (expired_id, _) = create_proposal(&env, &client, &signer, 1);
    client.approve_proposal(&signer, &expired_id);
    env.ledger().set_sequence_number(102);
    assert_eq!(
        client.try_revoke_approval(&signer, &expired_id),
        Err(Ok(MultisigError::ProposalExpired))
    );
}

#[test]
#[should_panic]
fn revoke_requires_the_signers_authentication() {
    let (env, contract_id, signers) = setup(2, 2);
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(1).unwrap();
    let (id, _) = create_proposal(&env, &client, &signers.get(0).unwrap(), 2);
    client.approve_proposal(&signer, &id);

    env.mock_auths(&[]);
    client.revoke_approval(&signer, &id);
}

#[test]
fn non_signer_cannot_revoke_another_signers_approval() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let signer = signers.get(1).unwrap();
    let outsider = Address::generate(&env);
    let (id, _) = create_proposal(&env, &client, &signers.get(0).unwrap(), 2);
    client.approve_proposal(&signer, &id);

    assert_eq!(
        client.try_revoke_approval(&outsider, &id),
        Err(Ok(MultisigError::Unauthorized))
    );
    assert!(client.get_proposal(&id).approvals.contains(&signer));
}
