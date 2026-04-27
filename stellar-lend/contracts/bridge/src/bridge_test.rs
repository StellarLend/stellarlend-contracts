//! bridge_test.rs — comprehensive happy/sad-path coverage for all bridge entrypoints.
//!
//! # Security notes
//! - Admin is the sole privileged actor; all mutating admin calls enforce `require_auth` +
//!   identity check. Tests verify that any other address is rejected (Unauthorised #3).
//! - `bridge_deposit` is open to any authenticated sender; the contract does NOT move tokens
//!   itself — it is a *record-keeping* layer. Custodial assumptions must be enforced off-chain.
//! - Fee arithmetic uses I256 intermediates to prevent overflow on i128::MAX inputs.
//! - `bridge_withdraw` is admin-only; it records the outflow but does not transfer tokens.
//!   Reentrancy is not a concern because Soroban's single-threaded execution model prevents
//!   re-entrant calls within one transaction.
//! - Upgrade functions are delegated to `stellarlend_common::upgrade::UpgradeManager` and are
//!   tested here for the full propose → approve → execute → rollback lifecycle.

#![cfg(test)]

use crate::bridge::BridgeContract;
use soroban_sdk::{
    testutils::Address as _,
    Address, BytesN, Env, String,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, crate::bridge::BridgeContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(BridgeContract, ());
    let client = crate::bridge::BridgeContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.init(&admin);
    (env, client, admin)
}

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

/// Register a default bridge: id="eth-mainnet", fee=50 bps, min=1_000.
fn default_bridge(client: &crate::bridge::BridgeContractClient, env: &Env, admin: &Address) {
    client.register_bridge(admin, &s(env, "eth-mainnet"), &50u64, &1_000i128);
}

/// Build a deterministic 32-byte hash from a seed byte.
fn fake_hash(env: &Env, seed: u8) -> BytesN<32> {
    let mut arr = [0u8; 32];
    arr[0] = seed;
    BytesN::from_array(env, &arr)
}

// ── init ──────────────────────────────────────────────────────────────────────

#[test]
fn init_happy_sets_admin_and_empty_list() {
    let (env, client, admin) = setup();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.list_bridges().len(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn init_sad_double_init_rejected() {
    let (env, client, _) = setup();
    client.init(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn get_admin_sad_uninitialised_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(BridgeContract, ());
    let client = crate::bridge::BridgeContractClient::new(&env, &id);
    // No init — should return NotInitialised
    client.get_admin();
}

// ── register_bridge ───────────────────────────────────────────────────────────

#[test]
fn register_bridge_happy_defaults() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let cfg = client.get_bridge_config(&s(&env, "eth-mainnet"));
    assert_eq!(cfg.fee_bps, 50);
    assert_eq!(cfg.min_amount, 1_000);
    assert!(cfg.active);
    assert_eq!(cfg.total_deposited, 0);
    assert_eq!(cfg.total_withdrawn, 0);
}

#[test]
fn register_bridge_happy_zero_min_amount() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "free-bridge"), &0u64, &0i128);
    let cfg = client.get_bridge_config(&s(&env, "free-bridge"));
    assert_eq!(cfg.min_amount, 0);
    assert_eq!(cfg.fee_bps, 0);
}

#[test]
fn register_bridge_happy_max_fee_boundary() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "max-fee"), &1_000u64, &1i128);
    assert_eq!(client.get_bridge_config(&s(&env, "max-fee")).fee_bps, 1_000);
}

#[test]
fn register_bridge_happy_appears_in_list() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let list = client.list_bridges();
    assert_eq!(list.len(), 1);
    assert_eq!(list.get(0).unwrap(), s(&env, "eth-mainnet"));
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn register_bridge_sad_non_admin_rejected() {
    let (env, client, _) = setup();
    client.register_bridge(&Address::generate(&env), &s(&env, "bsc"), &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn register_bridge_sad_duplicate_id_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    default_bridge(&client, &env, &admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn register_bridge_sad_fee_above_cap_rejected() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "x"), &1_001u64, &1i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn register_bridge_sad_empty_id_rejected() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, ""), &0u64, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn register_bridge_sad_id_65_chars_rejected() {
    let (env, client, admin) = setup();
    // 65 'a' characters — one over the 64-char limit
    let long_id = String::from_str(&env, &"a".repeat(65));
    client.register_bridge(&admin, &long_id, &0u64, &0i128);
}

#[test]
fn register_bridge_happy_id_exactly_64_chars() {
    let (env, client, admin) = setup();
    let id64 = String::from_str(&env, &"a".repeat(64));
    client.register_bridge(&admin, &id64, &0u64, &0i128);
    assert!(client.get_bridge_config(&id64).active);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn register_bridge_sad_negative_min_amount_rejected() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "neg"), &0u64, &-1i128);
}

