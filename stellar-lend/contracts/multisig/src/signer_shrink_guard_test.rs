/// Signer-shrink bricking-prevention tests for the multisig contract.
///
/// # Safety invariant under test
///
/// Applying a new signer set whose size is smaller than the current threshold
/// would permanently brick the multisig because quorum could never be reached.
/// These tests verify the documented coupling between signer-set size and the
/// live threshold inside `dispatch_action` when a `RotateSigners` proposal is
/// executed.
///
/// # Coverage
///
/// 1. Shrink below threshold — `RotateSigners` execution returns false (bricking prevented).
/// 2. Shrink to exactly threshold size — succeeds (tight but valid quorum).
/// 3. Live threshold getter is unchanged after a rejected rotate.
/// 4. Reducing the threshold first (via `SetThreshold`) enables a subsequent shrink.
use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Bytes, Env, Vec};

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

/// Spin up a multisig with `signer_count` signers and the given `threshold`.
fn setup(threshold: u32, signer_count: usize) -> (Env, Address, Vec<Address>) {
    let env = make_env();
    let contract_id = env.register(MultisigContract, ());
    let client = MultisigContractClient::new(&env, &contract_id);

    let mut signers = Vec::new(&env);
    for _ in 0..signer_count {
        signers.push_back(Address::generate(&env));
    }
    client.initialize(&signers, &threshold);

    (env, contract_id, signers)
}

/// Build a `Vec<Address>` of `n` freshly generated addresses.
fn make_signers(env: &Env, n: usize) -> Vec<Address> {
    let mut v = Vec::new(env);
    for _ in 0..n {
        v.push_back(Address::generate(env));
    }
    v
}

/// Create a proposal, gather enough approvals to pass it, then return the id.
fn create_and_pass(
    env: &Env,
    contract_id: &Address,
    signers: &Vec<Address>,
    action: ProposalAction,
    hash: &Bytes,
) -> u64 {
    let client = MultisigContractClient::new(env, contract_id);
    let threshold = client.get_threshold() as usize;

    let id = client.create_proposal(&signers.get(0).unwrap(), &action, hash, &500u64);
    for i in 0..threshold {
        client.approve_proposal(&signers.get(i as u32).unwrap(), &id);
    }
    id
}

// ---------------------------------------------------------------------------
// Test 1: shrink below threshold is rejected
// ---------------------------------------------------------------------------

/// Executing a `RotateSigners` proposal whose new set is smaller than the
/// current threshold must fail — the action returns
/// `Err(MultisigError::InvalidSigners)` from `dispatch_action`, propagated by
/// `execute_proposal`.
#[test]
fn test_shrink_below_threshold_is_rejected() {
    // threshold = 3, initial signers = 5; attempting to shrink to 2 (< 3).
    let (env, contract_id, signers) = setup(3, 5);
    let client = MultisigContractClient::new(&env, &contract_id);
    assert_eq!(client.get_threshold(), 3);

    let tiny_set = make_signers(&env, 2); // 2 < threshold 3
    let hash = make_bytes(&env, b"shrink_below_hash");
    let id = create_and_pass(
        &env,
        &contract_id,
        &signers,
        ProposalAction::RotateSigners(tiny_set),
        &hash,
    );

    // execute_proposal must fail because the guard rejects the shrink.
    // Typed contract errors surface as `Error(Contract, #N)` without the
    // variant name in the panic message in this environment, so assert via
    // `try_` instead of `should_panic`.
    let res = client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash);
    assert!(
        matches!(res, Err(Ok(MultisigError::InvalidSigners))),
        "expected InvalidSigners, got {:?}",
        res
    );
}

// ---------------------------------------------------------------------------
// Test 2: shrink to exactly threshold size succeeds
// ---------------------------------------------------------------------------

