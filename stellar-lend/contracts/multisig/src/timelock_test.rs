use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger;
use soroban_sdk::{Address, Bytes, Env, Vec};

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn make_payload_hash(env: &Env, data: &[u8]) -> Bytes {
    Bytes::from_slice(env, data)
}

fn setup_multisig_with_delay(env: &Env, delay: u32) -> (Address, Vec<Address>) {
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(env, &contract_id);

    let s1 = Address::generate(env);
    let s2 = Address::generate(env);
    let s3 = Address::generate(env);
    let mut signers = Vec::new(env);
    signers.push_back(s1);
    signers.push_back(s2);
    signers.push_back(s3);

    client.initialize_with_delay(&signers, &2u32, &delay);
    (contract_id, signers)
}

// ---------------------------------------------------------------------------
// 1. Initialization and query tests
// ---------------------------------------------------------------------------

#[test]
fn test_default_execution_delay_is_zero() {
    let env = make_env();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1);
    signers.push_back(s2);

    client.initialize(&signers, &2u32);
    assert_eq!(client.get_execution_delay(), 0);
}

#[test]
fn test_initialize_with_delay_stores_delay() {
    let env = make_env();
    let (contract_id, _) = setup_multisig_with_delay(&env, 120);
    let client = MultisigContractClient::new(&env, &contract_id);
    assert_eq!(client.get_execution_delay(), 120);
}

#[test]
fn test_initialize_with_delay_exceeding_max_rejected() {
    let env = make_env();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let mut signers = Vec::new(&env);
    signers.push_back(s1);
    signers.push_back(s2);

    let res = client.try_initialize_with_delay(&signers, &2u32, &(MAX_EXECUTION_DELAY_LEDGERS + 1));
    assert_eq!(res, Err(Ok(MultisigError::InvalidDelay)));
}

// ---------------------------------------------------------------------------
// 2. Proposal TTL vs delay interaction
// ---------------------------------------------------------------------------

#[test]
fn test_create_proposal_ttl_must_exceed_delay() {
    let env = make_env();
    let delay = 100u32;
    let (contract_id, signers) = setup_multisig_with_delay(&env, delay);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_payload_hash(&env, b"payload");
    let caller = signers.get(0).unwrap();

    // TTL == delay: rejected
    let res_equal = client.try_create_proposal(
        &caller,
        &ProposalAction::SetThreshold(3),
        &hash,
        &(delay as u64),
    );
    assert_eq!(res_equal, Err(Ok(MultisigError::InvalidTtl)));

    // TTL < delay: rejected
    let res_less = client.try_create_proposal(
        &caller,
        &ProposalAction::SetThreshold(3),
        &hash,
        &50u64,
    );
    assert_eq!(res_less, Err(Ok(MultisigError::InvalidTtl)));

    // TTL > delay: succeeds
    let res_ok = client.try_create_proposal(
        &caller,
        &ProposalAction::SetThreshold(3),
        &hash,
        &101u64,
    );
    assert!(res_ok.is_ok());
}

// ---------------------------------------------------------------------------
// 3. Execution timelock boundaries: D - 1, D, D + 1
// ---------------------------------------------------------------------------

