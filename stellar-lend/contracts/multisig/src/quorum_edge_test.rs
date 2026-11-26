use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Bytes, Env, Vec};

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn payload(env: &Env, tag: &[u8]) -> Bytes {
    Bytes::from_slice(env, tag)
}

fn setup(env: &Env, threshold: u32, signer_count: u32) -> (Address, Vec<Address>) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);
    let mut signers = Vec::new(env);
    for _ in 0..signer_count {
        signers.push_back(Address::generate(env));
    }
    client.initialize(&signers, &threshold);
    (contract_id, signers)
}

fn create_proposal(
    env: &Env,
    client: &MultisigContractClient,
    signers: &Vec<Address>,
    action: &ProposalAction,
    tag: &[u8],
) -> (u64, Bytes) {
    let hash = payload(env, tag);
    let id = client.create_proposal(&signers.get(0).unwrap(), action, &hash, &500u64);
    (id, hash)
}

#[test]
fn test_duplicate_approval_counted_once() {
    let env = make_env();
    let (contract_id, signers) = setup(&env, 2, 2);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(2),
        b"duplicate-approval",
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    assert_eq!(
        client.try_approve_proposal(&signers.get(0).unwrap(), &id),
        Err(Ok(MultisigError::AlreadyApproved))
    );

    let proposal = client.get_proposal(&id);
    assert_eq!(proposal.approvals.len(), 1);
    assert!(proposal.approvals.contains(signers.get(0).unwrap()));

    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::QuorumNotReached))
    );

    client.approve_proposal(&signers.get(1).unwrap(), &id);
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Ok(Ok(()))
    );
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
}

#[test]
fn test_removed_signer_excluded_from_quorum() {
    let env = make_env();
    let (contract_id, signers) = setup(&env, 2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let first = signers.get(0).unwrap();
    let second = signers.get(1).unwrap();
    let third = signers.get(2).unwrap();

    let (old_id, old_hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        b"old-context",
    );
    client.approve_proposal(&first, &old_id);
    client.approve_proposal(&second, &old_id);

    let replacement = Address::generate(&env);
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(first.clone());
    new_signers.push_back(third.clone());
    new_signers.push_back(replacement);

    let (rotation_id, rotation_hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::RotateSigners(new_signers.clone()),
        b"rotate-signers",
    );
    client.approve_proposal(&first, &rotation_id);
    client.approve_proposal(&second, &rotation_id);
    client.execute_proposal(&first, &rotation_id, &rotation_hash);

    assert_eq!(client.get_signers(), new_signers);
    assert_eq!(
        client.try_execute_proposal(&first, &old_id, &old_hash),
        Err(Ok(MultisigError::SignerSetChanged))
    );

    let (new_id, new_hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(2),
        b"after-rotation",
    );
    assert_eq!(
        client.try_approve_proposal(&second, &new_id),
        Err(Ok(MultisigError::Unauthorized))
    );
    client.approve_proposal(&first, &new_id);
    client.approve_proposal(&third, &new_id);
    assert_eq!(client.get_proposal(&new_id).status, ProposalStatus::Passed);
    client.execute_proposal(&first, &new_id, &new_hash);
    assert_eq!(
        client.get_proposal(&new_id).status,
        ProposalStatus::Executed
    );
}

#[test]
fn test_raised_threshold_requires_additional_approval() {
    let env = make_env();
    let (contract_id, signers) = setup(&env, 2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let first = signers.get(0).unwrap();
    let second = signers.get(1).unwrap();
    let third = signers.get(2).unwrap();

    let (raise_id, raise_hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        b"raise-threshold",
    );
    client.approve_proposal(&first, &raise_id);
    client.approve_proposal(&second, &raise_id);
    client.execute_proposal(&first, &raise_id, &raise_hash);
    assert_eq!(client.get_threshold(), 3);

    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(2),
        b"target-proposal",
    );
    client.approve_proposal(&first, &id);
    client.approve_proposal(&second, &id);
    assert_eq!(
        client.try_execute_proposal(&first, &id, &hash),
        Err(Ok(MultisigError::QuorumNotReached))
    );

    client.approve_proposal(&third, &id);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    client.execute_proposal(&first, &id, &hash);
    assert_eq!(client.get_threshold(), 2);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
}

#[test]
fn test_exactly_quorum_executes() {
    let env = make_env();
    let (contract_id, signers) = setup(&env, 3, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        b"exact-quorum",
    );

    for signer in signers.iter() {
        client.approve_proposal(&signer, &id);
    }
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
}

#[test]
fn test_one_below_quorum_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup(&env, 3, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        b"below-quorum",
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.approve_proposal(&signers.get(1).unwrap(), &id);
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::QuorumNotReached))
    );
}

#[test]
fn test_non_signer_cannot_approve() {
    let env = make_env();
    let (contract_id, signers) = setup(&env, 1, 2);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, _hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(1),
        b"non-signer",
    );
    let outsider = Address::generate(&env);

    assert_eq!(
        client.try_approve_proposal(&outsider, &id),
        Err(Ok(MultisigError::Unauthorized))
    );
}

#[test]
fn test_approve_already_executed_proposal_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup(&env, 1, 2);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(1),
        b"already-executed",
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash);
    assert_eq!(
        client.try_approve_proposal(&signers.get(0).unwrap(), &id),
        Err(Ok(MultisigError::AlreadyExecuted))
    );
}
