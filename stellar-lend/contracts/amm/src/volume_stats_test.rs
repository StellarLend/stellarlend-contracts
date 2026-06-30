//! Tests for the AMM cumulative-volume / last-price observability view.
//!
//! Exercises [`AmmContract::get_volume_stats`] and the counters that
//! [`AmmContract::swap_a_for_b`] / [`AmmContract::swap_b_for_a`] update.
//!
//! # Invariants tested
//!
//! | Invariant                                                      | Test function                          |
//! |----------------------------------------------------------------|----------------------------------------|
//! | Fresh pool reports all-zero stats and undefined price          | `test_no_swaps_zero_stats`             |
//! | A single A→B swap records A-side volume and B-per-A price       | `test_single_swap_a_for_b`             |
//! | A single B→A swap records B-side volume and B-per-A price       | `test_single_swap_b_for_a`             |
//! | Volume accumulates across many swaps; sides are independent     | `test_cumulative_many_swaps`           |
//! | Last price reflects the most recent swap's direction            | `test_last_price_after_each_direction` |
//! | A-side volume saturates at `i128::MAX` without panic            | `test_volume_a_saturates_at_max`       |
//! | B-side volume saturates at `i128::MAX` without panic            | `test_volume_b_saturates_at_max`       |
//! | Saturation of one side leaves the other untouched               | `test_saturation_is_per_side`          |
//! | `init_pool` resets accumulated stats back to zero               | `test_reinit_resets_stats`             |
//! | A dust swap that yields zero output leaves the price unchanged   | `test_zero_output_swap_keeps_price`    |

#![cfg(test)]

use crate::{AmmContract, AmmContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Large reserves so the swap formula itself never overflows while we drive
/// the volume counters with sizeable inputs.
const BIG_RESERVE: i128 = 1_000_000_000_000_000_000; // 10^18

/// Set up a pool and return `(env, amm_id, client)`. Mirrors the helper used
/// by the fee-accrual overflow tests so direct storage seeding via
/// `env.as_contract(&amm_id, ...)` is available.
fn setup(ra: i128, rb: i128) -> (Env, Address, AmmContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let amm_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &amm_id);
    client.init_pool(&ra, &rb);
    // SAFETY: env outlives the returned client via the tuple.
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, amm_id, client)
}

/// Seed the A-side cumulative volume counter directly, bypassing the swap
/// interface (reaching `i128::MAX` through real swaps is infeasible).
fn seed_vol_a(env: &Env, amm_id: &Address, value: i128) {
    env.as_contract(amm_id, || {
        env.storage().persistent().set(&("pool", "vol_a"), &value);
    });
}

/// Seed the B-side cumulative volume counter directly.
fn seed_vol_b(env: &Env, amm_id: &Address, value: i128) {
    env.as_contract(amm_id, || {
        env.storage().persistent().set(&("pool", "vol_b"), &value);
    });
}

// ---------------------------------------------------------------------------
// No swaps → zero stats
// ---------------------------------------------------------------------------

#[test]
fn test_no_swaps_zero_stats() {
    let (_env, _id, client) = setup(10_000, 10_000);

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_a_in, 0);
    assert_eq!(stats.cumulative_volume_b_in, 0);
    // denom == 0 is the "no priced swap yet" sentinel.
    assert_eq!(stats.last_price_num, 0);
    assert_eq!(stats.last_price_denom, 0);
}

// ---------------------------------------------------------------------------
// Single swap, each side
// ---------------------------------------------------------------------------

#[test]
fn test_single_swap_a_for_b() {
    let (_env, _id, client) = setup(1_000_000, 1_000_000);

    let amount_in = 10_000_i128;
    let out = client.swap_a_for_b(&amount_in);
    // Exact Uniswap-v2 output for a balanced 1e6/1e6 pool at 30 bps; this is
    // the value reproduced in VOLUME_STATS.md's worked example.
    assert_eq!(out, 9_871, "deterministic swap output");

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_a_in, amount_in, "A-side volume tracked");
    assert_eq!(stats.cumulative_volume_b_in, 0, "B-side untouched");
    // Price of A in B = amount_out (B) / amount_in (A).
    assert_eq!(stats.last_price_num, out, "num == B out");
    assert_eq!(stats.last_price_denom, amount_in, "denom == A in");
}

#[test]
fn test_single_swap_b_for_a() {
    let (_env, _id, client) = setup(1_000_000, 1_000_000);

    let amount_in = 10_000_i128;
    let out = client.swap_b_for_a(&amount_in);

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_b_in, amount_in, "B-side volume tracked");
    assert_eq!(stats.cumulative_volume_a_in, 0, "A-side untouched");
    // Same B-per-A convention: amount_in (B) / amount_out (A).
    assert_eq!(stats.last_price_num, amount_in, "num == B in");
    assert_eq!(stats.last_price_denom, out, "denom == A out");
}

