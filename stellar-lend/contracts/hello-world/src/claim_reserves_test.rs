#![cfg(test)]

//! Tests for exact-accounting bounds on `HelloContract::claim_reserves`.
//!
//! Goal: prove that protocol reserve claims are bounded by the accrued
//! `DepositDataKey::ProtocolReserve(asset)` balance and that storage is debited
//! exactly by the claimed amount on a successful claim.
//!
//! These tests pin:
//! - **Over-claim guard**  — a claim with `amount > reserve_balance` is rejected
//!   with `RiskManagementError::InvalidParameter` and the reserve is unchanged.
//! - **Exact debit**       — on success, `new_balance == old_balance − amount`
//!   with no rounding, no off-by-one.
//! - **Admin gate**        — non-admin callers are rejected with
//!   `RiskManagementError::Unauthorized`; the reserve is unchanged.
//! - **View consistency**  — `get_reserve_balance(asset)` reflects the
//!   post-claim storage state for any caller.
//! - **Asset isolation**   — a claim against one asset's `ProtocolReserve`
//!   bucket never affects any other asset's bucket.
//! - **Boundary**          — `amount == reserve_balance` is allowed and zeros
//!   the bucket (the `>` vs `>=` boundary in the clamp).

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::deposit::DepositDataKey;
use crate::risk_management::RiskManagementError;
use crate::{HelloContract, HelloContractClient};

// ── Test helpers ───────────────────────────────────────────────────────────

/// Deploys `HelloContract`, mocks all authentication, and initialises the
/// contract with `admin` as the protocol admin.
///
/// # Arguments
/// * `env` — the Soroban test environment.
///
/// # Returns
/// A `(client, contract_id, admin, user)` tuple:
/// - `client`     — typed `HelloContractClient` bound to the deployed contract.
/// - `contract_id`— the on-chain contract address (used with `env.as_contract`
///                  to seed persistent storage for the deployed contract).
/// - `admin`      — initialised protocol admin; passes `require_admin`.
/// - `user`       — a fresh non-admin address used to drive auth-failure
///                  tests.
fn setup_contract(
    env: &Env,
) -> (HelloContractClient, Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, HelloContract);
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let user = Address::generate(env);
    client.initialize(&admin);
    (client, contract_id, admin, user)
}

/// Seeds `DepositDataKey::ProtocolReserve(Some(asset))` with `balance` stroops.
///
/// Mirrors the flash-loan fee path: the `ProtocolReserve` bucket is credited
/// outside of `accrue_reserve`, so tests must seed it directly by writing
/// through `env.as_contract`, which evaluates the storage write inside the
/// deployed contract's address space.
///
/// # Arguments
/// * `env`         — the Soroban test environment.
/// * `contract_id` — the deployed hello-world contract address.
/// * `asset`       — the asset whose reserve bucket is being seeded.
/// * `balance`     — the accrued reserve balance (stroops, `i128`).
fn seed_protocol_reserve(
    env: &Env,
    contract_id: &Address,
    asset: &Address,
    balance: i128,
) {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::ProtocolReserve(Some(asset.clone()));
        env.storage().persistent().set(&key, &balance);
    });
}

// ── 1. Bounding: a full claim zeros the reserve ─────────────────────────────

/// A claim equal to the accrued reserve must succeed and zero the bucket.
#[test]
fn test_full_claim_zeros_reserve() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 1_000);
    client.claim_reserves(&admin, &Some(asset.clone()), &to, &1_000);

    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        0,
        "full claim must zero the reserve"
    );
}

// ── 2. Bounding: a partial claim leaves the exact remainder ─────────────────

/// A claim of less than the accrued reserve must deduct exactly and leave
/// `old_balance − amount` in the bucket.  No rounding, no off-by-one.
#[test]
fn test_partial_claim_leaves_exact_remainder() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 1_000);
    client.claim_reserves(&admin, &Some(asset.clone()), &to, &400);

    // 1000 − 400 = 600 (exact remainder, integer arithmetic, no fees).
    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        600,
        "partial claim must leave the exact remainder"
    );
}

// ── 3. Bounding: an over-claim is rejected, never overdrawn ─────────────────

/// A claim with `amount == reserve_balance + 1` is rejected with
/// `InvalidParameter`.  The reserve must be unchanged after rejection — the
/// protocol can never be overdrawn into insolvency.
#[test]
fn test_over_claim_is_rejected_never_overdrawn() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 500);
    let err = client
        .try_claim_reserves(&admin, &Some(asset.clone()), &to, &501)
        .expect_err("over-claim must be rejected");
    assert_eq!(err, RiskManagementError::InvalidParameter);

    // Reserve is untouched after the rejection.
    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        500,
        "reserve must be unchanged after over-claim rejection"
    );
}

// ── 4. Boundary: `amount == reserve_balance` is the upper bound ─────────────

/// Asserting the `>` vs `>=` boundary: a claim of exactly the balance is
/// allowed (boundary, success) and zeros the reserve.  This pins the exact
/// branch the over-by-one tests above are validating against.
#[test]
fn test_exact_balance_claim_zeros_reserve() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 777);
    client.claim_reserves(&admin, &Some(asset.clone()), &to, &777);

    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        0,
        "exact-balance claim must zero the reserve"
    );
}