#[test]
fn test_execution_timelock_boundaries() {
    let env = make_env();
    env.ledger().set_sequence_number(1_000);

    let delay = 100u32;
    let (contract_id, signers) = setup_multisig_with_delay(&env, delay);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_payload_hash(&env, b"action_hash");
    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    let id = client.create_proposal(
        &s1,
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    // Before quorum: get_executable_at is None
    assert_eq!(client.get_executable_at(&id), None);

    // Approve by s1
    client.approve_proposal(&s1, &id);
    assert_eq!(client.get_executable_at(&id), None);

    // Advance to ledger 1050 and approve by s2 to reach quorum (2-of-3)
    env.ledger().set_sequence_number(1_050);
    client.approve_proposal(&s2, &id);

    // Now proposal is Passed. passed_at = 1050. Executable at 1050 + 100 = 1150
    assert_eq!(client.get_executable_at(&id), Some(1_150));

    // Boundary 1: D - 1 (ledger 1149). Must fail with TimelockNotElapsed
    env.ledger().set_sequence_number(1_149);
    let res_early = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(res_early, Err(Ok(MultisigError::TimelockNotElapsed)));

    // Boundary 2: D (ledger 1150). Must succeed!
    env.ledger().set_sequence_number(1_150);
    let res_exact = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(res_exact, Ok(Ok(())));

    let executed_prop = client.get_proposal(&id);
    assert_eq!(executed_prop.status, ProposalStatus::Executed);
    assert_eq!(client.get_threshold(), 3);
}

#[test]
fn test_execution_timelock_boundary_plus_one() {
    let env = make_env();
    env.ledger().set_sequence_number(1_000);

    let delay = 100u32;
    let (contract_id, signers) = setup_multisig_with_delay(&env, delay);
    let client = MultisigContractClient::new(&env, &contract_id);

    let hash = make_payload_hash(&env, b"action_hash_plus_one");
    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    let id = client.create_proposal(
        &s1,
        &ProposalAction::SetThreshold(3),
        &hash,
        &500u64,
    );

    // Reach quorum at ledger 1050
    env.ledger().set_sequence_number(1_050);
    client.approve_proposal(&s1, &id);
    client.approve_proposal(&s2, &id);

    // Boundary 3: D + 1 (ledger 1151). Must succeed!
    env.ledger().set_sequence_number(1_151);
    let res_later = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(res_later, Ok(Ok(())));
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
}

// ---------------------------------------------------------------------------
// 4. Batch execution timelock enforcement
// ---------------------------------------------------------------------------

#[test]
fn test_batch_execute_timelock_enforcement_and_atomicity() {
    let env = make_env();
    env.ledger().set_sequence_number(1_000);

    let delay = 100u32;
    let (contract_id, signers) = setup_multisig_with_delay(&env, delay);
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    let hash1 = make_payload_hash(&env, b"batch_action_1");
    let hash2 = make_payload_hash(&env, b"batch_action_2");

    // SetThreshold(2) keeps the live threshold unchanged so id2 still meets
    // quorum when it executes after id1 in the same batch.
    let id1 = client.create_proposal(&s1, &ProposalAction::SetThreshold(2), &hash1, &500u64);
    let id2 = client.create_proposal(&s1, &ProposalAction::SetThreshold(2), &hash2, &500u64);

    // Proposal 1 reaches quorum at ledger 1000 -> executable at 1100
    client.approve_proposal(&s1, &id1);
    client.approve_proposal(&s2, &id1);

    // Proposal 2 reaches quorum at ledger 1020 -> executable at 1120
    env.ledger().set_sequence_number(1_020);
    client.approve_proposal(&s1, &id2);
    client.approve_proposal(&s2, &id2);

    let mut ids = Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);

    let mut hashes = Vec::new(&env);
    hashes.push_back(hash1.clone());
    hashes.push_back(hash2.clone());

    // At ledger 1105: id1 delay has elapsed, but id2 delay has NOT elapsed (< 1120)
    env.ledger().set_sequence_number(1_105);
    let res_early = client.try_batch_execute(&s1, &ids, &hashes);
    assert_eq!(res_early, Err(Ok(MultisigError::TimelockNotElapsed)));

    // Atomicity check: id1 must NOT be executed because the batch failed
    assert_eq!(client.get_proposal(&id1).status, ProposalStatus::Passed);
    assert_eq!(client.get_proposal(&id2).status, ProposalStatus::Passed);

    // At ledger 1120: both delays have elapsed -> batch succeeds!
    env.ledger().set_sequence_number(1_120);
    let res_ok = client.try_batch_execute(&s1, &ids, &hashes);
    assert_eq!(res_ok, Ok(Ok(())));

    assert_eq!(client.get_proposal(&id1).status, ProposalStatus::Executed);
    assert_eq!(client.get_proposal(&id2).status, ProposalStatus::Executed);
}

// ---------------------------------------------------------------------------
// 5. Cancellation during and after delay window
// ---------------------------------------------------------------------------

#[test]
fn test_cancel_during_delay_window_succeeds() {
    let env = make_env();
    env.ledger().set_sequence_number(1_000);

    let delay = 100u32;
    let (contract_id, signers) = setup_multisig_with_delay(&env, delay);
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    let hash = make_payload_hash(&env, b"veto_me");
    let id = client.create_proposal(&s1, &ProposalAction::SetThreshold(3), &hash, &500u64);

    // Pass at ledger 1000
    client.approve_proposal(&s1, &id);
    client.approve_proposal(&s2, &id);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Passed);

    // Cancel at ledger 1050 (during delay window: 1050 < 1100)
    env.ledger().set_sequence_number(1_050);
    client.cancel_proposal(&s1, &id);

    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Cancelled);

    // Once cancelled, cannot execute even after timelock expires
    env.ledger().set_sequence_number(1_150);
    let res = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(res, Err(Ok(MultisigError::AlreadyCancelled)));
}

