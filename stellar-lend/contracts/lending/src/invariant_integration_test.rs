//! Integration coverage for the reserve-invariant module (`invariants.rs`).
//!
//! These tests drive the canonical `LendingContract` and then assert on the
//! reserve invariant using a **real** SEP-41 token balance, so the checkpoint
//! helpers exercise the token-client path rather than a hand-written accounting
//! value. A mocked `TokenClient` that always returns the expected number proves
//! nothing; minting real units and moving them with `transfer` does.
//!
//! ## Why these checks are not wired into the entrypoints
//!
//! `compute_expected_reserve` models only `TotalDeposits - BadDebt`, and the
//! legacy single-asset flows invalidate that model in both directions:
//!
//! * `deposit`/`withdraw` mutate `TotalDeposits` without moving a single token,
//!   so the accounting ledger and the contract's token balance necessarily
//!   diverge.
//! * `liquidate` moves both tokens (repayment in, seized collateral out)
//!   without touching `TotalDeposits`.
//!
//! Enabling `check_invariant_*` inside those paths therefore makes every call
//! trap. The checks stay opt-in and are exercised directly here;
//! [`legacy_deposit_mutates_accounting_without_moving_tokens`] is the regression
//! guard for that decision — if the legacy path ever starts moving tokens, the
//! wiring decision can be revisited.

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use crate::{invariants, DataKey, LendingContract, LendingContractClient};

/// Register an initialized lending contract.
fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

/// Register a real SEP-41 asset and return its address.
fn new_asset(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

fn balance(env: &Env, asset: &Address, who: &Address) -> i128 {
    TokenClient::new(env, asset).balance(who)
}

fn expected_reserve(env: &Env, id: &Address, asset: &Address) -> i128 {
    env.as_contract(id, || invariants::compute_expected_reserve(env, asset))
}

fn set_accounting(env: &Env, id: &Address, total_deposits: i128, bad_debt: i128) {
    env.as_contract(id, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &total_deposits);
        env.storage().persistent().set(&DataKey::BadDebt, &bad_debt);
    });
}

#[test]
fn expected_reserve_is_zero_when_nothing_is_accounted() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);
    assert_eq!(expected_reserve(&env, &client.address, &asset), 0);
}

#[test]
fn expected_reserve_is_total_deposits_minus_bad_debt() {
    let (env, client, _admin, _user) = setup();
    set_accounting(&env, &client.address, 1_000, 100);
    assert_eq!(
        expected_reserve(&env, &client.address, &new_asset(&env)),
        900
    );
}

#[test]
fn checkpoints_hold_when_held_balance_matches_accounting_exactly() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);
    StellarAssetClient::new(&env, &asset).mint(&client.address, &1_000);
    set_accounting(&env, &client.address, 1_000, 0);

    env.as_contract(&client.address, || {
        // Exact equality is the boundary: neither checkpoint may panic.
        invariants::check_invariant_before(&env, &asset);
        invariants::check_invariant_after(&env, &asset);
    });
}

#[test]
fn checkpoints_hold_for_zero_reserves_and_zero_accounting() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);

    env.as_contract(&client.address, || {
        invariants::check_invariant_before(&env, &asset);
        invariants::check_invariant_after(&env, &asset);
    });
}

#[test]
fn checkpoints_hold_when_bad_debt_offsets_deposits() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);
    StellarAssetClient::new(&env, &asset).mint(&client.address, &600);
    set_accounting(&env, &client.address, 1_000, 400);

    env.as_contract(&client.address, || {
        invariants::check_invariant_before(&env, &asset)
    });
}

#[test]
fn macro_returns_the_wrapped_body_value() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);

    env.as_contract(&client.address, || {
        let value = crate::with_invariant_check!(&env, &asset, { 7 + 35 });
        assert_eq!(value, 42);
    });
}

// ---------------------------------------------------------------------------
// Failure paths: drift is detected
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn checkpoint_rejects_a_single_unit_of_drift() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);
    // One unit short of the ledger: the smallest possible drift must still fail.
    StellarAssetClient::new(&env, &asset).mint(&client.address, &999);
    set_accounting(&env, &client.address, 1_000, 0);

    env.as_contract(&client.address, || {
        invariants::check_invariant_before(&env, &asset)
    });
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn checkpoint_detects_an_external_transfer_out_of_the_contract() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);
    let thief = Address::generate(&env);
    StellarAssetClient::new(&env, &asset).mint(&client.address, &1_000);
    set_accounting(&env, &client.address, 1_000, 0);

    // Simulate a balance leak that bypasses the accounting ledger.
    TokenClient::new(&env, &asset).transfer(&client.address, &thief, &1);

    env.as_contract(&client.address, || {
        invariants::check_invariant_after(&env, &asset)
    });
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn macro_aborts_the_body_when_reserves_drift() {
    let (env, client, _admin, _user) = setup();
    let asset = new_asset(&env);
    StellarAssetClient::new(&env, &asset).mint(&client.address, &1);
    set_accounting(&env, &client.address, 2, 0);

    env.as_contract(&client.address, || {
        crate::with_invariant_check!(&env, &asset, { 42 })
    });
}

// ---------------------------------------------------------------------------
// Regression guard for the wiring decision
// ---------------------------------------------------------------------------

#[test]
fn legacy_deposit_mutates_accounting_without_moving_tokens() {
    let (env, client, _admin, user) = setup();
    let asset = new_asset(&env);

    let before = balance(&env, &asset, &client.address);
    client.deposit(&user, &100);
    let after = balance(&env, &asset, &client.address);

    assert_eq!(
        before, after,
        "the legacy deposit path is pure accounting; it must not move tokens"
    );
    assert_eq!(after, 0);
    assert_eq!(
        expected_reserve(&env, &client.address, &asset),
        100,
        "accounting rose while the held balance did not, so the reserve model \
         cannot be satisfied on the legacy deposit path"
    );
}
