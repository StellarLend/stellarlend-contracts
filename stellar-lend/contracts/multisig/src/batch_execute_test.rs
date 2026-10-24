use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Events;
use soroban_sdk::testutils::Ledger;
use soroban_sdk::{Address, Bytes, Env, Vec};

// ---------------------------------------------------------------------------
// Test helpers
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

/// Create a passed proposal with the given action and hash.
fn create_passed_proposal(
    _env: &Env,
    client: &MultisigContractClient,
    signers: &Vec<Address>,
    action: &ProposalAction,
    payload_hash: &Bytes,
) -> u64 {
    let id = client.create_proposal(&signers.get(0).unwrap(), action, payload_hash, &500u64);
    client.approve_proposal(&signers.get(0).unwrap(), &id);
    client.approve_proposal(&signers.get(1).unwrap(), &id);
    id
}

// ---------------------------------------------------------------------------
// Happy path — all proposals eligible
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_all_eligible_success() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"hash_a");
    let hash_b = make_bytes(&env, b"hash_b");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );
    let id_b = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(4),
        &hash_b,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a.clone());
    hashes.push_back(hash_b.clone());

    client.batch_execute(&signers.get(0).unwrap(), &ids, &hashes);

    let p_a = client.get_proposal(&id_a);
    assert_eq!(p_a.status, ProposalStatus::Executed);

    let p_b = client.get_proposal(&id_b);
    assert_eq!(p_b.status, ProposalStatus::Executed);

    assert_eq!(client.get_threshold(), 4);
}

#[test]
fn test_batch_execute_mixed_actions_success() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"hash_a");
    let hash_b = make_bytes(&env, b"hash_b");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );

    // The batch executes `id_a` (SetThreshold(3)) before `id_b`, and the
    // signer-shrink guard checks the new signer count against the *live*
    // threshold at execution time -- so `id_b`'s new set must have at least
    // 3 members here, not 2, or it would be (correctly) rejected as a
    // would-be-bricking shrink.
    let new_s1 = Address::generate(&env);
    let new_s2 = Address::generate(&env);
    let new_s3 = Address::generate(&env);
    let mut new_signers = Vec::new(&env);
    new_signers.push_back(new_s1.clone());
    new_signers.push_back(new_s2.clone());
    new_signers.push_back(new_s3.clone());

    let id_b = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::RotateSigners(new_signers.clone()),
        &hash_b,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(hash_b);

    client.batch_execute(&signers.get(0).unwrap(), &ids, &hashes);

    assert_eq!(client.get_threshold(), 3);

    let stored = client.get_signers();
    assert!(!stored.contains(signers.get(0).unwrap()));
    assert!(stored.contains(&new_s1));
    assert!(stored.contains(&new_s2));
}

// ---------------------------------------------------------------------------
// One ineligible — whole batch rejected
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_one_not_passed_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"h1");
    let hash_b = make_bytes(&env, b"h2");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );

    let id_b = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(4),
        &hash_b,
        &500u64,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(hash_b);

    // `id_b` never received any approvals, so it's still `Active` --
    // `batch_execute` (like `execute_proposal`) reports that specific case
    // as `QuorumNotReached` rather than the generic `ProposalNotPassed`.
    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::QuorumNotReached))
    );
}

#[test]
fn test_batch_execute_one_expired_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"h1");
    let hash_b = make_bytes(&env, b"h2");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );

    let id_b = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(4),
        &hash_b,
        &1u64,
    );
    client.approve_proposal(&signers.get(0).unwrap(), &id_b);
    client.approve_proposal(&signers.get(1).unwrap(), &id_b);

    let current = env.ledger().sequence();
    env.ledger().set_sequence_number(current + 2);

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(hash_b);

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::ProposalExpired))
    );
}

#[test]
fn test_batch_execute_one_already_executed_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"h1");
    let hash_b = make_bytes(&env, b"h2");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );
    let id_b = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(4),
        &hash_b,
    );

    client.execute_proposal(&signers.get(0).unwrap(), &id_b, &hash_b);

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(hash_b);

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::AlreadyExecuted))
    );
}

// The Cancelled-proposal guard in batch_execute is covered by the dedicated
// cancel_proposal_test module, which exercises the full cancel → batch-reject
// path end-to-end now that cancel_proposal correctly persists the status.

