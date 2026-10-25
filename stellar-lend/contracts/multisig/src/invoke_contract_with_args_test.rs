//! End-to-end test: MultisigContract executes an `InvokeContract` proposal
//! that carries non-empty arguments and successfully invokes the target
//! function with those arguments.
//!
//! # Structure
//!
//! * **`TargetContract`** – a minimal Soroban contract with a single
//!   `set_value(v: u32)` entrypoint that stores the supplied value.  Its
//!   `get_value()` view lets us assert the call happened with the right arg.
//!
//! * **Happy-path test** – `test_invoke_contract_with_args_end_to_end` walks
//!   through the full multisig lifecycle:
//!     1. Deploy and initialise the multisig (threshold = 2, three signers).
//!     2. Deploy the target contract.
//!     3. Create an `InvokeContract` proposal with `args = [42u32]`.
//!     4. Two signers approve (reaching the threshold).
//!     5. Execute the proposal.
//!     6. Assert the proposal is marked `Executed` and the target value is 42.
//!
//! * **Wrong-args guard test** – `test_invoke_contract_with_wrong_args_panics`
//!   checks that if the proposal is created with args that do NOT satisfy the
//!   target function's expected type the execution panics (type-mismatch at the
//!   cross-contract call boundary).  This validates that the args field is
//!   genuinely forwarded rather than silently ignored.

use crate::{MultisigContract, MultisigContractClient, ProposalAction, ProposalStatus};
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Bytes, Env, IntoVal, Symbol, Val, Vec,
};

// ---------------------------------------------------------------------------
// Target contract
// ---------------------------------------------------------------------------

/// Minimal contract that accepts a single `set_value` call with one `u32`
/// argument and persists it so tests can inspect the result.
#[contract]
pub struct TargetContract;

#[contracttype]
pub enum TargetKey {
    Value,
}

#[contractimpl]
impl TargetContract {
    /// Store `v` under the `Value` key.
    pub fn set_value(env: Env, v: u32) {
        env.storage().persistent().set(&TargetKey::Value, &v);
    }