// ---------------------------------------------------------------------------
// Cumulative across many swaps
// ---------------------------------------------------------------------------

#[test]
fn test_cumulative_many_swaps() {
    let (_env, _id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    let mut expected_a = 0_i128;
    let mut expected_b = 0_i128;
    for i in 1..=5_i128 {
        let a_in = 1_000 * i;
        let b_in = 700 * i;
        client.swap_a_for_b(&a_in);
        client.swap_b_for_a(&b_in);
        expected_a += a_in;
        expected_b += b_in;
    }

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_a_in, expected_a);
    assert_eq!(stats.cumulative_volume_b_in, expected_b);
}

// ---------------------------------------------------------------------------
// Last price reflects the most recent direction
// ---------------------------------------------------------------------------

#[test]
fn test_last_price_after_each_direction() {
    let (_env, _id, client) = setup(1_000_000, 1_000_000);

    // A→B last (matches VOLUME_STATS.md worked example, step 1).
    let out_ab = client.swap_a_for_b(&10_000);
    assert_eq!(out_ab, 9_871, "deterministic A→B output");
    let stats = client.get_volume_stats();
    assert_eq!(stats.last_price_num, out_ab);
    assert_eq!(stats.last_price_denom, 10_000);

    // B→A last on the now-shifted pool — convention stays B-per-A but
    // operands swap roles (worked example, step 2).
    let out_ba = client.swap_b_for_a(&10_000);
    assert_eq!(out_ba, 10_068, "deterministic B→A output on shifted pool");
    let stats = client.get_volume_stats();
    assert_eq!(stats.last_price_num, 10_000);
    assert_eq!(stats.last_price_denom, out_ba);
}

// ---------------------------------------------------------------------------
// Saturation at the extreme
// ---------------------------------------------------------------------------

#[test]
fn test_volume_a_saturates_at_max() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    seed_vol_a(&env, &amm_id, i128::MAX - 1);
    // amount_in (20_000) overshoots the remaining headroom of 1.
    client.swap_a_for_b(&20_000);

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_a_in, i128::MAX, "A volume saturates");

    // A further swap must stay pinned and must not panic.
    client.swap_a_for_b(&50_000);
    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_a_in, i128::MAX, "stays at MAX");
}

#[test]
fn test_volume_b_saturates_at_max() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    seed_vol_b(&env, &amm_id, i128::MAX - 1);
    client.swap_b_for_a(&20_000);

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_b_in, i128::MAX, "B volume saturates");

    client.swap_b_for_a(&50_000);
    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_b_in, i128::MAX, "stays at MAX");
}

#[test]
fn test_saturation_is_per_side() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    seed_vol_a(&env, &amm_id, i128::MAX);
    client.swap_b_for_a(&10_000);

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_a_in, i128::MAX, "A stays saturated");
    assert_eq!(stats.cumulative_volume_b_in, 10_000, "B accrues normally");
}

// ---------------------------------------------------------------------------
// Re-init resets stats
// ---------------------------------------------------------------------------

#[test]
fn test_reinit_resets_stats() {
    let (env, amm_id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    client.swap_a_for_b(&10_000);
    client.swap_b_for_a(&10_000);
    seed_vol_a(&env, &amm_id, i128::MAX);

    client.init_pool(&50_000, &50_000);

    let stats = client.get_volume_stats();
    assert_eq!(stats.cumulative_volume_a_in, 0, "A volume reset");
    assert_eq!(stats.cumulative_volume_b_in, 0, "B volume reset");
    assert_eq!(stats.last_price_num, 0, "price reset");
    assert_eq!(stats.last_price_denom, 0, "price reset");
}

// ---------------------------------------------------------------------------
// Dust swap with zero output leaves price unchanged
// ---------------------------------------------------------------------------

#[test]
fn test_zero_output_swap_keeps_price() {
    let (_env, _id, client) = setup(BIG_RESERVE, BIG_RESERVE);

    // A 1-unit input against a 10^18 pool floors to zero output.
    let out = client.swap_a_for_b(&1);
    assert_eq!(out, 0, "dust swap rounds output to zero");

    let stats = client.get_volume_stats();
    // Volume still counts the input that flowed in...
    assert_eq!(stats.cumulative_volume_a_in, 1, "volume still tracked");
    // ...but the price is not blanked to a zero denominator.
    assert_eq!(stats.last_price_num, 0, "price untouched (no priced swap yet)");
    assert_eq!(stats.last_price_denom, 0, "denominator stays at sentinel 0");
}