// ---------------------------------------------------------------------------
// Duplicate IDs
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_duplicate_id_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"dup_hash");
    let id = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id);
    ids.push_back(id);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash.clone());
    hashes.push_back(hash);

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::DuplicateProposalId))
    );
}

// ---------------------------------------------------------------------------
// Empty batch
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_empty_batch_succeeds() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let ids: Vec<u64> = Vec::new(&env);
    let hashes: Vec<Bytes> = Vec::new(&env);

    client.batch_execute(&signers.get(0).unwrap(), &ids, &hashes);
}

// ---------------------------------------------------------------------------
// Batch size exceeded
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_size_exceeded_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"x");
    let mut ids = Vec::new(&env);
    let mut hashes = Vec::new(&env);

    for _ in 0..=MAX_BATCH_SIZE {
        ids.push_back(999u64);
        hashes.push_back(hash.clone());
    }

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::BatchSizeExceeded))
    );
}

// ---------------------------------------------------------------------------
// Payload hash mismatch
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_payload_hash_mismatch_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"original");
    let hash_b = make_bytes(&env, b"different");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );
    let id_b = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(4),
        &hash_b,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let wrong_hash = make_bytes(&env, b"wrong");
    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(wrong_hash);

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::PayloadHashMismatch))
    );
}

// ---------------------------------------------------------------------------
// Payload hash vec length mismatch
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_hash_count_mismatch_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"h");
    let id = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id);

    let hashes: Vec<Bytes> = Vec::new(&env);

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::PayloadHashMismatch))
    );
}

// ---------------------------------------------------------------------------
// Action dispatch failure — whole batch rolled back
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_dispatch_failure_rolls_back() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"h1");
    let hash_b = make_bytes(&env, b"h2");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );
    let id_b = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(0),
        &hash_b,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(hash_b);

    assert_eq!(
        client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes),
        Err(Ok(MultisigError::InvalidThreshold))
    );
}

// ---------------------------------------------------------------------------
// Non-signer caller rejected
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_non_signer_rejected() {
    let env = make_env();
    let (contract_id, _signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let outsider = Address::generate(&env);
    let ids: Vec<u64> = Vec::new(&env);
    let hashes: Vec<Bytes> = Vec::new(&env);

    assert_eq!(
        client.try_batch_execute(&outsider, &ids, &hashes),
        Err(Ok(MultisigError::Unauthorized))
    );
}

// ---------------------------------------------------------------------------
// Event emission
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_emits_event() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"ha");
    let hash_b = make_bytes(&env, b"hb");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );
    let id_b = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(4),
        &hash_b,
    );

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(hash_b);

    client.batch_execute(&signers.get(0).unwrap(), &ids, &hashes);

    let contract_events = env.events().all();
    let filtered = contract_events.filter_by_contract(&contract_id);
    // At least one event (the BatchExecuted) should be present
    assert!(
        !filtered.events().is_empty(),
        "at least one event must have been emitted for the contract"
    );
}

// ---------------------------------------------------------------------------
// All-or-nothing: proposal statuses unchanged on failure
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_atomicity_on_validation_failure() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash_a = make_bytes(&env, b"h1");
    let hash_b = make_bytes(&env, b"h2");

    let id_a = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(3),
        &hash_a,
    );
    let id_b = create_passed_proposal(
        &env,
        &client,
        &signers,
        &ProposalAction::SetThreshold(4),
        &hash_b,
    );

    client.execute_proposal(&signers.get(0).unwrap(), &id_b, &hash_b);

    let mut ids = Vec::new(&env);
    ids.push_back(id_a);
    ids.push_back(id_b);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash_a);
    hashes.push_back(hash_b);

    let result = client.try_batch_execute(&signers.get(0).unwrap(), &ids, &hashes);
    assert!(result.is_err());

    let p_a = client.get_proposal(&id_a);
    assert_eq!(
        p_a.status,
        ProposalStatus::Passed,
        "id_a must remain Passed after failed batch"
    );

    let p_b = client.get_proposal(&id_b);
    assert_eq!(
        p_b.status,
        ProposalStatus::Executed,
        "id_b must remain Executed after failed batch"
    );
}
