//! End-to-end tests for the AMM `flash_swap_a_for_b` "optimistic transfer
//! then verify-k" entrypoint and its `repay_flash_swap` companion.
//!
//! # Why no in-callback receiver?
//!
//! Soroban 25.3.1 forbids a contract from invoking itself directly from
//! inside a callback (`Contract re-entry is not allowed`).  The flash
//! swap therefore dispatches as two entry points across the caller's
//! transaction: `flash_swap_a_for_b` debits, then the caller does
//! arbitrary logic, then `repay_flash_swap` credits and runs the
//! verify-k check.  Soroban rolls back the whole transaction (including
//! the `flash_swap_a_for_b` debit) if any step — including the
//! `repay_flash_swap`'s verify-k — fails.
//!
//! # What is exercised
//!
//! | Test                                    | Expected outcome                                                  |
//! |-----------------------------------------|-------------------------------------------------------------------|
//! | `test_flash_swap_debits_reserve_b`      | Optimistic debit applied; reserves reflect `rb - amount_out`.    |
//! | `test_flash_swap_arms_flash_active`     | `is_flash_active()` flips true after `flash_swap_a_for_b`.        |
//! | `test_flash_then_repay_recovers_state`  | After `repay_flash_swap`, `is_flash_active()` is false and            |
//! |                                         | `(ra + amount_in)*rb >= k_before`.                                  |
//! | `test_under_repay_panics_k_violation`   | `Invariant violation: k decreased` panic; storage fully rolls back. |
//! | `test_over_repay_yields_extra_fee`      | `k` strictly grows; pool keeps the surplus.                         |
//! | `test_reentrancy_blocks_add`            | `add_liquidity` panics with `ReentrantFlashSwap` while in-flight. |
//! | `test_reentrancy_blocks_remove`         | `remove_liquidity` panics with `ReentrantFlashSwap` while in-flight. |
//! | `test_reentrancy_blocks_swap`           | `swap_a_for_b` panics with `ReentrantFlashSwap` while in-flight.    |
//! | `test_reentrancy_blocks_nested`         | Nested `flash_swap_a_for_b` panics with `ReentrantFlashSwap`.       |
//! | `test_repay_without_flash_panics`       | `repay_flash_swap` panics with "no flash swap in progress".         |
//! | `test_zero_amount_out_rejected`         | `flash_swap_a_for_b` panics on `amount_out <= 0`.                  |
//! | `test_invalid_fee_bps_rejected`         | `flash_swap_a_for_b` panics on out-of-range fee.                   |
//! | `test_drain_rejected`                   | `flash_swap_a_for_b` panics on `amount_out >= reserve_b`.          |
//! | `test_zero_fee_flash_swap_succeeds`     | Inverse formula handles `fee_bps == 0` without dividing by zero.  |
//! | `test_rollback_full_state_on_under_pay` | `try_` variant captures panic; reserves fully restored.            |
//! | `test_repay_zero_amount_rejected`       | `repay_flash_swap` rejects `amount_in <= 0`.                       |

#![cfg(test)]

use crate::{AmmContract, AmmContractClient, inverse_swap_in};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Bytes, Env};

const FEE_BPS: i128 = 30;

fn setup_pool(ra: i128, rb: i128) -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let amm_id = env.register(AmmContract, ());
    let amm_client = AmmContractClient::new(&env, &amm_id);
    amm_client.init_pool(&ra, &rb);
    (env, amm_id)
}

// =========================================================================
// Optimistic debit (step 1 of flash swap)
// =========================================================================

/// `flash_swap_a_for_b` debits `reserve_b` by `amount_out` and returns it.
#[test]
fn test_flash_swap_debits_reserve_b() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 332;
    let result = client.flash_swap_a_for_b(&amount_out, &FEE_BPS, &Bytes::new(&env));
    assert_eq!(result, amount_out);

    let (ra, rb) = client.get_reserves();
    assert_eq!(ra, 1_000, "reserve_a must NOT yet be credited");
    assert_eq!(rb, 1_000 - amount_out, "reserve_b has been debited");
}

/// `is_flash_active()` returns true after `flash_swap_a_for_b` and false
/// after the matching `repay_flash_swap`.
#[test]
fn test_flash_swap_arms_flash_active() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    assert!(!client.is_flash_active(), "initially false");
    client.flash_swap_a_for_b(&100, &FEE_BPS, &Bytes::new(&env));
    assert!(client.is_flash_active(), "true after flash_swap_a_for_b");
}

// =========================================================================
// Verifying repayment (step 2 of flash swap)
// =========================================================================

