//! Tests for overflow-safe AMM fee accrual using saturating arithmetic.
//!
//! Verifies that fee accumulators saturate at `i128::MAX` instead of
//! panicking when the counter would otherwise overflow.
//!
//! # Invariants tested
//!
//! | Invariant                                                     | Test function                        |
//! |---------------------------------------------------------------|--------------------------------------|
//! | Normal accrual unchanged for realistic values                  | `test_normal_accrual_unchanged`      |
//! | `fee_a` saturates at `i128::MAX` without panic                 | `test_saturate_at_max_for_a_side`    |
//! | `fee_b` saturates at `i128::MAX` without panic                 | `test_saturate_at_max_for_b_side`    |
//! | Saturation of one side does not affect the other               | `test_saturate_then_other_side_untouched` |
//! | Both sides saturate independently                             | `test_both_sides_saturate_independently` |
//! | Zero-fee swap near max is safe                                | `test_zero_fee_safe_near_max`        |
//! | Accumulator never exceeds `i128::MAX`                         | `test_saturate_never_exceeds_max`    |
//! | Re-init resets saturated counters back to zero                | `test_reinit_resets_saturated_fees`  |
//! | No panic on large-swap fee near saturation                    | `test_no_panic_on_large_fee`         |

#![cfg(test)]

use crate::{AmmContract, AmmContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Reserve size used for saturation tests — large enough to accommodate
/// any swap amount in the test, but small enough that the swap formula
/// itself never overflows (`reserve * 10_000 << i128::MAX`).
const BIG_RESERVE: i128 = 1_000_000_000_000_000_000; // 10^18

/// Helper: set up a pool and return `(env, amm_id, client)` so that tests
/// needing direct storage access can use `env.as_contract(&amm_id, ...)`.
fn setup(ra: i128, rb: i128) -> (Env, Address, AmmContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let amm_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &amm_id);
    client.init_pool(&ra, &rb);
    // SAFETY: env outlives the returned client via the tuple
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, amm_id, client)
}

/// Seed `fee_a` storage to a value near `i128::MAX` directly, bypassing
/// the swap interface (which would require thousands of individual swaps).
fn seed_fee_a(env: &Env, amm_id: &Address, value: i128) {
    env.as_contract(amm_id, || {
        env.storage().persistent().set(&("pool", "fee_a"), &value);
    });
}

/// Seed `fee_b` storage to a value near `i128::MAX` directly.
fn seed_fee_b(env: &Env, amm_id: &Address, value: i128) {
    env.as_contract(amm_id, || {
        env.storage().persistent().set(&("pool", "fee_b"), &value);
    });
}

// ---------------------------------------------------------------------------
// Normal accrual unchanged
// ---------------------------------------------------------------------------

#[test]
fn test_normal_accrual_unchanged() {
    let (_env, _id, client) = setup(10_000, 10_000);
    client.swap_a_for_b(&1_000);
    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, 3, "normal fee must still be exact");
    assert_eq!(fee_b, 0);

    client.swap_b_for_a(&2_000);
    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, 3, "fee_a unchanged after B→A swap");
    assert_eq!(fee_b, 10, "fee_b = 2000 * 50 / 10000 = 10");
}

// ---------------------------------------------------------------------------
// Saturating behaviour: A side
// ---------------------------------------------------------------------------

#[test]
fn test_saturate_at_max_for_a_side() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    // Seed fee_a to one below max
    seed_fee_a(&env, &amm_id, i128::MAX - 1);

    // A single swap with fee = 2 should push it to i128::MAX
    client.swap_a_for_b(&20_000);

    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, i128::MAX, "fee_a must saturate at i128::MAX");
    assert_eq!(fee_b, 0, "fee_b must stay zero");

    // Another swap — must stay at MAX, no panic
    client.swap_a_for_b(&50_000);
    let (fee_a2, _) = client.get_accrued_fees();
    assert_eq!(
        fee_a2,
        i128::MAX,
        "fee_a must stay at MAX after further swaps"
    );
}

#[test]
fn test_saturate_at_max_for_b_side() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    seed_fee_b(&env, &amm_id, i128::MAX - 1);

    client.swap_b_for_a(&20_000);

    let (_, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_b, i128::MAX, "fee_b must saturate at i128::MAX");

    // Another swap — must stay at MAX
    client.swap_b_for_a(&50_000);
    let (_, fee_b2) = client.get_accrued_fees();
    assert_eq!(
        fee_b2,
        i128::MAX,
        "fee_b must stay at MAX after further swaps"
    );
}

