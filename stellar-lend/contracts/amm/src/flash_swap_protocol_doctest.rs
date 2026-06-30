//! Doc-tests that exercise the full flash-swap protocol as documented in
//! `FLASH_SWAP_PROTOCOL.md`.
//!
//! Each test maps to a numbered section of the spec so reviewers can trace
//! the documented claim directly to the executable verification.
//!
//! # Covered scenarios
//!
//! | Test | Spec section |
//! |------|--------------|
//! | `doc_test_full_sequence`          | Call Sequence + Worked Example §Steps 1-4 |
//! | `doc_test_under_repay_rollback`   | Failure and Rollback Semantics §Under-Repayment |
//! | `doc_test_reentrancy_guard`       | Reentrancy Guard §Blocked Operations |
//! | `doc_test_fee_zero_and_max`       | Edge Cases §fee_bps = 0 / fee_bps = 9 999 |

#![cfg(test)]

use crate::{inverse_swap_in, AmmContract, AmmContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Bytes, Env};

// ---------------------------------------------------------------------------
// Shared pool fixture
// ---------------------------------------------------------------------------

fn make_pool(ra: i128, rb: i128) -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    AmmContractClient::new(&env, &id).init_pool(&ra, &rb);
    (env, id)
}

// ---------------------------------------------------------------------------
// §Call Sequence + §Worked Example
// ---------------------------------------------------------------------------

/// Exercises the full documented happy-path sequence:
///
/// ```text
/// Op 1: flash_swap_a_for_b(amount_out=100, fee_bps=30)
///         k_before  = 1_000 × 1_000 = 1_000_000
///         reserve_b = 1_000 − 100   =     900
/// Op 3: repay_flash_swap(amount_in = amount_in_min)
///         amount_in_min = ⌈1_000 × 100 / 900⌉ = 112
///         k_after   = (1_000 + 112) × 900 = 1_000_800 ≥ 1_000_000  ✓
/// ```
#[test]
fn doc_test_full_sequence() {
    let (env, amm_id) = make_pool(1_000, 1_000);
    let client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 100;
    let fee_bps: i128 = 30;

    // ---- Op 1: optimistic debit ----
    assert!(!client.is_flash_active(), "guard off before flash");
    let returned = client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
    assert_eq!(returned, amount_out, "return value must equal amount_out");

    // reserve_b must be debited; reserve_a untouched.
    let (ra, rb) = client.get_reserves();
    assert_eq!(ra, 1_000, "reserve_a not touched by Op 1");
    assert_eq!(rb, 900, "reserve_b debited by amount_out=100");
    assert!(client.is_flash_active(), "guard armed after Op 1");

    // ---- §Minimum Repayment Formula verification ----
    // amount_in_min = ⌈1_000 × 100 / 900⌉ = 112
    let amount_in = inverse_swap_in(1_000, 1_000, amount_out, fee_bps);
    assert_eq!(
        amount_in, 112,
        "inverse formula gives 112 for worked example"
    );

    // ---- Op 3: repayment + verify-k ----
    client.repay_flash_swap(&amount_in);

    let (ra_new, rb_new) = client.get_reserves();
    let k_before: i128 = 1_000 * 1_000;
    let k_after: i128 = ra_new * rb_new;

    assert!(
        k_after >= k_before,
        "k must be non-decreasing after repay (k_before={k_before}, k_after={k_after})"
    );
    // Worked example: k_after = 1_112 × 900 = 1_000_800
    assert_eq!(k_after, 1_000_800, "k_after matches worked-example value");

    assert!(!client.is_flash_active(), "guard cleared after Op 3");
    assert_eq!(ra_new, 1_112, "reserve_a = 1_000 + 112");
    assert_eq!(rb_new, 900, "reserve_b unchanged from the debit");
}

// ---------------------------------------------------------------------------
// §Failure and Rollback Semantics — Under-Repayment
// ---------------------------------------------------------------------------

/// A `ProxyContract` is required to simulate a Soroban multi-operation
/// transaction in tests: both the debit (Op 1) and the failing repay (Op 3)
/// must live inside a single host invocation for Soroban's atomic rollback to
/// cover both writes.
///
/// When the proxy panics (due to under-repayment), the host rolls back every
/// storage write — including the optimistic reserve_b debit.
#[contract]
pub struct DocProxyContract;

#[contractimpl]
impl DocProxyContract {
    pub fn do_flash_and_repay(env: Env, amm: Address, amount_out: i128, amount_in: i128) {
        let client = AmmContractClient::new(&env, &amm);
        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
        client.repay_flash_swap(&amount_in);
    }
}