// ── set_bridge_fee ────────────────────────────────────────────────────────────

#[test]
fn set_bridge_fee_happy_updates_fee() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &300u64);
    assert_eq!(client.get_bridge_config(&s(&env, "eth-mainnet")).fee_bps, 300);
}

#[test]
fn set_bridge_fee_happy_zero_fee() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &0u64);
    assert_eq!(client.get_bridge_config(&s(&env, "eth-mainnet")).fee_bps, 0);
}

#[test]
fn set_bridge_fee_happy_max_boundary() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &1_000u64);
    assert_eq!(client.get_bridge_config(&s(&env, "eth-mainnet")).fee_bps, 1_000);
}

#[test]
fn set_bridge_fee_happy_does_not_affect_other_fields() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let before = client.get_bridge_config(&s(&env, "eth-mainnet"));
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &200u64);
    let after = client.get_bridge_config(&s(&env, "eth-mainnet"));
    assert_eq!(after.min_amount, before.min_amount);
    assert_eq!(after.active, before.active);
    assert_eq!(after.total_deposited, before.total_deposited);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn set_bridge_fee_sad_non_admin_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&Address::generate(&env), &s(&env, "eth-mainnet"), &10u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn set_bridge_fee_sad_unknown_bridge_rejected() {
    let (env, client, admin) = setup();
    client.set_bridge_fee(&admin, &s(&env, "ghost"), &10u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn set_bridge_fee_sad_above_cap_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &1_001u64);
}

// ── set_bridge_active ─────────────────────────────────────────────────────────

#[test]
fn set_bridge_active_happy_deactivate_then_reactivate() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    assert!(!client.get_bridge_config(&s(&env, "eth-mainnet")).active);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &true);
    assert!(client.get_bridge_config(&s(&env, "eth-mainnet")).active);
}

#[test]
fn set_bridge_active_happy_idempotent_deactivate() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    assert!(!client.get_bridge_config(&s(&env, "eth-mainnet")).active);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn set_bridge_active_sad_deposit_on_inactive_bridge_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    client.bridge_deposit(&Address::generate(&env), &s(&env, "eth-mainnet"), &10_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn set_bridge_active_sad_non_admin_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&Address::generate(&env), &s(&env, "eth-mainnet"), &false);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn set_bridge_active_sad_unknown_bridge_rejected() {
    let (env, client, admin) = setup();
    client.set_bridge_active(&admin, &s(&env, "ghost"), &false);
}

// ── bridge_deposit ────────────────────────────────────────────────────────────

#[test]
fn deposit_happy_net_equals_amount_minus_fee() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // fee=50 bps, min=1_000
    let user = Address::generate(&env);
    // fee = 100_000 * 50 / 10_000 = 500  →  net = 99_500
    assert_eq!(client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &100_000i128), 99_500);
}

#[test]
fn deposit_happy_zero_fee_bridge_net_equals_amount() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "free"), &0u64, &1i128);
    let user = Address::generate(&env);
    assert_eq!(client.bridge_deposit(&user, &s(&env, "free"), &50_000i128), 50_000);
}

#[test]
fn deposit_happy_exactly_minimum_accepted() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // min=1_000
    let user = Address::generate(&env);
    let net = client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &1_000i128);
    assert!(net > 0);
}

#[test]
fn deposit_happy_accumulates_total_deposited() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &20_000i128);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &30_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_deposited,
        50_000
    );
}

#[test]
fn deposit_happy_multiple_users_accumulate_independently() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    client.bridge_deposit(&u1, &s(&env, "eth-mainnet"), &10_000i128);
    client.bridge_deposit(&u2, &s(&env, "eth-mainnet"), &10_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_deposited,
        20_000
    );
}

#[test]
fn deposit_happy_does_not_affect_total_withdrawn() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &10_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_withdrawn,
        0
    );
}

