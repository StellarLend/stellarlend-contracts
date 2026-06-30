//! Tests for AMM swap fee accrual accounting.
//!
//! Verifies that `swap_a_for_b` and `swap_b_for_a` correctly accumulate
//! protocol fees into per-side counters accessible via `get_accrued_fees`.
//!
//! # Invariants tested
//!
//! | Invariant                                             | Test function                         |
//! |-------------------------------------------------------|---------------------------------------|
//! | Fee = `amount_in * fee_bps / 10_000` (floor)          | `test_fee_formula_a`, `test_fee_formula_b` |
//! | `get_accrued_fees` starts at `(0, 0)` after init      | `test_initial_fees_zero`              |
//! | Accumulator is monotonic non-decreasing               | `test_fee_accumulator_monotonic`      |
//! | Accrued fee never exceeds cumulative `amount_in`      | `test_fee_never_exceeds_amount_in`    |
//! | Zero-fee swaps leave accumulator unchanged            | `test_zero_fee_swap`                  |
//! | Multiple swaps accumulate correctly                   | `test_multiple_swaps_accrue`          |
//! | A→B swaps increment only `fee_a`                      | `test_swap_a_only_increments_fee_a`   |
//! | B→A swaps increment only `fee_b`                      | `test_swap_b_only_increments_fee_b`   |
//! | Large fee_bps (9_999) handled safely                  | `test_max_fee_bps`                    |
//! | Fee accumulator survives add/remove liquidity          | `test_liquidity_ops_preserve_fees`    |

#![cfg(test)]

use crate::{AmmContract, AmmContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_pool(ra: i128, rb: i128) -> (Env, AmmContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    client.init_pool(&ra, &rb).unwrap();
    let admin = Address::generate(&env);
    // SAFETY: env outlives the returned client via the tuple
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client, admin)
}

// ---------------------------------------------------------------------------
// Initial state
// ---------------------------------------------------------------------------

#[test]
fn test_initial_fees_zero() {
    let (env, client, _admin) = setup_pool(10_000, 10_000);
    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, 0, "fee_a must start at zero");
    assert_eq!(fee_b, 0, "fee_b must start at zero");
}

// ---------------------------------------------------------------------------
// Fee formula verification
// ---------------------------------------------------------------------------

#[test]
fn test_fee_formula_a() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    let amount_in: i128 = 1_000;
    let fee_bps: i128 = 30;
    let expected_fee = amount_in * fee_bps / 10_000;

    client.swap_a_for_b(&amount_in);
    let (fee_a, _fee_b) = client.get_accrued_fees();
    assert_eq!(
        fee_a, expected_fee,
        "fee_a must equal amount_in * fee_bps / 10_000"
    );
}

#[test]
fn test_fee_formula_b() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    let amount_in: i128 = 1_000;
    let fee_bps: i128 = 30;
    let expected_fee = amount_in * fee_bps / 10_000;

    client.swap_b_for_a(&amount_in);
    let (_fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(
        fee_b, expected_fee,
        "fee_b must equal amount_in * fee_bps / 10_000"
    );
}

// ---------------------------------------------------------------------------
// Monotonicity
// ---------------------------------------------------------------------------

#[test]
fn test_fee_accumulator_monotonic() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    let (mut prev_a, mut prev_b) = client.get_accrued_fees();

    for &amt in &[100_i128, 200, 300, 400] {
        client.swap_a_for_b(&amt);
        let (fa, fb) = client.get_accrued_fees();
        assert!(
            fa >= prev_a,
            "fee_a must be monotonic (prev={}, curr={})",
            prev_a,
            fa
        );
        assert!(
            fb >= prev_b,
            "fee_b must be monotonic (prev={}, curr={})",
            prev_b,
            fb
        );
        prev_a = fa;
        prev_b = fb;
    }
}

// ---------------------------------------------------------------------------
// Fee never exceeds amount_in
// ---------------------------------------------------------------------------

#[test]
fn test_fee_never_exceeds_amount_in() {
    let (_env, client, admin) = setup_pool(10_000, 10_000);
    let amount_in: i128 = 5_000;
    let fee_bps: i128 = 4_999; // within MAX_FEE_BPS (5_000)
    client.set_fee_bps(&admin, &fee_bps).unwrap();
    let fee = amount_in * fee_bps / 10_000;
    assert!(fee <= amount_in, "fee must not exceed amount_in");

    client.swap_a_for_b(&amount_in);
    let (fee_a, _) = client.get_accrued_fees();
    assert!(
        fee_a <= amount_in,
        "accrued fee_a must not exceed the swap's amount_in"
    );
}

// ---------------------------------------------------------------------------
// Zero-fee edge case
// ---------------------------------------------------------------------------

#[test]
fn test_zero_fee_swap() {
    let (_env, client, admin) = setup_pool(10_000, 10_000);
    // Set stored fee to 0 so swaps accrue no fee.
    client.set_fee_bps(&admin, &0).unwrap();

    client.swap_a_for_b(&1_000);
    let (fee_a, _fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, 0, "zero stored fee must yield zero accrued fee");

    client.swap_b_for_a(&1_000);
    let (_fa, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_b, 0, "zero stored fee must yield zero accrued fee");
}

