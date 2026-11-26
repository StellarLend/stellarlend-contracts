use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Bytes, Env, Vec};

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_with_delay(
    env: &Env,
    threshold: u32,
    signer_count: u32,
    delay: u32,
) -> (Address, Vec<Address>) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);
    let mut signers = Vec::new(env);
    for _ in 0..signer_count {
        signers.push_back(Address::generate(env));
    }
    client.initialize_with_delay(&signers, &threshold, &delay);
    (contract_id, signers)
}

fn create_proposal(
    env: &Env,
    client: &MultisigContractClient,
    signers: &Vec<Address>,
    action: &ProposalAction,
    tag: &[u8],
    ttl: u64,
) -> (u64, Bytes) {
    let hash = Bytes::from_slice(env, tag);
    let id = client.create_proposal(&signers.get(0).unwrap(), action, &hash, &ttl);
    (id, hash)
}

#[test]
fn test_single_signer_propose_approve_execute() {
    let env = make_env();
    let (contract_id, signers) = setup_with_delay(&env, 1, 2, 100);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(1),
        b"single-signer",
        500,
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    let executable_at = client.get_executable_at(&id).unwrap();
    env.ledger().set_sequence_number(executable_at as u32);

    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Ok(Ok(()))
    );
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
}

#[test]
fn test_two_of_two_propose_approve_execute() {
    let env = make_env();
    let (contract_id, signers) = setup_with_delay(&env, 2, 2, 100);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(2),
        b"two-of-two",
        500,
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::QuorumNotReached))
    );

    client.approve_proposal(&signers.get(1).unwrap(), &id);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
    let executable_at = client.get_executable_at(&id).unwrap();
    env.ledger().set_sequence_number(executable_at as u32);

    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Ok(Ok(()))
    );
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
}

#[test]
fn test_execute_before_eta_fails() {
    let env = make_env();
    let (contract_id, signers) = setup_with_delay(&env, 1, 2, 100);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(1),
        b"before-eta",
        500,
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::TimelockNotElapsed))
    );
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);
}

#[test]
fn test_double_execute_fails() {
    let env = make_env();
    let (contract_id, signers) = setup_with_delay(&env, 1, 2, 100);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(1),
        b"double-execute",
        500,
    );

    client.approve_proposal(&signers.get(0).unwrap(), &id);
    let executable_at = client.get_executable_at(&id).unwrap();
    env.ledger().set_sequence_number(executable_at as u32);
    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash);

    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::AlreadyExecuted))
    );
}

#[test]
fn test_execute_expired_proposal_fails() {
    let env = make_env();
    let (contract_id, signers) = setup_with_delay(&env, 1, 2, 10);
    let client = MultisigContractClient::new(&env, &contract_id);
    let (id, hash) = create_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(1),
        b"expired",
        20,
    );
    let expires_at = client.get_proposal(&id).expires_at;
    env.ledger().set_sequence_number(expires_at as u32 + 1);

    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::ProposalExpired))
    );
    assert!(!client.is_nonce_consumed(&client.get_proposal_nonce(&id)));
}