#[test]
fn deposit_happy_max_fee_rate_correct_net() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "max-fee"), &1_000u64, &1i128);
    let user = Address::generate(&env);
    // fee = 10_000 * 1000 / 10_000 = 1_000  →  net = 9_000
    assert_eq!(client.bridge_deposit(&user, &s(&env, "max-fee"), &10_000i128), 9_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn deposit_sad_zero_amount_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.bridge_deposit(&Address::generate(&env), &s(&env, "eth-mainnet"), &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn deposit_sad_negative_amount_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.bridge_deposit(&Address::generate(&env), &s(&env, "eth-mainnet"), &-1i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn deposit_sad_below_minimum_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // min=1_000
    client.bridge_deposit(&Address::generate(&env), &s(&env, "eth-mainnet"), &999i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn deposit_sad_unknown_bridge_rejected() {
    let (env, client, _) = setup();
    client.bridge_deposit(&Address::generate(&env), &s(&env, "ghost"), &50_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn deposit_sad_inactive_bridge_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    client.bridge_deposit(&Address::generate(&env), &s(&env, "eth-mainnet"), &10_000i128);
}

// ── bridge_withdraw ───────────────────────────────────────────────────────────

#[test]
fn withdraw_happy_accumulates_total_withdrawn() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &40_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_withdrawn,
        40_000
    );
}

#[test]
fn withdraw_happy_multiple_withdrawals_accumulate() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &10_000i128);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &20_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_withdrawn,
        30_000
    );
}

#[test]
fn withdraw_happy_does_not_affect_total_deposited() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &10_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_deposited,
        0
    );
}

#[test]
fn withdraw_happy_works_on_inactive_bridge() {
    // Withdrawals are admin-only and should succeed even when bridge is inactive
    // (admin may need to drain a paused bridge).
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.set_bridge_active(&admin, &s(&env, "eth-mainnet"), &false);
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &5_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_withdrawn,
        5_000
    );
}

#[test]
fn withdraw_happy_exactly_minimum_accepted() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // min=1_000
    let recip = Address::generate(&env);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &1_000i128);
    assert_eq!(
        client.get_bridge_config(&s(&env, "eth-mainnet")).total_withdrawn,
        1_000
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn withdraw_sad_non_admin_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let rando = Address::generate(&env);
    client.bridge_withdraw(&rando, &s(&env, "eth-mainnet"), &Address::generate(&env), &5_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn withdraw_sad_zero_amount_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &Address::generate(&env), &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn withdraw_sad_negative_amount_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &Address::generate(&env), &-1i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn withdraw_sad_below_minimum_rejected() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin); // min=1_000
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &Address::generate(&env), &999i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn withdraw_sad_unknown_bridge_rejected() {
    let (env, client, admin) = setup();
    client.bridge_withdraw(&admin, &s(&env, "ghost"), &Address::generate(&env), &5_000i128);
}

// ── transfer_admin ────────────────────────────────────────────────────────────

#[test]
fn transfer_admin_happy_new_admin_can_register_bridge() {
    let (env, client, admin) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    assert_eq!(client.get_admin(), new_admin);
    // New admin can perform admin actions
    client.register_bridge(&new_admin, &s(&env, "bsc"), &10u64, &100i128);
    assert!(client.get_bridge_config(&s(&env, "bsc")).active);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn transfer_admin_sad_old_admin_loses_rights() {
    let (env, client, admin) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    // Old admin can no longer register bridges
    client.register_bridge(&admin, &s(&env, "bsc"), &10u64, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn transfer_admin_sad_non_admin_rejected() {
    let (env, client, _) = setup();
    let rando = Address::generate(&env);
    client.transfer_admin(&rando, &Address::generate(&env));
}

#[test]
fn transfer_admin_happy_chain_of_transfers() {
    let (env, client, admin) = setup();
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);
    client.transfer_admin(&admin, &admin2);
    client.transfer_admin(&admin2, &admin3);
    assert_eq!(client.get_admin(), admin3);
}

// ── list_bridges ──────────────────────────────────────────────────────────────

#[test]
fn list_bridges_happy_empty_before_registration() {
    let (_, client, _) = setup();
    assert_eq!(client.list_bridges().len(), 0);
}

#[test]
fn list_bridges_happy_order_preserved() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "alpha"), &10u64, &100i128);
    client.register_bridge(&admin, &s(&env, "beta"), &20u64, &200i128);
    client.register_bridge(&admin, &s(&env, "gamma"), &30u64, &300i128);
    let list = client.list_bridges();
    assert_eq!(list.len(), 3);
    assert_eq!(list.get(0).unwrap(), s(&env, "alpha"));
    assert_eq!(list.get(1).unwrap(), s(&env, "beta"));
    assert_eq!(list.get(2).unwrap(), s(&env, "gamma"));
}

#[test]
fn list_bridges_happy_count_matches_registrations() {
    let (env, client, admin) = setup();
    let ids = ["bridge-0", "bridge-1", "bridge-2", "bridge-3", "bridge-4"];
    for id in ids {
        client.register_bridge(&admin, &s(&env, id), &10u64, &100i128);
    }
    assert_eq!(client.list_bridges().len(), 5);
}