#[test]
fn test_cancel_after_delay_window_rejected() {
    let env = make_env();
    env.ledger().set_sequence_number(1_000);

    let delay = 100u32;
    let (contract_id, signers) = setup_multisig_with_delay(&env, delay);
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    let hash = make_payload_hash(&env, b"dont_cancel_me");
    let id = client.create_proposal(&s1, &ProposalAction::SetThreshold(3), &hash, &500u64);

    // Pass at ledger 1000 (executable at 1100)
    client.approve_proposal(&s1, &id);
    client.approve_proposal(&s2, &id);

    // Try to cancel at ledger 1100 (delay window has elapsed)
    env.ledger().set_sequence_number(1_100);
    let res = client.try_cancel_proposal(&s1, &id);
    assert_eq!(res, Err(Ok(MultisigError::ProposalNotPassed)));

    // Proposal can still execute!
    let res_exec = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(res_exec, Ok(Ok(())));
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Executed);
}

// ---------------------------------------------------------------------------
// 6. SetExecutionDelay governance and self-bypass guard
// ---------------------------------------------------------------------------

#[test]
fn test_set_execution_delay_governance_and_self_bypass_guard() {
    let env = make_env();
    env.ledger().set_sequence_number(1_000);

    let initial_delay = 100u32;
    let (contract_id, signers) = setup_multisig_with_delay(&env, initial_delay);
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    // Proposal attempting to set execution delay to 0
    let hash = make_payload_hash(&env, b"reduce_delay_to_0");
    let id = client.create_proposal(&s1, &ProposalAction::SetExecutionDelay(0), &hash, &500u64);

    // Pass at ledger 1000
    client.approve_proposal(&s1, &id);
    client.approve_proposal(&s2, &id);

    // Attacker tries to execute immediately at ledger 1001 (attempting to bypass delay)
    env.ledger().set_sequence_number(1_001);
    let res_bypass = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(
        res_bypass,
        Err(Ok(MultisigError::TimelockNotElapsed)),
        "SetExecutionDelay proposals cannot bypass the currently active timelock"
    );

    // Advance to ledger 1100 (timelock elapsed)
    env.ledger().set_sequence_number(1_100);
    let res_exec = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(res_exec, Ok(Ok(())));

    // Delay is now updated to 0
    assert_eq!(client.get_execution_delay(), 0);

    // Subsequent proposals with delay=0 can execute immediately
    let hash2 = make_payload_hash(&env, b"action_with_zero_delay");
    let id2 = client.create_proposal(&s1, &ProposalAction::SetThreshold(3), &hash2, &500u64);
    client.approve_proposal(&s1, &id2);
    client.approve_proposal(&s2, &id2);

    let res2 = client.try_execute_proposal(&s1, &id2, &hash2);
    assert_eq!(res2, Ok(Ok(())));
    assert_eq!(client.get_proposal(&id2).status, ProposalStatus::Executed);
}

#[test]
fn test_set_execution_delay_exceeding_max_rejected_on_execution() {
    let env = make_env();
    env.ledger().set_sequence_number(1_000);

    let (contract_id, signers) = setup_multisig_with_delay(&env, 0);
    let client = MultisigContractClient::new(&env, &contract_id);

    let s1 = signers.get(0).unwrap();
    let s2 = signers.get(1).unwrap();

    let invalid_delay = MAX_EXECUTION_DELAY_LEDGERS + 1;
    let hash = make_payload_hash(&env, b"invalid_delay_proposal");
    let id = client.create_proposal(&s1, &ProposalAction::SetExecutionDelay(invalid_delay), &hash, &500u64);

    client.approve_proposal(&s1, &id);
    client.approve_proposal(&s2, &id);

    let res = client.try_execute_proposal(&s1, &id, &hash);
    assert_eq!(res, Err(Ok(MultisigError::InvalidDelay)));
}
