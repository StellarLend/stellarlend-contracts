//! Tests for bidirectional swap symmetry and k-monotonicity in the AMM.
//!
//! Issue #1111: `swap_b_for_a` must mirror `swap_a_for_b` with identical fee
//! and invariant guarantees so neither direction can be exploited.

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{AmmContract, AmmContractClient};

fn setup(ra: i128, rb: i128) -> (Env, AmmContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    client.init_pool(&ra, &rb).unwrap();
    let admin = Address::generate(&env);
    // SAFETY: the env lives for the duration of the test via the returned value
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client, admin)
}

// ---------------------------------------------------------------------------
// Basic correctness
// ---------------------------------------------------------------------------

#[test]
fn test_swap_b_for_a_returns_nonzero() {
    let (_env, client, _admin) = setup(10_000, 10_000);
    let out = client.swap_b_for_a(&1_000);
    assert!(out > 0, "expected positive output");
}

#[test]
fn test_swap_b_for_a_reduces_reserve_a() {
    let (_env, client, _admin) = setup(10_000, 10_000);
    client.swap_b_for_a(&1_000);
    let (ra, _rb) = client.get_reserves();
    assert!(ra < 10_000, "reserve_a must decrease after B→A swap");
}

#[test]
fn test_swap_b_for_a_increases_reserve_b() {
    let (_env, client, _admin) = setup(10_000, 10_000);
    client.swap_b_for_a(&1_000);
    let (_ra, rb) = client.get_reserves();
    assert!(rb > 10_000, "reserve_b must increase after B→A swap");
}

// ---------------------------------------------------------------------------
// k-monotonicity
// ---------------------------------------------------------------------------

#[test]
fn test_swap_b_for_a_k_monotonic() {
    let (_env, client, _admin) = setup(10_000, 10_000);
    let k_before = 10_000_i128 * 10_000;
    client.swap_b_for_a(&500);
    let (ra, rb) = client.get_reserves();
    assert!(ra * rb >= k_before, "k must not decrease after B→A swap");
}

#[test]
fn test_swap_a_for_b_k_monotonic_unchanged() {
    // Regression: existing path still satisfies invariant.
    let (_env, client, _admin) = setup(10_000, 10_000);
    let k_before = 10_000_i128 * 10_000;
    client.swap_a_for_b(&500);
    let (ra, rb) = client.get_reserves();
    assert!(ra * rb >= k_before);
}

// ---------------------------------------------------------------------------
// Round-trip: trader never profits net of fees
// ---------------------------------------------------------------------------

#[test]
fn test_round_trip_trader_does_not_profit() {
    // Start with 1 000 A. Swap A→B, then swap all B back to A.
    // After two fee-bearing swaps the trader must end with ≤ 1 000 A.
    let (_env, client, _admin) = setup(100_000, 100_000);
    let start_a = 1_000_i128;
    let b_out = client.swap_a_for_b(&start_a);
    assert!(b_out > 0);
    let a_back = client.swap_b_for_a(&b_out);
    assert!(
        a_back <= start_a,
        "round-trip profit impossible: started={}, ended={}",
        start_a,
        a_back
    );
}

#[test]
fn test_round_trip_k_monotonic() {
    let (_env, client, _admin) = setup(100_000, 100_000);
    let k_start = 100_000_i128 * 100_000;
    let b_out = client.swap_a_for_b(&1_000);
    let (ra1, rb1) = client.get_reserves();
    assert!(ra1 * rb1 >= k_start);
    client.swap_b_for_a(&b_out);
    let (ra2, rb2) = client.get_reserves();
    assert!(
        ra2 * rb2 >= k_start,
        "k must stay >= initial after round-trip"
    );
}

// ---------------------------------------------------------------------------
// Symmetry: equal reserves + equal amounts → equal outputs in both directions
// ---------------------------------------------------------------------------