// ── 5. Empty bucket: claim against zero reserve ────────────────────────────

/// A bucket of zero cannot satisfy any positive claim — over-claim semantics
/// also reject `1 > 0` exactly the same as `100 > 0`.
#[test]
fn test_zero_reserve_claim_is_rejected() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 0);
    let err = client
        .try_claim_reserves(&admin, &Some(asset.clone()), &to, &1)
        .expect_err("claim against zero reserve must be rejected");
    assert_eq!(err, RiskManagementError::InvalidParameter);

    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        0,
        "zero reserve must remain zero on rejection"
    );
}

// ── 6. Empty bucket: zero-amount claim is a no-op ───────────────────────────

/// `0 > 0` is false, so a zero-amount claim against a zero reserve is
/// permitted and is a no-op for the ledger.
#[test]
fn test_zero_reserve_zero_amount_claim_is_noop() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 0);
    client.claim_reserves(&admin, &Some(asset.clone()), &to, &0);

    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        0,
        "zero-amount claim against zero reserve must be a no-op"
    );
}

/// `0 > reserve_balance` is false for any non-zero bucket, so a zero-amount
/// claim must be a no-op regardless of the accrued balance.
#[test]
fn test_zero_amount_claim_against_positive_reserve_is_noop() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 1_000);
    client.claim_reserves(&admin, &Some(asset.clone()), &to, &0);

    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        1_000,
        "zero-amount claim must not debit the reserve"
    );
}

// ── 7. Authorization: non-admin caller is rejected ──────────────────────────

/// Non-admin callers cannot move reserves.  The reserve must be untouched.
#[test]
fn test_non_admin_claim_is_rejected() {
    let env = Env::default();
    let (client, contract_id, _admin, user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 1_000);
    let err = client
        .try_claim_reserves(&user, &Some(asset.clone()), &to, &100)
        .expect_err("non-admin claim must be rejected");
    assert_eq!(err, RiskManagementError::Unauthorized);

    // Reserve is unchanged after the auth rejection.
    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        1_000,
        "reserve must be unchanged on non-admin rejection"
    );
}

// ── 8. Sequence: successive partial claims drain the reserve exactly ────────

/// Two successive partial claims must debits compound without loss.  After
/// draining the reserve, any subsequent claim is rejected.
#[test]
fn test_two_partial_claims_then_full_claim_drain() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 1_000);

    client.claim_reserves(&admin, &Some(asset.clone()), &to, &250);
    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        750,
        "first partial claim must leave exact remainder"
    );

    client.claim_reserves(&admin, &Some(asset.clone()), &to, &750);
    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        0,
        "second claim must drain the reserve to zero"
    );

    // Subsequent claim against the depleted reserve is rejected.
    let err = client
        .try_claim_reserves(&admin, &Some(asset.clone()), &to, &1)
        .expect_err("depleted-reserve claim must be rejected");
    assert_eq!(err, RiskManagementError::InvalidParameter);
    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        0,
        "depleted reserve must remain zero after rejected over-claim"
    );
}

// ── 9. Asset isolation: claims on one bucket do not touch another ───────────

/// Each asset has its own `ProtocolReserve` bucket keyed by `Option<Address>`.
/// A claim on asset A must never alter asset B's bucket.
#[test]
fn test_multiple_assets_isolated_balances() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset_a, 500);
    seed_protocol_reserve(&env, &contract_id, &asset_b, 1_500);

    client.claim_reserves(&admin, &Some(asset_a.clone()), &to, &200);
    assert_eq!(
        client.get_reserve_balance(&Some(asset_a)),
        300,
        "asset A reserve must reflect exact debit"
    );
    assert_eq!(
        client.get_reserve_balance(&Some(asset_b)),
        1_500,
        "asset B reserve must be untouched by asset A claim"
    );
}

// ── 10. View consistency: `get_reserve_balance` reflects post-claim state ───

/// After a successful claim, `get_reserve_balance` must return
/// `old_balance − amount`.  This pins the read-side invariant against
/// any disconnect between the debit logic and the view function.
#[test]
fn test_post_claim_balance_reflects_storage_state() {
    let env = Env::default();
    let (client, contract_id, admin, _user) = setup_contract(&env);
    let asset = Address::generate(&env);
    let to = Address::generate(&env);

    seed_protocol_reserve(&env, &contract_id, &asset, 2_000);

    // Pre-claim view: stored balance.
    assert_eq!(
        client.get_reserve_balance(&Some(asset.clone())),
        2_000,
        "pre-claim view must return the seeded balance"
    );

    // Partial claim.
    client.claim_reserves(&admin, &Some(asset.clone()), &to, &1_234);

    // Post-claim view: debited storage state.
    assert_eq!(
        client.get_reserve_balance(&Some(asset)),
        766,
        "post-claim view must reflect exactly `old_balance - amount`"
    );
}