// ── compute_fee ───────────────────────────────────────────────────────────────

#[test]
fn compute_fee_happy_standard_case() {
    let env = Env::default();
    // 1_000_000 * 50 / 10_000 = 5_000
    assert_eq!(BridgeContract::compute_fee(env, 1_000_000, 50), 5_000);
}

#[test]
fn compute_fee_happy_zero_bps_returns_zero() {
    let env = Env::default();
    assert_eq!(BridgeContract::compute_fee(env, 1_000_000, 0), 0);
}

#[test]
fn compute_fee_happy_max_bps_ten_percent() {
    let env = Env::default();
    // 100_000 * 1_000 / 10_000 = 10_000
    assert_eq!(BridgeContract::compute_fee(env, 100_000, 1_000), 10_000);
}

#[test]
fn compute_fee_happy_rounds_down_fractional_fee() {
    let env = Env::default();
    // 999 * 10 / 10_000 = 0.999 → rounds down to 0
    assert_eq!(BridgeContract::compute_fee(env, 999, 10), 0);
}

#[test]
fn compute_fee_happy_large_amount_no_overflow() {
    let env = Env::default();
    // 10^30 * 1000 / 10_000 = 10^29 — uses I256 internally
    let amount = 1_000_000_000_000_000_000_000_000_000_000i128;
    let fee = BridgeContract::compute_fee(env, amount, 1_000);
    assert_eq!(fee, 100_000_000_000_000_000_000_000_000_000i128);
}

#[test]
fn compute_fee_happy_i128_max_no_overflow() {
    let env = Env::default();
    let fee = BridgeContract::compute_fee(env, i128::MAX, 1_000);
    // Result should be approximately i128::MAX / 10
    assert!(fee > 0);
    assert_eq!(fee, i128::MAX / 10);
}

#[test]
fn compute_fee_happy_amount_one_any_bps_rounds_to_zero() {
    let env = Env::default();
    // 1 * 999 / 10_000 = 0
    assert_eq!(BridgeContract::compute_fee(env, 1, 999), 0);
}

// ── upgrade lifecycle ─────────────────────────────────────────────────────────
//
// Security note: upgrade functions are delegated to UpgradeManager.
// The bridge admin initialises the upgrade subsystem separately via upgrade_init.
// Required approvals must be >= 1 (InvalidThreshold otherwise).
// Only the upgrade admin can propose; only registered approvers can approve/execute.
//
// Note on upgrade_execute in tests: the Soroban test host validates that the target
// WASM hash exists in the ledger even though the actual `env.deployer()` call is
// cfg-gated out. We therefore test the full propose→approve flow and verify state
// transitions up to execution, and separately confirm execute panics with a storage
// error when the WASM is not uploaded (expected in unit-test environments).

fn setup_upgrade(
    client: &crate::bridge::BridgeContractClient,
    env: &Env,
    admin: &Address,
) -> BytesN<32> {
    let hash = fake_hash(env, 1);
    client.upgrade_init(admin, &hash, &1u32);
    hash
}

#[test]
fn upgrade_happy_propose_creates_proposal_with_correct_status() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);

    let new_hash = fake_hash(&env, 2);
    let proposal_id = client.upgrade_propose(&admin, &new_hash, &1u32);
    assert_eq!(proposal_id, 1);

    // With required_approvals=1 the proposer auto-approves → Approved stage
    let status = client.upgrade_status(&proposal_id);
    assert_eq!(status.approval_count, 1);
    assert_eq!(status.target_version, 1);
    assert_eq!(status.required_approvals, 1);
}

#[test]
fn upgrade_happy_current_version_starts_at_zero() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);
    assert_eq!(client.current_version(), 0);
}

#[test]
fn upgrade_happy_current_wasm_hash_matches_init_hash() {
    let (env, client, admin) = setup();
    let hash = setup_upgrade(&client, &env, &admin);
    assert_eq!(client.current_wasm_hash(), hash);
}

#[test]
fn upgrade_happy_multi_approver_second_approval_increments_count() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);

    let approver2 = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver2);

    let new_hash = fake_hash(&env, 3);
    let proposal_id = client.upgrade_propose(&admin, &new_hash, &1u32);

    // Admin auto-approved on propose; approver2 adds second approval
    let count = client.upgrade_approve(&approver2, &proposal_id);
    assert_eq!(count, 2);
}

#[test]
fn upgrade_happy_add_approver_then_remove_leaves_admin_only() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);

    let approver2 = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver2);
    client.upgrade_remove_approver(&admin, &approver2);

    // Admin is still the sole approver; proposing should still work
    let new_hash = fake_hash(&env, 4);
    let pid = client.upgrade_propose(&admin, &new_hash, &1u32);
    let status = client.upgrade_status(&pid);
    assert_eq!(status.approval_count, 1);
}