#[test]
fn test_symmetric_output_equal_reserves() {
    // With a balanced pool, swapping X of A gives the same output as X of B.
    let env = Env::default();
    env.mock_all_auths();
    let id_ab = env.register(AmmContract, ());
    let id_ba = env.register(AmmContract, ());
    let c_ab = AmmContractClient::new(&env, &id_ab);
    let c_ba = AmmContractClient::new(&env, &id_ba);
    c_ab.init_pool(&50_000, &50_000).unwrap();
    c_ba.init_pool(&50_000, &50_000).unwrap();

    let out_ab = c_ab.swap_a_for_b(&1_000);
    let out_ba = c_ba.swap_b_for_a(&1_000);
    assert_eq!(out_ab, out_ba, "symmetric pool must give equal outputs");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "amount must be positive")]
fn test_swap_b_for_a_zero_amount_panics() {
    let (_env, client, _admin) = setup(10_000, 10_000);
    client.swap_b_for_a(&0);
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn test_swap_b_for_a_negative_amount_panics() {
    let (_env, client, _admin) = setup(10_000, 10_000);
    client.swap_b_for_a(&-1);
}

#[test]
#[should_panic(expected = "empty pool")]
fn test_swap_b_for_a_empty_pool_panics() {
    let (_env, client, _admin) = setup(0, 0);
    client.swap_b_for_a(&100);
}

#[test]
fn test_swap_b_for_a_zero_fee() {
    // Admin sets fee to 0 → output maximised (no fee deducted).
    let (_env, client, admin) = setup(10_000, 10_000);
    client.set_fee_bps(&admin, &0).unwrap();
    let out_zero_fee = client.swap_b_for_a(&1_000);

    // Compare with default-fee pool (30 bps).
    let (_env2, client2, _admin2) = setup(10_000, 10_000);
    let out_with_fee = client2.swap_b_for_a(&1_000);
    assert!(
        out_zero_fee >= out_with_fee,
        "zero-fee output must be >= fee output"
    );
}

#[test]
fn test_swap_b_for_a_max_fee_gives_reduced_output() {
    // Admin sets fee to MAX_FEE_BPS (5_000 = 50%) → output is much lower than with default fee.
    use crate::MAX_FEE_BPS;
    let (_env, client, admin) = setup(10_000, 10_000);
    client.set_fee_bps(&admin, &MAX_FEE_BPS).unwrap();
    let out_max_fee = client.swap_b_for_a(&1_000);

    let (_env2, client2, _admin2) = setup(10_000, 10_000);
    let out_default_fee = client2.swap_b_for_a(&1_000);
    assert!(
        out_max_fee < out_default_fee,
        "max fee output must be less than default fee output"
    );
}

#[test]
fn test_swap_b_for_a_dust_input_rounds_down() {
    // 1-unit input on a large pool: output must be 0 (floor division) or 1, never > input value.
    let (_env, client, _admin) = setup(1_000_000, 1_000_000);
    let out = client.swap_b_for_a(&1);
    assert!(out <= 1, "dust input must not produce more than 1 unit out");
}

// ---------------------------------------------------------------------------
// Fuzz-style sweep
// ---------------------------------------------------------------------------

#[test]
fn fuzz_swap_b_for_a_k_monotonic() {
    let reserve_sizes = [1_000_i128, 10_000, 100_000, 1_000_000];
    let amounts = [1_i128, 10, 100, 1_000, 10_000];

    for &ra in &reserve_sizes {
        for &rb in &reserve_sizes {
            for &amt in &amounts {
                if amt >= rb {
                    continue; // skip if amount_in would drain reserve_b entirely
                }
                let (_env, client, _admin) = setup(ra, rb);
                let _out = client.swap_b_for_a(&amt);
                let (new_ra, new_rb) = client.get_reserves();
                let k_before = ra.checked_mul(rb).unwrap();
                let k_after = new_ra.checked_mul(new_rb).unwrap();
                assert!(
                    k_after >= k_before,
                    "k decreased: ra={}, rb={}, amt={}, k_before={}, k_after={}",
                    ra,
                    rb,
                    amt,
                    k_before,
                    k_after
                );
            }
        }
    }
}