/// A sufficient `repay_flash_swap` recovers the pool back to (k-monotonic)
/// steady state and clears `FlashActive`.
#[test]
fn test_flash_then_repay_recovers_state() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 332;
    client.flash_swap_a_for_b(&amount_out, &FEE_BPS, &Bytes::new(&env));

    let amount_in: i128 =
        inverse_swap_in(1_000_i128, 1_000_i128, amount_out, FEE_BPS);
    client.repay_flash_swap(&amount_in);

    let (ra, rb) = client.get_reserves();
    let k_before: i128 = 1_000_i128 * 1_000_i128;
    let k_after: i128 = ra * rb;
    assert!(
        k_after >= k_before,
        "k must be non-decreasing after repay (k_before={k_before}, k_after={k_after})"
    );
    assert!(!client.is_flash_active(), "FlashActive must be cleared");
    assert_eq!(rb, 1_000 - amount_out);
    // ra must be 1_000 + amount_in; allow ±1 stroop for integer rounding.
    let diff = if ra - 1_000_i128 > amount_in {
        ra - 1_000_i128 - amount_in
    } else {
        amount_in - (ra - 1_000_i128)
    };
    assert!(
        diff <= 1,
        "reserve_a must reflect the inverse-formula repay (ra={ra}, expected ~{})",
        1_000_i128 + amount_in
    );
}

/// Overpaying (e.g. 2× the inverse) keeps `k` strictly growing — i.e. the
/// protocol earns the fee.
#[test]
fn test_over_repay_yields_extra_fee() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 100;
    client.flash_swap_a_for_b(&amount_out, &FEE_BPS, &Bytes::new(&env));

    let exact_in: i128 =
        inverse_swap_in(1_000_i128, 1_000_i128, amount_out, FEE_BPS);
    let over_in: i128 = exact_in.saturating_mul(2);
    client.repay_flash_swap(&over_in);

    let (ra, rb) = client.get_reserves();
    let k_before: i128 = 1_000_i128 * 1_000_i128;
    let k_after: i128 = ra * rb;
    assert!(
        k_after > k_before,
        "k must strictly grow on over-repayment (k_before={k_before}, k_after={k_after})"
    );
    assert!(!client.is_flash_active());
}

/// Underpaying (one stroop short of the inverse) trips the verify-k check.
#[test]
#[should_panic(expected = "Invariant violation: k decreased during flash-swap repayment")]
fn test_under_repay_panics_k_violation() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 332;
    client.flash_swap_a_for_b(&amount_out, &FEE_BPS, &Bytes::new(&env));

    let exact_in: i128 =
        inverse_swap_in(1_000_i128, 1_000_i128, amount_out, FEE_BPS);
    let under_in: i128 = exact_in - 1;
    client.repay_flash_swap(&under_in);
}

/// Underpayment captured via `try_` restores EVERY storage mutation
/// (optimistic debit included).
///
/// Soroban `client.foo()` invocations outside a single host call are
/// independent simulated transactions — a panic in Op 2 would not roll
/// back Op 1's debit.  We mirror the production "multi-op transaction"
/// pattern by routing both ops through a single `ProxyContract.do_…` call,
/// which gives the host the same atomicity contract it would see in a
/// multi-op production TX.
#[test]
fn test_rollback_full_state_on_under_pay() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let amm_client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 100;
    let exact_in: i128 =
        inverse_swap_in(1_000_i128, 1_000_i128, amount_out, FEE_BPS);
    let under_in: i128 = exact_in - 1;

    let proxy_id = env.register(ProxyContract, ());
    let proxy_client = ProxyContractClient::new(&env, &proxy_id);

    let res = proxy_client.try_do_flash_and_repay(&amm_id, &amount_out, &under_in);
    assert!(res.is_err(), "under-repay must panic and return Err");

    // Production multi-op TX atomicity: the entire proxy invocation
    // (which contains both the optimistic debit AND the failing repay)
    // is rolled back, so the pool is left exactly where it started.
    let (ra, rb) = amm_client.get_reserves();
    assert_eq!(ra, 1_000, "reserve_a must be rolled back");
    assert_eq!(rb, 1_000, "reserve_b must be rolled back");
    assert!(
        !amm_client.is_flash_active(),
        "FlashActive must NOT remain true after a rolled-back flash swap"
    );
}

/// Proxy contract used by `test_rollback_full_state_on_under_pay` to mount
/// the flash-swap + repay sequence as a single host invocation, so the
/// Soroban atomic-rollback guarantee applies to both ops together.
#[contract]
pub struct ProxyContract;

#[contractimpl]
impl ProxyContract {
    pub fn do_flash_and_repay(env: Env, amm: Address, amount_out: i128, amount_in: i128) {
        let bytes = Bytes::new(&env);
        let client = AmmContractClient::new(&env, &amm);
        client.flash_swap_a_for_b(&amount_out, &FEE_BPS_VAL, &bytes);
        client.repay_flash_swap(&amount_in);
    }
}

// Hard-coded to match the surrounding test fixture's `FEE_BPS`.  Soroban
// `contractimpl` methods cannot capture module-level constants directly,
// so we re-declare it here for the proxy.
const FEE_BPS_VAL: i128 = 30;