/// Verifies §Under-Repayment rollback:
///
/// Under-paying by 1 stroop triggers `"Invariant violation: k decreased"`.
/// Soroban rolls back every storage change — reserves are fully restored and
/// `FlashActive` is cleared.
///
/// Worked-example parallel:
/// ```text
/// amount_in = 111  (one stroop short of 112)
/// k_after   = (1_000 + 111) × 900 = 999_900 < 1_000_000  → PANIC + rollback
/// ```
#[test]
fn doc_test_under_repay_rollback() {
    let (env, amm_id) = make_pool(1_000, 1_000);
    let amm_client = AmmContractClient::new(&env, &amm_id);

    let amount_out: i128 = 100;
    let amount_in_min: i128 = inverse_swap_in(1_000, 1_000, amount_out, 30);
    assert_eq!(amount_in_min, 112);
    let under_in: i128 = amount_in_min - 1; // 111

    let proxy_id = env.register(DocProxyContract, ());
    let proxy = DocProxyContractClient::new(&env, &proxy_id);

    let result = proxy.try_do_flash_and_repay(&amm_id, &amount_out, &under_in);
    assert!(
        result.is_err(),
        "under-repay must return Err (panic captured)"
    );

    // Full atomicity: both writes rolled back.
    let (ra, rb) = amm_client.get_reserves();
    assert_eq!(ra, 1_000, "reserve_a fully restored by rollback");
    assert_eq!(
        rb, 1_000,
        "reserve_b fully restored (optimistic debit undone)"
    );
    assert!(
        !amm_client.is_flash_active(),
        "FlashActive must be false after rollback"
    );
}

// ---------------------------------------------------------------------------
// §Reentrancy Guard — Blocked Operations
// ---------------------------------------------------------------------------

/// Verifies that the four guarded state-mutating entry points are blocked while
/// a flash swap is in flight, each with the documented panic message:
///
/// `"ReentrantFlashSwap: pool mutation blocked while flash-swap is in flight"`
///
/// Operations tested (those that call `assert_no_active_flash_swap`):
/// - `add_liquidity`        (I-Mut-1)
/// - `remove_liquidity`     (I-Mut-2)
/// - `swap_a_for_b`         (I-Mut-3)
/// - `flash_swap_a_for_b`   (I-Mut-4 — nested flash blocked)
#[test]
fn doc_test_reentrancy_guard() {
    // add_liquidity blocked
    {
        let (env, amm_id) = make_pool(1_000, 1_000);
        let client = AmmContractClient::new(&env, &amm_id);
        client.flash_swap_a_for_b(&100, &Bytes::new(&env));
        let result = client.try_add_liquidity(&1, &1);
        assert!(
            result.is_err(),
            "add_liquidity must be blocked while FlashActive"
        );
    }

    // remove_liquidity blocked
    {
        let (env, amm_id) = make_pool(1_000, 1_000);
        let client = AmmContractClient::new(&env, &amm_id);
        client.flash_swap_a_for_b(&100, &Bytes::new(&env));
        let result = client.try_remove_liquidity(&1, &1);
        assert!(
            result.is_err(),
            "remove_liquidity must be blocked while FlashActive"
        );
    }

    // swap_a_for_b blocked
    {
        let (env, amm_id) = make_pool(1_000, 1_000);
        let client = AmmContractClient::new(&env, &amm_id);
        client.flash_swap_a_for_b(&100, &Bytes::new(&env));
        let result = client.try_swap_a_for_b(&1);
        assert!(result.is_err(), "swap_a_for_b must be blocked while FlashActive");
    }

    // nested flash_swap_a_for_b blocked
    {
        let (env, amm_id) = make_pool(1_000, 1_000);
        let client = AmmContractClient::new(&env, &amm_id);
        client.flash_swap_a_for_b(&100, &Bytes::new(&env));
        let result = client.try_flash_swap_a_for_b(&1, &Bytes::new(&env));
        assert!(result.is_err(), "nested flash_swap_a_for_b must be blocked while FlashActive");
    }
}

// ---------------------------------------------------------------------------
// §Edge Cases — fee_bps = 0 and fee_bps = 9_999
// ---------------------------------------------------------------------------

/// Verifies the two fee-bps edge cases from the spec's Edge Cases table:
///
/// 1. `fee_bps = 0`: no fee discount; `inverse_swap_in` denominator is
///    `rb − amount_out` (not zero), k-check still applies.
/// 2. `fee_bps = 9_999`: maximum valid fee; flash-swap validates, the
///    same verify-k check runs.
#[test]
fn doc_test_fee_zero_and_max() {
    // fee_bps = 0
    {
        let (env, amm_id) = make_pool(1_000, 1_000);
        let client = AmmContractClient::new(&env, &amm_id);
        let amount_out: i128 = 100;

        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
        let amount_in = inverse_swap_in(1_000, 1_000, amount_out, 0);
        client.repay_flash_swap(&amount_in);

        let (ra, rb) = client.get_reserves();
        assert!(
            ra * rb >= 1_000 * 1_000,
            "fee=0: k-monotonicity must still hold"
        );
        assert!(
            !client.is_flash_active(),
            "fee=0: guard cleared after repay"
        );
    }

    // fee_bps = 9_999 (maximum valid)
    {
        let (env, amm_id) = make_pool(1_000, 1_000);
        let client = AmmContractClient::new(&env, &amm_id);
        let amount_out: i128 = 50;

        client.flash_swap_a_for_b(&amount_out, &Bytes::new(&env));
        let amount_in = inverse_swap_in(1_000, 1_000, amount_out, 9_999);
        client.repay_flash_swap(&amount_in);

        let (ra, rb) = client.get_reserves();
        assert!(
            ra * rb >= 1_000 * 1_000,
            "fee=9999: k-monotonicity must still hold"
        );
        assert!(
            !client.is_flash_active(),
            "fee=9999: guard cleared after repay"
        );
    }
}