/// Shrinking to exactly the threshold size is the tightest valid quorum
/// and must be accepted.
#[test]
fn test_shrink_to_exactly_threshold_succeeds() {
    // threshold = 3, initial signers = 5; shrink to 3 (== threshold).
    let (env, contract_id, signers) = setup(3, 5);
    let client = MultisigContractClient::new(&env, &contract_id);

    let exact_set = make_signers(&env, 3); // 3 == threshold
    let hash = make_bytes(&env, b"shrink_exact_hash");
    let id = create_and_pass(
        &env,
        &contract_id,
        &signers,
        ProposalAction::RotateSigners(exact_set.clone()),
        &hash,
    );

    client.execute_proposal(&signers.get(0).unwrap(), &id, &hash);

    let stored = client.get_signers();
    assert_eq!(
        stored.len(),
        3,
        "signer set must have exactly 3 members after shrink-to-threshold"
    );
    // Original signers must have been replaced.
    assert!(
        !stored.contains(signers.get(0).unwrap()),
        "old signers must not appear in the rotated set"
    );
}

// ---------------------------------------------------------------------------
// Test 3: threshold is unchanged after a rejected rotate
// ---------------------------------------------------------------------------

/// A rejected `RotateSigners` execution must leave the live threshold intact.
#[test]
fn test_threshold_unchanged_after_rejected_rotate() {
    // threshold = 3, initial signers = 4; try to shrink to 1 (< 3).
    let (env, contract_id, signers) = setup(3, 4);
    let client = MultisigContractClient::new(&env, &contract_id);
    let threshold_before = client.get_threshold();

    let tiny_set = make_signers(&env, 1); // 1 < threshold 3
    let hash = make_bytes(&env, b"unchanged_thresh_hash");
    let id = create_and_pass(
        &env,
        &contract_id,
        &signers,
        ProposalAction::RotateSigners(tiny_set),
        &hash,
    );

    // The execute attempt will panic; catch it so we can assert threshold afterward.
    let result = client.try_execute_proposal(&signers.get(0).unwrap(), &id, &hash);
    assert!(
        result.is_err(),
        "executing a shrink-below-threshold must fail"
    );

    assert_eq!(
        client.get_threshold(),
        threshold_before,
        "threshold must be unchanged after a rejected shrink"
    );
}

// ---------------------------------------------------------------------------
// Test 4: threshold reduction first enables a subsequent shrink
// ---------------------------------------------------------------------------

/// Reducing the threshold (via a `SetThreshold` proposal) before rotating
/// the signer set makes the previously invalid shrink valid.
#[test]
fn test_threshold_reduction_enables_subsequent_shrink() {
    // threshold = 3, signers = 5.  Reduce threshold to 2 first, then shrink to 2.
    let (env, contract_id, signers) = setup(3, 5);
    let client = MultisigContractClient::new(&env, &contract_id);

    // Step 1 — reduce threshold from 3 → 2 via a SetThreshold proposal.
    let thresh_hash = make_bytes(&env, b"set_threshold_hash");
    let thresh_id = create_and_pass(
        &env,
        &contract_id,
        &signers,
        ProposalAction::SetThreshold(2),
        &thresh_hash,
    );
    client.execute_proposal(&signers.get(0).unwrap(), &thresh_id, &thresh_hash);
    assert_eq!(
        client.get_threshold(),
        2,
        "threshold must be 2 after SetThreshold"
    );

    // Step 2 — now shrink the signer set to 2 (== new threshold).
    // Need to re-approve with the original signers since the signer set hasn't changed yet.
    let two_signers = make_signers(&env, 2);
    let rotate_hash = make_bytes(&env, b"rotate_to_two_hash");
    let rotate_id = create_and_pass(
        &env,
        &contract_id,
        &signers,
        ProposalAction::RotateSigners(two_signers.clone()),
        &rotate_hash,
    );
    client.execute_proposal(&signers.get(0).unwrap(), &rotate_id, &rotate_hash);

    assert_eq!(
        client.get_signers().len(),
        2,
        "signer set must have 2 members after valid shrink post threshold-reduction"
    );
}