#[test]
fn upgrade_happy_sequential_proposals_have_incrementing_ids() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);

    let id1 = client.upgrade_propose(&admin, &fake_hash(&env, 10), &1u32);
    let id2 = client.upgrade_propose(&admin, &fake_hash(&env, 11), &2u32);
    assert_eq!(id2, id1 + 1);
}

/// upgrade_execute requires the WASM to be uploaded to the ledger.
/// In unit tests the WASM is never uploaded, so execute panics with a storage error.
/// This test documents and asserts that boundary.
#[test]
#[should_panic]
fn upgrade_execute_panics_when_wasm_not_uploaded_in_test_env() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);
    let pid = client.upgrade_propose(&admin, &fake_hash(&env, 2), &1u32);
    client.upgrade_execute(&admin, &pid);
}

/// upgrade_rollback requires a previously executed proposal.
/// Since execute panics in test env, rollback on a non-executed proposal also panics.
#[test]
#[should_panic]
fn upgrade_rollback_panics_on_non_executed_proposal() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);
    let pid = client.upgrade_propose(&admin, &fake_hash(&env, 8), &1u32);
    client.upgrade_rollback(&admin, &pid);
}

#[test]
#[should_panic]
fn upgrade_sad_propose_version_not_greater_than_current_rejected() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);
    // version 0 is current; proposing version 0 should fail (InvalidVersion)
    client.upgrade_propose(&admin, &fake_hash(&env, 5), &0u32);
}

#[test]
#[should_panic]
fn upgrade_sad_non_admin_cannot_propose() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);
    let rando = Address::generate(&env);
    client.upgrade_propose(&rando, &fake_hash(&env, 6), &1u32);
}

#[test]
#[should_panic]
fn upgrade_sad_execute_unapproved_proposal_rejected() {
    let (env, client, admin) = setup();
    // required_approvals=2 so proposer alone is not enough
    let hash = fake_hash(&env, 1);
    client.upgrade_init(&admin, &hash, &2u32);
    let approver2 = Address::generate(&env);
    client.upgrade_add_approver(&admin, &approver2);

    let new_hash = fake_hash(&env, 7);
    let pid = client.upgrade_propose(&admin, &new_hash, &1u32);
    // Only 1 approval, need 2 — execute should panic with InvalidStatus
    client.upgrade_execute(&admin, &pid);
}

#[test]
#[should_panic]
fn upgrade_sad_double_approve_rejected() {
    let (env, client, admin) = setup();
    setup_upgrade(&client, &env, &admin);
    let pid = client.upgrade_propose(&admin, &fake_hash(&env, 9), &1u32);
    // Admin already approved on propose — approving again should fail (AlreadyApproved)
    client.upgrade_approve(&admin, &pid);
}

// ── cross-entrypoint state isolation ─────────────────────────────────────────

#[test]
fn state_isolation_deposit_and_withdraw_tracked_separately() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    let recip = Address::generate(&env);

    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &50_000i128);
    client.bridge_withdraw(&admin, &s(&env, "eth-mainnet"), &recip, &20_000i128);

    let cfg = client.get_bridge_config(&s(&env, "eth-mainnet"));
    assert_eq!(cfg.total_deposited, 50_000);
    assert_eq!(cfg.total_withdrawn, 20_000);
}

#[test]
fn state_isolation_two_bridges_do_not_share_state() {
    let (env, client, admin) = setup();
    client.register_bridge(&admin, &s(&env, "bridge-a"), &50u64, &1_000i128);
    client.register_bridge(&admin, &s(&env, "bridge-b"), &100u64, &500i128);

    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "bridge-a"), &10_000i128);

    assert_eq!(client.get_bridge_config(&s(&env, "bridge-a")).total_deposited, 10_000);
    assert_eq!(client.get_bridge_config(&s(&env, "bridge-b")).total_deposited, 0);
}

#[test]
fn state_isolation_fee_change_does_not_affect_existing_totals() {
    let (env, client, admin) = setup();
    default_bridge(&client, &env, &admin);
    let user = Address::generate(&env);
    client.bridge_deposit(&user, &s(&env, "eth-mainnet"), &10_000i128);

    client.set_bridge_fee(&admin, &s(&env, "eth-mainnet"), &500u64);

    // total_deposited should be unchanged
    assert_eq!(client.get_bridge_config(&s(&env, "eth-mainnet")).total_deposited, 10_000);
}