    /// Return the stored value, defaulting to 0.
    pub fn get_value(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&TargetKey::Value)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Deploy and initialise a 3-signer multisig with threshold = 2.
///
/// Returns `(multisig_contract_id, [s1, s2, s3])`.
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

/// Build a dummy payload hash from a byte slice.
fn make_hash(env: &Env, data: &[u8]) -> Bytes {
    Bytes::from_slice(env, data)
}

// ---------------------------------------------------------------------------
// Happy-path end-to-end test
// ---------------------------------------------------------------------------

/// Full lifecycle: propose → approve × 2 → execute → target value is 42.
#[test]
fn test_invoke_contract_with_args_end_to_end() {
    let env = make_env();

    // ----- Set up multisig -------------------------------------------------
    let (multisig_id, signers) = setup_multisig(&env);
    let multisig = MultisigContractClient::new(&env, &multisig_id);

    // ----- Deploy target contract ------------------------------------------
    let target_id = env.register(TargetContract, ());
    let target = TargetContractClient::new(&env, &target_id);

    // Sanity: target starts at default (0).
    assert_eq!(target.get_value(), 0u32, "initial value must be 0");

    // ----- Build the InvokeContract action ---------------------------------
    let fn_symbol: Symbol = Symbol::new(&env, "set_value");

    // Build the args Vec<Val> with a single u32 argument (42).
    let mut args: Vec<Val> = Vec::new(&env);
    args.push_back(42u32.into_val(&env));

    let action = ProposalAction::InvokeContract {
        contract: target_id.clone(),
        fn_symbol: fn_symbol.clone(),
        args: args.clone(),
    };

    // Arbitrary payload hash (in production this would be a real SHA-256 of
    // the encoded action; here we use a fixed constant for simplicity).
    let payload_hash = make_hash(&env, b"invoke_42_payload_hash");

    // ----- Create the proposal ---------------------------------------------
    let id = multisig.create_proposal(
        &signers.get(0).unwrap(),
        &action,
        &payload_hash,
        &500u64,
    );

    // Verify it exists and is Active.
    let p = multisig.get_proposal(&id);
    assert_eq!(p.status, ProposalStatus::Active);

    // ----- Two signers approve (threshold = 2) -----------------------------
    multisig.approve_proposal(&signers.get(0).unwrap(), &id);
    // After the first approval the proposal is still Active (1 < 2).
    assert_eq!(multisig.get_proposal(&id).status, ProposalStatus::Active);

    multisig.approve_proposal(&signers.get(1).unwrap(), &id);
    // After the second approval it transitions to Passed.
    assert_eq!(multisig.get_proposal(&id).status, ProposalStatus::Passed);

    // ----- Execute ---------------------------------------------------------
    multisig.execute_proposal(&signers.get(0).unwrap(), &id, &payload_hash);

    // The proposal must be marked Executed.
    assert_eq!(
        multisig.get_proposal(&id).status,
        ProposalStatus::Executed,
        "proposal must be Executed after execute_proposal"
    );

    // The target must have been called with 42, not 0 or any other value.
    assert_eq!(
        target.get_value(),
        42u32,
        "target value must be 42 — args were forwarded correctly"
    );
}

// ---------------------------------------------------------------------------
// Negative: wrong args type causes execution to panic
// ---------------------------------------------------------------------------

/// If the action is created with an argument that does not match the target
/// function's parameter type (a `Bytes` blob instead of a `u32`), the
/// cross-contract invocation must panic — proving the args field is genuinely
/// forwarded and not silently dropped.
#[test]
#[should_panic]
fn test_invoke_contract_with_wrong_args_panics() {
    let env = make_env();

    let (multisig_id, signers) = setup_multisig(&env);
    let multisig = MultisigContractClient::new(&env, &multisig_id);

    let target_id = env.register(TargetContract, ());

    let fn_symbol: Symbol = Symbol::new(&env, "set_value");

    // Wrong type: pass a Bytes blob where a u32 is expected.
    let mut args: Vec<Val> = Vec::new(&env);
    let bad_arg: Bytes = Bytes::from_slice(&env, b"not_a_u32");
    args.push_back(bad_arg.into_val(&env));

    let action = ProposalAction::InvokeContract {
        contract: target_id.clone(),
        fn_symbol,
        args,
    };
    let payload_hash = make_hash(&env, b"wrong_type_hash");

    let id = multisig.create_proposal(
        &signers.get(0).unwrap(),
        &action,
        &payload_hash,
        &500u64,
    );

    multisig.approve_proposal(&signers.get(0).unwrap(), &id);
    multisig.approve_proposal(&signers.get(1).unwrap(), &id);

    // This must panic because `set_value` expects a u32, not Bytes.
    multisig.execute_proposal(&signers.get(0).unwrap(), &id, &payload_hash);
}

// ---------------------------------------------------------------------------
// Verify empty-args still works for zero-parameter functions
// ---------------------------------------------------------------------------

/// A zero-argument cross-contract call still works when `args` is empty.
/// This ensures the fix is backward-compatible for no-arg entrypoints.
#[test]
fn test_invoke_contract_no_args_works() {
    let env = make_env();

    let (multisig_id, signers) = setup_multisig(&env);
    let multisig = MultisigContractClient::new(&env, &multisig_id);

    let target_id = env.register(TargetContract, ());
    let target = TargetContractClient::new(&env, &target_id);

    // `get_value` takes no arguments.  Use it as the target function to
    // prove that invoking with an empty Vec<Val> still works end-to-end.
    let fn_symbol: Symbol = Symbol::new(&env, "get_value");
    let args: Vec<Val> = Vec::new(&env);

    let action = ProposalAction::InvokeContract {
        contract: target_id.clone(),
        fn_symbol,
        args,
    };
    let payload_hash = make_hash(&env, b"no_args_payload");

    let id = multisig.create_proposal(
        &signers.get(0).unwrap(),
        &action,
        &payload_hash,
        &500u64,
    );

    multisig.approve_proposal(&signers.get(0).unwrap(), &id);
    multisig.approve_proposal(&signers.get(1).unwrap(), &id);
    multisig.execute_proposal(&signers.get(0).unwrap(), &id, &payload_hash);

    // Value is still 0 (get_value doesn't mutate state), proposal is Executed.
    assert_eq!(target.get_value(), 0u32);
    assert_eq!(multisig.get_proposal(&id).status, ProposalStatus::Executed);
}
