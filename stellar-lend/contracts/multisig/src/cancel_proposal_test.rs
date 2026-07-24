use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Bytes, Env, Vec};

// ---------------------------------------------------------------------------
// Helpers (mirror the pattern used across the test suite)
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_bytes(env: &Env, data: &[u8]) -> Bytes {
    Bytes::from_slice(env, data)
}

/// Spin up a 3-of-2 multisig and return (contract_id, signers).
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

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

/// After a successful `cancel_proposal` call, `get_proposal` must return a
/// proposal whose status is `ProposalStatus::Cancelled`.
#[test]
fn test_cancel_proposal_status_is_cancelled() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"payload_hash");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    // Proposal should be Active before cancellation.
    let before = client.get_proposal(&id);
    assert_eq!(before.status, ProposalStatus::Active);

    client.cancel_proposal(&signers.get(0).unwrap(), &id);

    // Proposal must now be Cancelled in persistent storage.
    let after = client.get_proposal(&id);
    assert_eq!(
        after.status,
        ProposalStatus::Cancelled,
        "proposal status must be Cancelled after cancel_proposal"
    );
}

/// Any registered signer (not just the proposer) can cancel an active proposal.
#[test]
fn test_cancel_proposal_by_non_proposer_signer() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"hash_x");
    // s1 creates the proposal.
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    // s2 (not the proposer) cancels it.
    client.cancel_proposal(&signers.get(1).unwrap(), &id);

    let p = client.get_proposal(&id);
    assert_eq!(
        p.status,
        ProposalStatus::Cancelled,
        "non-proposer signer must be able to cancel an active proposal"
    );
}

// ---------------------------------------------------------------------------
// Idempotency / double-cancel guard
// ---------------------------------------------------------------------------

/// Attempting to cancel an already-cancelled proposal must panic.
#[test]
#[should_panic(expected = "ProposalNotPassed")]
fn test_cancel_already_cancelled_panics() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"hash_y");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    client.cancel_proposal(&signers.get(0).unwrap(), &id);
    // Second call must panic because status is no longer Active.
    client.cancel_proposal(&signers.get(0).unwrap(), &id);
}

// ---------------------------------------------------------------------------
// Interaction with execute_proposal
// ---------------------------------------------------------------------------

/// A cancelled proposal must be rejected by `execute_proposal`.
/// The proposal is cancelled while still Active (before quorum), so
/// cancel_proposal succeeds, and a subsequent execute_proposal attempt
/// panics with AlreadyCancelled.
#[test]
#[should_panic(expected = "AlreadyCancelled")]
fn test_execute_cancelled_proposal_panics() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"hash_z");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    // Cancel while still Active (before quorum is reached).
    client.cancel_proposal(&signers.get(0).unwrap(), &id);

    // execute_proposal must reject a Cancelled proposal.
    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash);
}

// ---------------------------------------------------------------------------
// Access control
// ---------------------------------------------------------------------------

/// A caller who is not a registered signer must not be able to cancel.
#[test]
#[should_panic(expected = "Unauthorized")]
fn test_cancel_proposal_non_signer_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"hash_w");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    let outsider = Address::generate(&env);
    client.cancel_proposal(&outsider, &id);
}

// ---------------------------------------------------------------------------
// Interaction with batch_execute
// ---------------------------------------------------------------------------

/// A cancelled proposal must be rejected by `batch_execute`.
/// The proposal is cancelled while still Active so cancel_proposal succeeds.
/// batch_execute then panics with AlreadyCancelled.
#[test]
#[should_panic(expected = "AlreadyCancelled")]
fn test_batch_execute_cancelled_proposal_rejected() {
    let env = make_env();
    let (contract_id, signers) = setup_multisig(&env);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_bytes(&env, b"hash_b");
    let id = client.create_proposal(
        &signers.get(0).unwrap(),
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    // Cancel while still Active (before quorum).
    client.cancel_proposal(&signers.get(0).unwrap(), &id);

    let mut ids = Vec::new(&env);
    ids.push_back(id);
    let mut hashes = Vec::new(&env);
    hashes.push_back(hash);

    client.batch_execute(&signers.get(0).unwrap(), &ids, &hashes);
}