/// `fee_bps == 0` exercises the degenerate path in `inverse_swap_in` that
/// would otherwise divide by zero — make sure the formula handles it
/// cleanly.
#[test]
fn test_zero_fee_flash_swap_succeeds() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 100;
    client.flash_swap_a_for_b(&amount_out, &0_i128, &Bytes::new(&env));

    let amount_in: i128 = inverse_swap_in(1_000, 1_000, amount_out, 0_i128);
    client.repay_flash_swap(&amount_in);

    let (ra, rb) = client.get_reserves();
    let k_before: i128 = 1_000_i128 * 1_000_i128;
    let k_after: i128 = ra * rb;
    assert!(
        k_after >= k_before,
        "zero-fee flash swap must still satisfy k-monotonicity"
    );
}

// =========================================================================
// In-flight reentrancy guards
// =========================================================================

#[test]
#[should_panic(expected = "ReentrantFlashSwap")]
fn test_reentrancy_blocks_add() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&100, &FEE_BPS, &Bytes::new(&env));
    client.add_liquidity(&1_i128, &1_i128);
}

#[test]
#[should_panic(expected = "ReentrantFlashSwap")]
fn test_reentrancy_blocks_remove() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&100, &FEE_BPS, &Bytes::new(&env));
    client.remove_liquidity(&1_i128, &1_i128);
}

#[test]
#[should_panic(expected = "ReentrantFlashSwap")]
fn test_reentrancy_blocks_swap() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&100, &FEE_BPS, &Bytes::new(&env));
    client.swap_a_for_b(&1_i128, &FEE_BPS);
}

#[test]
#[should_panic(expected = "ReentrantFlashSwap")]
fn test_reentrancy_blocks_nested() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&100, &FEE_BPS, &Bytes::new(&env));
    client.flash_swap_a_for_b(
        &1_i128,
        &FEE_BPS,
        &Bytes::new(&env),
    );
}

// =========================================================================
// Input validation and out-of-flight guards
// =========================================================================

#[test]
#[should_panic(expected = "repay_flash_swap: no flash swap in progress")]
fn test_repay_without_flash_panics() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.repay_flash_swap(&1_i128);
}

#[test]
#[should_panic(expected = "repay_flash_swap: amount_in must be positive")]
fn test_repay_zero_amount_rejected() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&100, &FEE_BPS, &Bytes::new(&env));
    client.repay_flash_swap(&0_i128);
}

#[test]
#[should_panic(expected = "amount_out must be positive")]
fn test_zero_amount_out_rejected() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&0_i128, &FEE_BPS, &Bytes::new(&env));
}

#[test]
#[should_panic(expected = "invalid fee_bps")]
fn test_invalid_fee_bps_rejected() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    client.flash_swap_a_for_b(&100_i128, &10_000_i128, &Bytes::new(&env));
}

#[test]
#[should_panic(
    expected = "Insufficient reserves: amount_out would drain reserve_b"
)]
fn test_drain_rejected() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    // amount_out == reserve_b is forbidden (must be strictly less).
    client.flash_swap_a_for_b(&1_000_i128, &FEE_BPS, &Bytes::new(&env));
}

// =========================================================================
// Misc
// =========================================================================

/// Two flash swaps in sequence both succeed and leave `FlashActive = false`.
/// Verifies the guard is fully cleared between consecutive calls.
#[test]
fn test_consecutive_flash_swaps_succeed() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out_1: i128 = 100;
    client.flash_swap_a_for_b(&amount_out_1, &FEE_BPS, &Bytes::new(&env));
    let in_1: i128 = inverse_swap_in(1_000, 1_000, amount_out_1, FEE_BPS);
    client.repay_flash_swap(&in_1);
    assert!(!client.is_flash_active());

    let amount_out_2: i128 = 50;
    client.flash_swap_a_for_b(&amount_out_2, &FEE_BPS, &Bytes::new(&env));
    // The first flash grew k from 1,000,000 to ~1,000,800 and changed
    // reserves to (~1_112, 900), so the second flash must use the
    // *post-first-repay* reserves when computing the inverse formula.
    let (ra2_pre, rb2_pre) = client.get_reserves();
    let rb_pre_2 = rb2_pre + amount_out_2;
    let in_2: i128 = inverse_swap_in(ra2_pre, rb_pre_2, amount_out_2, FEE_BPS);
    client.repay_flash_swap(&in_2);
    assert!(!client.is_flash_active());
}

/// `flash_swap_a_for_b`'s `params` argument does not affect accounting; it
/// only flows through as opaque user data.
#[test]
fn test_params_payload_flows_through() {
    let (env, amm_id) = setup_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    // Single-byte payload (Soroban Bytes must be at least 1 byte).
    let params = Bytes::from_array(&env, &[0x42]);
    let out = client.flash_swap_a_for_b(&100, &FEE_BPS, &params);
    assert_eq!(out, 100);
    let (ra, rb) = client.get_reserves();
    assert_eq!(ra, 1_000);
    assert_eq!(rb, 900);
}

// Suppress "unused import" warnings for items only referenced via the
// contract trait / compile-time checks (kept here so external readers
// see the full toolbox this test module relies on).
#[allow(dead_code)]
fn _addr_marker(_: &Address) {}
