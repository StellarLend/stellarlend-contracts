//! Dust-swap guard tests.
//!
//! Covers the zero-output rejection and optional admin `min_swap_in` floor
//! on `swap_a_for_b`, `swap_b_for_a`, and `flash_swap_a_for_b`.
//!
//! | Invariant                                              | Test function                                      |
//! |--------------------------------------------------------|----------------------------------------------------|
//! | Smallest zero-output input is rejected (A→B)           | `test_swap_a_for_b_rejects_zero_output_dust`       |
//! | Smallest one-unit output is allowed (A→B)              | `test_swap_a_for_b_allows_smallest_nonzero_out`    |
//! | Zero-output rejection on B→A                           | `test_swap_b_for_a_rejects_zero_output_dust`       |
//! | Smallest one-unit output is allowed (B→A)              | `test_swap_b_for_a_allows_smallest_nonzero_out`    |
//! | Reserves unchanged after rejected dust swap            | `test_zero_output_does_not_mutate_reserves`        |
//! | Admin `min_swap_in` rejects below-floor inputs         | `test_min_swap_in_floor_rejects_below`             |
//! | Admin `min_swap_in` allows at-floor inputs             | `test_min_swap_in_floor_allows_at_or_above`        |
//! | Default min_swap_in is 0 (disabled)                    | `test_default_min_swap_in_is_zero`                 |
//! | Flash path applies min_swap_in to amount_out           | `test_flash_swap_respects_min_swap_in`             |
//! | Flash path still accepts amount_out >= 1 by default    | `test_flash_swap_allows_unit_amount_out`           |
//! | Error codes 15/16 are stable                           | `test_dust_error_code_stability`                   |
//! | Legitimate non-dust swaps are unchanged                | `test_nonzero_swap_output_unchanged`               |

#![cfg(test)]

use crate::{AmmContract, AmmContractClient, AmmPoolError, DEFAULT_MIN_SWAP_IN};
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

fn setup_pool(ra: i128, rb: i128) -> (Env, AmmContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let admin = Address::generate(&env);
    client.init_pool(&ra, &rb, &token_a, &token_b);
    // Zero fee so the dust boundary is purely floor-division, not fee noise.
    client.set_fee_bps(&admin, &0_i128);
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client, admin)
}

// ---------------------------------------------------------------------------
// Zero-output rejection — A → B
// ---------------------------------------------------------------------------

#[test]
fn test_swap_a_for_b_rejects_zero_output_dust() {
    // With equal reserves R and fee=0:
    //   amount_out = amount_in * R / (R + amount_in)
    // amount_in = 1 → amount_out = R/(R+1) floors to 0 for any R >= 1.
    let (_env, client, _admin) = setup_pool(1_000_000, 1_000_000);
    let res = client.try_swap_a_for_b(&1_i128);
    assert_eq!(
        res,
        Err(Ok(AmmPoolError::ZeroOutput)),
        "amount_in=1 must floor to zero output and be rejected"
    );
}

#[test]
fn test_swap_a_for_b_allows_smallest_nonzero_out() {
    // amount_in such that amount_out floors to exactly 1:
    //   amount_in * R / (R + amount_in) >= 1
    //   amount_in * R >= R + amount_in
    //   amount_in * (R - 1) >= R
    //   amount_in >= ceil(R / (R - 1)) = 2 for R > 1
    // With R=1_000_000, amount_in=2:
    //   out = 2*1e6 / (1e6+2) = 2_000_000/1_000_002 = 1
    let (_env, client, _admin) = setup_pool(1_000_000, 1_000_000);
    let out = client.swap_a_for_b(&2_i128);
    assert_eq!(out, 1, "amount_in=2 must yield exactly 1 unit of B");
}

// ---------------------------------------------------------------------------
// Zero-output rejection — B → A
// ---------------------------------------------------------------------------

#[test]
fn test_swap_b_for_a_rejects_zero_output_dust() {
    let (_env, client, _admin) = setup_pool(1_000_000, 1_000_000);
    let res = client.try_swap_b_for_a(&1_i128);
    assert_eq!(res, Err(Ok(AmmPoolError::ZeroOutput)));
}

#[test]
fn test_swap_b_for_a_allows_smallest_nonzero_out() {
    let (_env, client, _admin) = setup_pool(1_000_000, 1_000_000);
    let out = client.swap_b_for_a(&2_i128);
    assert_eq!(out, 1, "amount_in=2 must yield exactly 1 unit of A");
}

// ---------------------------------------------------------------------------
// No state mutation on rejection
// ---------------------------------------------------------------------------

