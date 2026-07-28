//! Tests for the `initialize` auth requirement (issue #1498).
//!
//! `LendingContract::initialize` must call `admin.require_auth()` so that an
//! attacker cannot front-run the deployer and claim the admin role by
//! submitting their own `initialize` transaction first.
//!
//! Test coverage:
//! - Happy path: the intended admin signs → `initialize` succeeds.
//! - Sad path: a different address signs → the call is rejected by the Soroban
//!   runtime (panics, which is the expected behaviour for auth failures in the
//!   Soroban test harness).
//! - Sad path: no auth mock at all → the call is rejected.

#![cfg(test)]

use super::{LendingContract, LendingContractClient};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register a fresh, *uninitialized* contract and return the environment,
/// client, and contract id.
fn fresh_contract() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    (env, client, contract_id)
}

/// Configure `env` so that exactly `signer` is treated as having authorised a
/// call to `initialize` with `admin_arg` on `contract_id`.
fn mock_initialize_auth(env: &Env, contract_id: &Address, signer: &Address, admin_arg: &Address) {
    env.mock_auths(&[MockAuth {
        address: signer,
        invoke: &MockAuthInvoke {
            contract: contract_id,
            fn_name: "initialize",
            args: (admin_arg.clone(),).into_val(env),
            sub_invokes: &[],
        },
    }]);
}

// ---------------------------------------------------------------------------
// Happy-path tests
// ---------------------------------------------------------------------------

/// The intended admin signs the `initialize` call → contract initialises
/// successfully and `get_admin` returns the correct address.
#[test]
fn initialize_with_admin_signature_succeeds() {
    let (env, client, contract_id) = fresh_contract();
    let admin = Address::generate(&env);

    mock_initialize_auth(&env, &contract_id, &admin, &admin);
    client.initialize(&admin);

    assert_eq!(client.get_admin(), admin);
}

/// A third-party helper that *does* hold the admin's authority (e.g., a
/// multisig wrapper) can call `initialize` on behalf of the admin, as long as
/// the admin's signature is present in the auth tree.  We model this by
/// providing `admin` as the signer even when a different caller address
/// triggers the transaction.
#[test]
fn initialize_with_correct_auth_sets_admin() {
    let (env, client, contract_id) = fresh_contract();
    let admin = Address::generate(&env);
    let _unrelated = Address::generate(&env); // would be the tx submitter in prod

    // The auth entry is for `admin`, which is what `admin.require_auth()` checks.
    mock_initialize_auth(&env, &contract_id, &admin, &admin);
    client.initialize(&admin);

    assert_eq!(client.get_admin(), admin);
}

// ---------------------------------------------------------------------------
// Front-running / sad-path tests  (acceptance criteria for #1498)
// ---------------------------------------------------------------------------

/// A non-deployer submits `initialize` but provides *only their own* signature
/// (not the intended admin's). The runtime must reject the call.
///
/// This is the acceptance-criteria test from issue #1498.
#[test]
#[should_panic]
fn initialize_without_admin_signature_is_rejected() {
    let (env, client, contract_id) = fresh_contract();
    let intended_admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    // The attacker signs for themselves — they do NOT hold the intended admin's
    // signature, so `intended_admin.require_auth()` must fail.
    mock_initialize_auth(&env, &contract_id, &attacker, &intended_admin);
    // This call must panic (auth failure).
    client.initialize(&intended_admin);
}

/// `initialize` called with *no* auth context at all must be rejected.
/// No `mock_auths` or `mock_all_auths` is set up, so `require_auth` has
/// nothing to satisfy.
#[test]
#[should_panic]
fn initialize_with_no_auth_context_is_rejected() {
    let (env, client, _contract_id) = fresh_contract();
    let admin = Address::generate(&env);
    // No mock_auths or mock_all_auths — require_auth() will panic.
    client.initialize(&admin);
}

/// The attacker names *themselves* as admin and signs for themselves.  Even
/// if the attacker's signature is technically consistent with the argument,
/// `intended_admin.require_auth()` checks the *argument address*, so using a
/// different `admin` argument means the deployed-as address won't match.
///
/// Concretely: `attacker.require_auth()` would pass if `admin == attacker`,
/// but the protocol's slot should only be claimable by whoever controls the
/// address they pass as `admin`.  The guard prevents a third party from
/// nominating *someone else* as admin without that party's signature.
#[test]
fn initialize_attacker_naming_themselves_is_accepted_for_their_own_address() {
    // This test documents a nuance: if an attacker passes *their own* address as
    // admin and signs for it, the contract accepts them as admin — that is correct
    // behaviour.  The front-running protection is about preventing an attacker from
    // naming the *legitimate* admin (or any other address) without that address's
    // consent.
    let (env, client, contract_id) = fresh_contract();
    let attacker = Address::generate(&env);

    // Attacker signs for their own address and passes it as admin.
    mock_initialize_auth(&env, &contract_id, &attacker, &attacker);
    client.initialize(&attacker);

    // They become admin — their own choice, their own key, their own risk.
    assert_eq!(client.get_admin(), attacker);
}

/// A second `initialize` call (after the first succeeds) must return
/// `AlreadyInitialized` regardless of auth.
#[test]
fn double_initialize_returns_already_initialized() {
    use super::LendingError;

    let (env, client, contract_id) = fresh_contract();
    let admin = Address::generate(&env);

    // First call — valid auth.
    mock_initialize_auth(&env, &contract_id, &admin, &admin);
    client.initialize(&admin);

    // Second call — also valid auth but should be rejected with AlreadyInitialized.
    let admin2 = Address::generate(&env);
    mock_initialize_auth(&env, &contract_id, &admin2, &admin2);
    let result = client.try_initialize(&admin2);
    assert_eq!(result, Err(Ok(LendingError::AlreadyInitialized)));

    // Original admin is unchanged.
    assert_eq!(client.get_admin(), admin);
}
