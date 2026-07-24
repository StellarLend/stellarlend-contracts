//! Tests for `get_swap_quote` — the read-only constant-product swap quotation.
//!
//! Coverage targets:
//! - Quote output matches a live `swap_a_for_b` to the unit (A→B direction).
//! - Quote output matches a live `swap_b_for_a` to the unit (B→A direction).
//! - Zero reserves returns `AmmPoolError::EmptyPool`, not a panic.
//! - Large amount near pool depletion behaves correctly (no panic, sensible output).
//! - Quoted fee matches `compute_fee` output directly.

#![cfg(test)]

use crate::{AmmContract, AmmContractClient, AmmPoolError, DEFAULT_FEE_BPS};
use soroban_sdk::{testutils::Address as _, Address, Env};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register a fresh AMM, initialise it with the given reserves, and return the
/// env + client.  The client lifetime is transmuted to `'static` using the same
/// pattern as every other test module in this crate.
fn setup(ra: i128, rb: i128) -> (Env, AmmContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    client.init_pool(&ra, &rb).unwrap();
    // SAFETY: env is returned alongside the client and outlives this call.
    let client: AmmContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client)
}

// ---------------------------------------------------------------------------
// Quote matches live swap — A→B direction
// ---------------------------------------------------------------------------

/// The projected `amount_out` from `get_swap_quote` must equal the actual
/// output produced by `swap_a_for_b` on an identical pool state.
#[test]
fn test_quote_matches_live_swap_a_for_b() {
    let ra: i128 = 1_000_000;
    let rb: i128 = 2_000_000;
    let amount_in: i128 = 50_000;

    // Get the quote first (read-only — does not mutate state).
    let (_env, client) = setup(ra, rb);
    let fee_bps = client.get_fee_bps();
    let quote = client.get_swap_quote(&amount_in, &fee_bps, &true).unwrap();

    // Execute the live swap on the same pool.
    let live_out = client.swap_a_for_b(&amount_in);

    assert_eq!(
        quote.amount_out, live_out,
        "quote amount_out must match live swap output to the unit (A→B)"
    );

    // Reserves after live swap must match the quote's projected reserves.
    let (live_ra, live_rb) = client.get_reserves();
    assert_eq!(
        quote.reserve_a_after, live_ra,
        "projected reserve_a must match post-swap reserve_a"
    );
    assert_eq!(
        quote.reserve_b_after, live_rb,
        "projected reserve_b must match post-swap reserve_b"
    );
}

// ---------------------------------------------------------------------------
// Quote matches live swap — B→A direction
// ---------------------------------------------------------------------------

/// The projected `amount_out` from `get_swap_quote` must equal the actual
/// output produced by `swap_b_for_a` on an identical pool state.
#[test]
fn test_quote_matches_live_swap_b_for_a() {
    let ra: i128 = 500_000;
    let rb: i128 = 800_000;
    let amount_in: i128 = 30_000;

    let (_env, client) = setup(ra, rb);
    let fee_bps = client.get_fee_bps();
    let quote = client.get_swap_quote(&amount_in, &fee_bps, &false).unwrap();

    let live_out = client.swap_b_for_a(&amount_in);

    assert_eq!(
        quote.amount_out, live_out,
        "quote amount_out must match live swap output to the unit (B→A)"
    );

    let (live_ra, live_rb) = client.get_reserves();
    assert_eq!(
        quote.reserve_a_after, live_ra,
        "projected reserve_a must match post-swap reserve_a (B→A)"
    );
    assert_eq!(
        quote.reserve_b_after, live_rb,
        "projected reserve_b must match post-swap reserve_b (B→A)"
    );
}

// ---------------------------------------------------------------------------
// Zero reserves → typed EmptyPool error, not a panic
// ---------------------------------------------------------------------------

#[test]
fn test_zero_reserves_returns_empty_pool_error_a_for_b() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    // Intentionally do NOT call init_pool → reserves default to 0.

    let result = client.try_get_swap_quote(&100, &DEFAULT_FEE_BPS, &true);
    assert_eq!(
        result,
        Err(Ok(AmmPoolError::EmptyPool)),
        "zero reserves must return EmptyPool, not panic"
    );
}

#[test]
fn test_zero_reserves_returns_empty_pool_error_b_for_a() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);

    let result = client.try_get_swap_quote(&100, &DEFAULT_FEE_BPS, &false);
    assert_eq!(
        result,
        Err(Ok(AmmPoolError::EmptyPool)),
        "zero reserves must return EmptyPool (B→A), not panic"
    );
}

// ---------------------------------------------------------------------------
// Large amount near pool depletion
// ---------------------------------------------------------------------------