#[test]
fn test_zero_output_does_not_mutate_reserves() {
    let (_env, client, _admin) = setup_pool(50_000, 50_000);
    let (ra0, rb0) = client.get_reserves();
    let fees0 = client.get_accrued_fees();

    let res = client.try_swap_a_for_b(&1_i128);
    assert_eq!(res, Err(Ok(AmmPoolError::ZeroOutput)));

    let (ra1, rb1) = client.get_reserves();
    let fees1 = client.get_accrued_fees();
    assert_eq!(
        (ra1, rb1),
        (ra0, rb0),
        "reserves must be unchanged after dust reject"
    );
    assert_eq!(
        fees1, fees0,
        "fee accumulators must be unchanged after dust reject"
    );
}

// ---------------------------------------------------------------------------
// Admin min_swap_in floor
// ---------------------------------------------------------------------------

#[test]
fn test_default_min_swap_in_is_zero() {
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    assert_eq!(client.get_min_swap_in(), DEFAULT_MIN_SWAP_IN);
    assert_eq!(client.get_min_swap_in(), 0);
}

#[test]
fn test_min_swap_in_floor_rejects_below() {
    let (_env, client, admin) = setup_pool(1_000_000, 1_000_000);
    // Floor at 100; amount_in=50 is below even though it would produce nonzero out.
    client.set_min_swap_in(&admin, &100_i128);
    assert_eq!(client.get_min_swap_in(), 100);

    let res = client.try_swap_a_for_b(&50_i128);
    assert_eq!(res, Err(Ok(AmmPoolError::AmountBelowMinSwapIn)));

    let res_b = client.try_swap_b_for_a(&99_i128);
    assert_eq!(res_b, Err(Ok(AmmPoolError::AmountBelowMinSwapIn)));
}

#[test]
fn test_min_swap_in_floor_allows_at_or_above() {
    let (_env, client, admin) = setup_pool(1_000_000, 1_000_000);
    client.set_min_swap_in(&admin, &100_i128);

    let out = client.swap_a_for_b(&100_i128);
    assert!(out > 0, "at-floor input must succeed with nonzero out");

    let out_b = client.swap_b_for_a(&150_i128);
    assert!(out_b > 0, "above-floor input must succeed with nonzero out");
}

#[test]
fn test_set_min_swap_in_rejects_negative() {
    let (_env, client, admin) = setup_pool(10_000, 10_000);
    let res = client.try_set_min_swap_in(&admin, &(-1_i128));
    assert_eq!(res, Err(Ok(AmmPoolError::NonPositiveAmount)));
}

// ---------------------------------------------------------------------------
// Flash-swap path
// ---------------------------------------------------------------------------

#[test]
fn test_flash_swap_allows_unit_amount_out() {
    // Default min_swap_in = 0 → any positive amount_out is fine.
    let (env, client, _admin) = setup_pool(1_000, 1_000);
    let out = client.flash_swap_a_for_b(&1_i128, &Bytes::new(&env));
    assert_eq!(out, 1);
}

#[test]
fn test_flash_swap_respects_min_swap_in() {
    let (env, client, admin) = setup_pool(1_000, 1_000);
    client.set_min_swap_in(&admin, &50_i128);

    let res = client.try_flash_swap_a_for_b(&10_i128, &Bytes::new(&env));
    assert_eq!(
        res,
        Err(Ok(AmmPoolError::AmountBelowMinSwapIn)),
        "flash amount_out below min_swap_in must be rejected"
    );

    // At the floor the flash opens successfully.
    let out = client.flash_swap_a_for_b(&50_i128, &Bytes::new(&env));
    assert_eq!(out, 50);
}

// ---------------------------------------------------------------------------
// Error-code stability + legitimate swap regression
// ---------------------------------------------------------------------------

#[test]
fn test_dust_error_code_stability() {
    assert_eq!(AmmPoolError::ZeroOutput as u32, 15);
    assert_eq!(AmmPoolError::AmountBelowMinSwapIn as u32, 16);
}

#[test]
fn test_nonzero_swap_output_unchanged() {
    // A mid-size swap must still produce the same output as before the guard.
    // fee=0, ra=rb=10_000, amount_in=1_000:
    //   out = 1000*10000 / (10000+1000) = 10_000_000 / 11_000 = 909
    let (_env, client, _admin) = setup_pool(10_000, 10_000);
    let out = client.swap_a_for_b(&1_000_i128);
    assert_eq!(
        out, 909,
        "non-dust swap output must be unchanged by the guard"
    );
}