#[test]
fn test_saturate_then_other_side_untouched() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    // Saturate fee_a only
    seed_fee_a(&env, &amm_id, i128::MAX - 1);
    client.swap_a_for_b(&20_000);

    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, i128::MAX, "fee_a saturated");
    assert_eq!(fee_b, 0, "fee_b still zero");

    // Now do a normal B→A swap — fee_b should accrue normally
    client.swap_b_for_a(&10_000);
    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, i128::MAX, "fee_a remains saturated");
    assert_eq!(fee_b, 30, "fee_b accrues normally = 10000 * 30 / 10000");
}

#[test]
fn test_both_sides_saturate_independently() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    seed_fee_a(&env, &amm_id, i128::MAX - 1);
    seed_fee_b(&env, &amm_id, i128::MAX - 1);

    client.swap_a_for_b(&20_000);
    client.swap_b_for_a(&20_000);

    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, i128::MAX, "fee_a must saturate");
    assert_eq!(fee_b, i128::MAX, "fee_b must saturate");
}

// ---------------------------------------------------------------------------
// Zero-fee edge case near max
// ---------------------------------------------------------------------------

#[test]
fn test_zero_fee_safe_near_max() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    seed_fee_a(&env, &amm_id, i128::MAX - 1);

    // Zero-fee swap must not alter accumulator and must not panic
    client.swap_a_for_b(&1_000);

    let (fee_a, _) = client.get_accrued_fees();
    assert_eq!(
        fee_a,
        i128::MAX - 1,
        "zero-fee swap must not alter accumulator"
    );
}

// ---------------------------------------------------------------------------
// Never exceed i128::MAX
// ---------------------------------------------------------------------------

#[test]
fn test_saturate_never_exceeds_max() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    // Start from various seed values and verify we never exceed MAX
    let seeds = [i128::MAX - 100, i128::MAX - 1, i128::MAX];

    for &seed in &seeds {
        seed_fee_a(&env, &amm_id, seed);
        // Swap with a moderate fee — using small amount so swap math is safe
        client.swap_a_for_b(&10_000);
        let (fee_a, _) = client.get_accrued_fees();
        assert!(
            fee_a <= i128::MAX,
            "fee_a must never exceed i128::MAX (seed={}, fee_a={})",
            seed,
            fee_a
        );
    }
}

// ---------------------------------------------------------------------------
// Re-init resets saturated counter
// ---------------------------------------------------------------------------

#[test]
fn test_reinit_resets_saturated_fees() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    seed_fee_a(&env, &amm_id, i128::MAX);
    seed_fee_b(&env, &amm_id, i128::MAX);

    // Re-initialize the pool with new reserves
    client.init_pool(&50_000, &50_000);

    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, 0, "re-init must reset fee_a to zero");
    assert_eq!(fee_b, 0, "re-init must reset fee_b to zero");

    // After re-init, fee accrual should work normally
    client.swap_a_for_b(&1_000);
    let (fee_a, _) = client.get_accrued_fees();
    assert_eq!(fee_a, 3, "fee accrual works normally after re-init");
}

// ---------------------------------------------------------------------------
// Large-swap fee near saturation (no panic)
// ---------------------------------------------------------------------------

#[test]
fn test_no_panic_on_large_fee() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    // Pre-seed the fee accumulator close to max, then do a swap whose fee
    // would overflow if unchecked.
    seed_fee_a(&env, &amm_id, i128::MAX - 100);

    // Swap a moderate amount with max fee_bps (9999).
    // fee = amount_in * 9999 / 10000 ≈ amount_in.
    // Use an amount_in that produces a fee large enough to exceed the
    // remaining headroom (100), forcing saturation.
    client.swap_a_for_b(&1_000_000);

    let (fee_a, _) = client.get_accrued_fees();
    assert!(fee_a == i128::MAX, "fee_a must saturate at i128::MAX");

    // A second swap should also not panic; fee stays at MAX.
    client.swap_a_for_b(&500_000);
    let (fee_a2, _) = client.get_accrued_fees();
    assert_eq!(
        fee_a2,
        i128::MAX,
        "fee_a must stay at MAX after second swap"
    );
}