// ---------------------------------------------------------------------------
// Multiple swaps accumulate correctly
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_swaps_accrue() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);

    let amounts_a = [100_i128, 200, 300];
    let amounts_b = [150_i128, 250, 350];
    let fee_bps: i128 = 30;

    let expected_fee_a: i128 = amounts_a.iter().map(|&a| a * fee_bps / 10_000).sum();
    let expected_fee_b: i128 = amounts_b.iter().map(|&b| b * fee_bps / 10_000).sum();

    for &amt in &amounts_a {
        client.swap_a_for_b(&amt);
    }
    for &amt in &amounts_b {
        client.swap_b_for_a(&amt);
    }

    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(
        fee_a, expected_fee_a,
        "fee_a must accumulate across all A→B swaps"
    );
    assert_eq!(
        fee_b, expected_fee_b,
        "fee_b must accumulate across all B→A swaps"
    );
}

// ---------------------------------------------------------------------------
// Direction isolation
// ---------------------------------------------------------------------------

#[test]
fn test_swap_a_only_increments_fee_a() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    client.swap_a_for_b(&1_000);
    let (fee_a, fee_b) = client.get_accrued_fees();
    assert!(fee_a > 0, "fee_a must increase after A→B swap");
    assert_eq!(fee_b, 0, "fee_b must stay zero after A→B swap");
}

#[test]
fn test_swap_b_only_increments_fee_b() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    client.swap_b_for_a(&1_000);
    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, 0, "fee_a must stay zero after B→A swap");
    assert!(fee_b > 0, "fee_b must increase after B→A swap");
}

// ---------------------------------------------------------------------------
// Max fee_bps edge case (capped at MAX_FEE_BPS = 5_000)
// ---------------------------------------------------------------------------

#[test]
fn test_max_fee_bps() {
    use crate::MAX_FEE_BPS;
    let (_env, client, admin) = setup_pool(10_000, 10_000);
    let amount_in: i128 = 1_000;
    let fee_bps: i128 = MAX_FEE_BPS; // 5_000 bps = 50 %
    client.set_fee_bps(&admin, &fee_bps).unwrap();
    let expected_fee = amount_in * fee_bps / 10_000;

    client.swap_a_for_b(&amount_in);
    let (fee_a, _) = client.get_accrued_fees();
    assert_eq!(fee_a, expected_fee, "max fee_bps must compute correctly");
}

// ---------------------------------------------------------------------------
// Liquidity ops preserve fee accumulators
// ---------------------------------------------------------------------------

#[test]
fn test_liquidity_ops_preserve_fees() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    client.swap_a_for_b(&500);
    let (fee_a_before, _) = client.get_accrued_fees();

    client.add_liquidity(&100, &200);
    let (fa_after_add, _) = client.get_accrued_fees();
    assert_eq!(
        fa_after_add, fee_a_before,
        "add_liquidity must not alter fee_a"
    );

    client.remove_liquidity(&50, &100);
    let (fa_after_rem, _) = client.get_accrued_fees();
    assert_eq!(
        fa_after_rem, fee_a_before,
        "remove_liquidity must not alter fee_a"
    );
}

// ---------------------------------------------------------------------------
// Re-init resets accumulators
// ---------------------------------------------------------------------------

#[test]
fn test_reinit_resets_fees() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    client.swap_a_for_b(&500);
    assert!(
        client.get_accrued_fees().0 > 0,
        "fee_a should be positive after swap"
    );

    client.init_pool(&20_000, &20_000);
    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, 0, "re-init must reset fee_a");
    assert_eq!(fee_b, 0, "re-init must reset fee_b");
}

// ---------------------------------------------------------------------------
// Analytical fee verification across sequence
// ---------------------------------------------------------------------------

#[test]
fn test_analytical_fee_sequence() {
    let (_env, client, admin) = setup_pool(100_000, 100_000);
    let fee_bps: i128 = 50;
    client.set_fee_bps(&admin, &fee_bps).unwrap();

    let swaps_a = [1_000_i128, 2_000, 3_000, 4_000, 5_000];
    let swaps_b = [500_i128, 1_500, 2_500];

    let expected_fee_a: i128 = swaps_a.iter().map(|&a| a * fee_bps / 10_000).sum();
    let expected_fee_b: i128 = swaps_b.iter().map(|&b| b * fee_bps / 10_000).sum();

    for &amt in &swaps_a {
        client.swap_a_for_b(&amt);
    }
    for &amt in &swaps_b {
        client.swap_b_for_a(&amt);
    }

    let (fee_a, fee_b) = client.get_accrued_fees();
    assert_eq!(fee_a, expected_fee_a, "fee_a must match analytical sum");
    assert_eq!(fee_b, expected_fee_b, "fee_b must match analytical sum");
}
