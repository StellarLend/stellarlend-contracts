use crate::{
    MultisigContract, MultisigContractClient, MultisigError, ProposalAction, ProposalStatus,
};
use soroban_sdk::testutils::Address as TestAddress;
use soroban_sdk::{Address, Bytes, Env, Vec};

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

fn propose(
    env: &Env,
    client: &MultisigContractClient,
    proposer: &Address,
    threshold: u32,
) -> (u64, Bytes) {
    let hash = Bytes::from_slice(env, b"live-quorum");
    let id = client.create_proposal(
        proposer,
        &ProposalAction::SetThreshold(threshold),
        &hash,
        &1_000u64,
    );
    (id, hash)
}

fn approve(client: &MultisigContractClient, signers: &Vec<Address>, id: u64, count: u32) {
    for i in 0..count {
        client.approve_proposal(&signers.get(i).unwrap(), &id);
    }
}

#[test]
fn raising_threshold_invalidates_cached_pass_until_new_approval() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (raise_id, raise_hash) = propose(&env, &client, &signers.get(0).unwrap(), 3);
    approve(&client, &signers, raise_id, 2);
    let (target_id, target_hash) = propose(&env, &client, &signers.get(0).unwrap(), 1);
    approve(&client, &signers, target_id, 2);
    assert_eq!(client.get_proposal(&target_id).status, ProposalStatus::Passed);

    client.execute_proposal(&signers.get(0).unwrap(), &raise_id, &raise_hash);
    assert_eq!(client.get_threshold(), 3);
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &target_id, &target_hash),
        Err(Ok(MultisigError::QuorumNotReached))
    );

    client.approve_proposal(&signers.get(2).unwrap(), &target_id);
    client.execute_proposal(&signers.get(0).unwrap(), &target_id, &target_hash);
    assert_eq!(
        client.get_proposal(&target_id).status,
        ProposalStatus::Executed
    );
}

#[test]
fn lowering_threshold_makes_existing_active_proposal_executable() {
    let (env, contract_id, signers) = setup(3, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (lower_id, lower_hash) = propose(&env, &client, &signers.get(0).unwrap(), 2);
    approve(&client, &signers, lower_id, 3);
    let (target_id, target_hash) = propose(&env, &client, &signers.get(0).unwrap(), 1);
    approve(&client, &signers, target_id, 2);
    assert_eq!(client.get_proposal(&target_id).status, ProposalStatus::Active);

    client.execute_proposal(&signers.get(0).unwrap(), &lower_id, &lower_hash);
    assert_eq!(client.get_threshold(), 2);
    assert_eq!(client.get_proposal(&target_id).status, ProposalStatus::Active);

    client.execute_proposal(&signers.get(0).unwrap(), &target_id, &target_hash);
    assert_eq!(
        client.get_proposal(&target_id).status,
        ProposalStatus::Executed
    );
}

#[test]
fn batch_rechecks_quorum_after_threshold_is_raised_and_rolls_back() {
    let (env, contract_id, signers) = setup(2, 3);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (raise_id, raise_hash) = propose(&env, &client, &signers.get(0).unwrap(), 3);
    approve(&client, &signers, raise_id, 2);
    let (target_id, target_hash) = propose(&env, &client, &signers.get(0).unwrap(), 1);
    approve(&client, &signers, target_id, 2);
    let third_hash = Bytes::from_slice(&env, b"third");
    let third_id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &third_hash,
        &1_000u64,
    );
    approve(&client, &signers, third_id, 3);
    let ids = soroban_sdk::vec![&env, raise_id, target_id, third_id];
    let hashes = soroban_sdk::vec![&env, raise_hash, target_hash, third_hash];

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::QuorumNotReached))
    );
    assert_eq!(client.get_threshold(), 2);
    assert_eq!(client.get_proposal(&raise_id).status, ProposalStatus::Passed);
    assert_eq!(client.get_proposal(&target_id).status, ProposalStatus::Passed);
    assert_eq!(client.get_proposal(&third_id).status, ProposalStatus::Passed);
}
