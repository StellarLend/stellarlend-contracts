use super::*;
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env, Vec};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_bytes(env: &Env, data: &[u8]) -> Bytes {
    Bytes::from_slice(env, data)
}

fn setup_multisig(env: &Env) -> (Address, Vec<Address>) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);

    let s1 = Address::generate(env);
    let s2 = Address::generate(env);
    let s3 = Address::generate(env);
    let mut signers = Vec::new(env);
    signers.push_back(s1.clone());
    signers.push_back(s2.clone());
    signers.push_back(s3.clone());

    client.initialize(&signers, &2u32);
    (contract_id, signers)
}

// Approve proposal by `n` signers from the list.
fn approve_n(client: &MultisigContractClient, signers: &Vec<Address>, id: u64, n: usize) {
    for i in 0..n {
        client.approve_proposal(&signers.get(i as u32).unwrap(), &id);
    }
}

// ---------------------------------------------------------------------------
// Initialization tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_sets_threshold_and_signers() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    assert_eq!(client.get_threshold(), 2u32);
    let stored = client.get_signers();
    assert_eq!(stored.len(), 3);
    assert!(stored.contains(&signers.get(0).unwrap()));
}

#[test]
fn test_initialize_rejects_zero_threshold() {
    let env = make_env();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1);
    assert_eq!(client.try_initialize(&signers, &0u32), Err(Ok(MultisigError::InvalidThreshold)));
}

#[test]
fn test_initialize_rejects_threshold_exceeding_signers() {
    let env = make_env();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1);
    // threshold 2 > 1 signer
    assert_eq!(client.try_initialize(&signers, &2u32), Err(Ok(MultisigError::InvalidThreshold)));
}

// ---------------------------------------------------------------------------
// create_proposal tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_proposal_returns_incrementing_ids() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"hash_a");
    let id0 = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &100u64,
    ).unwrap();
    let id1 = client.create_proposal(
        &signers.get(1).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &100u64,
    ).unwrap();
    assert_eq!(id1, id0 + 1);
}

#[test]
fn test_create_proposal_rejects_non_signer() {
    let env = make_env();
    let (contract_id, _) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let outsider = Address::generate(&env);
    let hash = make_bytes(&env, b"hash");
    assert_eq!(
        client.try_create_proposal(
            &outsider,
            &ProposalAction::SetThreshold(1),
            &hash,
            &100u64,
        ),
        Err(Ok(MultisigError::Unauthorized))
    );
}

// ---------------------------------------------------------------------------
// approve_proposal tests
// ---------------------------------------------------------------------------

#[test]
fn test_approve_proposal_transitions_to_passed_at_quorum() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"h1");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &200u64,
    );

    // One approval: still Active
    client.approve_proposal(&signers.get(0).unwrap(), &id).unwrap();
    let p = client.get_proposal(&id).unwrap();
    assert_eq!(p.status, ProposalStatus::Active);

    // Second approval: reaches threshold of 2 → Passed
    client.approve_proposal(&signers.get(1).unwrap(), &id).unwrap();
    let p2 = client.get_proposal(&id).unwrap();
    assert_eq!(p2.status, ProposalStatus::Passed);
}

#[test]
fn test_approve_proposal_rejects_double_approval() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"h2");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &100u64,
    ).unwrap();
    client.approve_proposal(&signers.get(0).unwrap(), &id).unwrap();
    // Same signer approves again
    assert_eq!(
        client.try_approve_proposal(&signers.get(0).unwrap(), &id),
        Err(Ok(MultisigError::AlreadyApproved))
    );
}

// ---------------------------------------------------------------------------
// execute_proposal — SetThreshold tests
// ---------------------------------------------------------------------------

#[test]
fn test_execute_set_threshold_updates_threshold() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"threshold_hash");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    ).unwrap();
    approve_n(&client, &signers, id, 2);

    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash).unwrap();

    assert_eq!(client.get_threshold(), 3u32);
    let p = client.get_proposal(&id).unwrap();
    assert_eq!(p.status, ProposalStatus::Executed);
}

// ---------------------------------------------------------------------------
// execute_proposal — RotateSigners tests
// ---------------------------------------------------------------------------

#[test]
fn test_execute_rotate_signers_replaces_signer_set() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let new_s1 = Address::generate(&env);
    let new_s2 = Address::generate(&env);
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(new_s1.clone());
    new_signers.push_back(new_s2.clone());

    let hash = make_bytes(&env, b"rotate_hash");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::RotateSigners(new_signers.clone()),
        &hash,
        &500u64,
    ).unwrap();
    approve_n(&client, &signers, id, 2);

    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash);

    let stored = client.get_signers();
    assert!(stored.contains(&new_s1));
    assert!(stored.contains(&new_s2));
    assert!(!stored.contains(&signers.get(0).unwrap()));
}

// ---------------------------------------------------------------------------
// execute_proposal — rejection guards
// ---------------------------------------------------------------------------

#[test]
fn test_execute_before_quorum_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"qh");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(1),
        &hash,
        &100u64,
    ).unwrap();
    // Only one approval — threshold is 2
    client.approve_proposal(&signers.get(0).unwrap(), &id).unwrap();
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::QuorumNotReached))
    );
}

#[test]
fn test_execute_double_execution_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"dex_hash");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &500u64,
    ).unwrap();
    approve_n(&client, &signers, id, 2);
    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash).unwrap();
    // Second execution attempt should return AlreadyExecuted
    assert_eq!(
        client.try_execute_proposal(&signers.get(1).unwrap(), &id, &hash),
        Err(Ok(MultisigError::AlreadyExecuted))
    );
}

#[test]
fn test_execute_payload_hash_mismatch_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let original_hash = make_bytes(&env, b"original");
    let swapped_hash = make_bytes(&env, b"swapped_payload");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &original_hash,
        &500u64,
    ).unwrap();
    approve_n(&client, &signers, id, 2);
    // Present a different hash at execution — must be rejected
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &swapped_hash),
        Err(Ok(MultisigError::PayloadHashMismatch))
    );
}

#[test]
fn test_execute_cancelled_proposal_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"cancel_hash");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &500u64,
    ).unwrap();
    client.cancel_proposal(&signers.get(0).unwrap(), &id).unwrap();
    // Attempt to execute a cancelled proposal
    assert_eq!(
        client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash),
        Err(Ok(MultisigError::AlreadyCancelled))
    );
}

// ---------------------------------------------------------------------------
// cancel_proposal tests
// ---------------------------------------------------------------------------

#[test]
fn test_cancel_proposal_sets_cancelled_status() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"ch");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &200u64,
    ).unwrap();
    client.cancel_proposal(&signers.get(0).unwrap(), &id).unwrap();
    let p = client.get_proposal(&id).unwrap();
    assert_eq!(p.status, ProposalStatus::Cancelled);
}

#[test]
fn test_cancel_passed_proposal_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"cp2");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(2),
        &hash,
        &300u64,
    ).unwrap();
    approve_n(&client, &signers, id, 2);
    // Cannot cancel a Passed proposal
    assert_eq!(
        client.try_cancel_proposal(&signers.get(0).unwrap(), &id),
        Err(Ok(MultisigError::ProposalNotPassed))
    );
}

// ---------------------------------------------------------------------------
// get_proposal view tests
// ---------------------------------------------------------------------------

#[test]
fn test_get_proposal_nonexistent_panics() {
    let env = make_env();
    let (contract_id, _) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);
    assert_eq!(client.try_get_proposal(&9999u64), Err(Ok(MultisigError::ProposalNotFound)));
}