/// An amount just below the full reserve should produce a non-zero, sensible
/// output — the formula never panics and the projected reserve stays >= 1.
#[test]
fn test_large_amount_near_depletion_a_for_b() {
    let ra: i128 = 1_000_000;
    let rb: i128 = 1_000_000;
    // Swap in 999_990 — nearly the entire reserve_a equivalent.
    // The Uniswap-v2 formula limits amount_out < rb, so reserve_b_after >= 1.
    let amount_in: i128 = 999_990;

    let (_env, client) = setup(ra, rb);
    let fee_bps = client.get_fee_bps();
    let quote = client.get_swap_quote(&amount_in, &fee_bps, &true).unwrap();

    assert!(
        quote.amount_out > 0,
        "large-amount quote must produce positive output"
    );
    assert!(
        quote.reserve_b_after >= 0,
        "projected reserve_b must remain non-negative near depletion"
    );
    assert!(
        quote.reserve_b_after < rb,
        "reserve_b must decrease after A→B swap"
    );
    assert_eq!(
        quote.reserve_a_after,
        ra + amount_in,
        "reserve_a must increase by amount_in"
    );
}

#[test]
fn test_large_amount_near_depletion_b_for_a() {
    let ra: i128 = 1_000_000;
    let rb: i128 = 1_000_000;
    let amount_in: i128 = 999_990;

    let (_env, client) = setup(ra, rb);
    let fee_bps = client.get_fee_bps();
    let quote = client.get_swap_quote(&amount_in, &fee_bps, &false).unwrap();

    assert!(
        quote.amount_out > 0,
        "large-amount quote (B→A) must produce positive output"
    );
    assert!(
        quote.reserve_a_after >= 0,
        "projected reserve_a must remain non-negative near depletion"
    );
    assert!(
        quote.reserve_a_after < ra,
        "reserve_a must decrease after B→A swap"
    );
    assert_eq!(
        quote.reserve_b_after,
        rb + amount_in,
        "reserve_b must increase by amount_in"
    );
}

// ---------------------------------------------------------------------------
// Fee in quote matches compute_fee output
// ---------------------------------------------------------------------------

/// The `fee` field of the quote must equal `amount_in * fee_bps / 10_000`,
/// which is the definition of `compute_fee`.
#[test]
fn test_quoted_fee_matches_compute_fee_default_bps() {
    let amount_in: i128 = 10_000;
    let (_env, client) = setup(1_000_000, 1_000_000);
    let fee_bps = DEFAULT_FEE_BPS; // 30

    let quote = client.get_swap_quote(&amount_in, &fee_bps, &true).unwrap();
    let expected_fee = amount_in * fee_bps / 10_000; // floor division

    assert_eq!(
        quote.fee, expected_fee,
        "quoted fee must equal amount_in * fee_bps / 10_000"
    );
}

#[test]
fn test_quoted_fee_matches_compute_fee_custom_bps() {
    let amount_in: i128 = 20_000;
    let fee_bps: i128 = 200; // 2 %
    let (_env, client) = setup(1_000_000, 1_000_000);

    let quote = client.get_swap_quote(&amount_in, &fee_bps, &true).unwrap();
    let expected_fee = amount_in * fee_bps / 10_000;

    assert_eq!(
        quote.fee, expected_fee,
        "quoted fee must match compute_fee with custom bps"
    );
}

/// Zero fee: `fee` field must be 0 and quote still produces positive output.
#[test]
fn test_quoted_fee_zero_bps() {
    let amount_in: i128 = 5_000;
    let (_env, client) = setup(100_000, 100_000);

    let quote = client.get_swap_quote(&amount_in, &0, &true).unwrap();
    assert_eq!(quote.fee, 0, "zero fee_bps must yield fee=0 in quote");
    assert!(quote.amount_out > 0, "zero-fee quote must still produce output");
}

// ---------------------------------------------------------------------------
// Non-positive amount_in → NonPositiveAmount error
// ---------------------------------------------------------------------------

#[test]
fn test_zero_amount_in_returns_error() {
    let (_env, client) = setup(100_000, 100_000);
    let result = client.try_get_swap_quote(&0, &DEFAULT_FEE_BPS, &true);
    assert_eq!(
        result,
        Err(Ok(AmmPoolError::NonPositiveAmount)),
        "zero amount_in must return NonPositiveAmount"
    );
}

#[test]
fn test_negative_amount_in_returns_error() {
    let (_env, client) = setup(100_000, 100_000);
    let result = client.try_get_swap_quote(&-1, &DEFAULT_FEE_BPS, &true);
    assert_eq!(
        result,
        Err(Ok(AmmPoolError::NonPositiveAmount)),
        "negative amount_in must return NonPositiveAmount"
    );
}

// ---------------------------------------------------------------------------
// Quote does not mutate pool state
// ---------------------------------------------------------------------------

/// After calling `get_swap_quote`, the reserves must be unchanged.
#[test]
fn test_quote_does_not_mutate_reserves() {
    let ra: i128 = 1_000_000;
    let rb: i128 = 2_000_000;
    let (_env, client) = setup(ra, rb);
    let fee_bps = client.get_fee_bps();

    let _quote = client.get_swap_quote(&50_000, &fee_bps, &true).unwrap();

    let (ra_after, rb_after) = client.get_reserves();
    assert_eq!(ra_after, ra, "get_swap_quote must not mutate reserve_a");
    assert_eq!(rb_after, rb, "get_swap_quote must not mutate reserve_b");
}
