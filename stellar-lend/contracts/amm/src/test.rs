//! Tests for the AMM minimum-liquidity floor.
//!
//! Coverage targets:
//! - Floor 0 no-op (backward compatible)
//! - Remove exactly to floor (allowed)
//! - Remove below floor (rejected)
//! - Swap leaving reserve below floor (rejected)
//! - K-invariant preserved for permitted ops
//! - Admin-gated setter
//! - Initialize, views, and edge cases

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

/// Default initial reserves for tests.
const INIT_A: i128 = 1_000_000;
const INIT_B: i128 = 2_000_000;

/// Set up a fresh AMM contract with 2 tokens and initial liquidity.
fn setup() -> (Env, AmmContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    client.initialize(&admin, &token_a, &token_b);
    (env, client, admin, token_a, token_b)
}

/// Set up with initial liquidity already added.
fn setup_with_liquidity(
) -> (Env, AmmContractClient<'static>, Address, Address, Address) {
    let (env, client, admin, token_a, token_b) = setup();
    let lp = Address::generate(&env);
    // First deposit into the empty pool uses token B = amount_b_min
    client.add_liquidity(&lp, &INIT_A, &INIT_B);
    (env, client, admin, token_a, token_b)
}

// -----------------------------------------------------------------------
// Initialization
// -----------------------------------------------------------------------

#[test]
fn test_initialize_sets_admin_and_tokens() {
    let (env, client, admin, token_a, token_b) = setup();
    assert_eq!(client.get_admin(), admin);
    let (a, b) = client.get_tokens();
    assert_eq!(a, token_a);
    assert_eq!(b, token_b);
}

#[test]
fn test_initialize_reserves_start_at_zero() {
    let (env, client, _admin, _token_a, _token_b) = setup();
    let reserves = client.get_reserves();
    assert_eq!(reserves.reserve_a, 0);
    assert_eq!(reserves.reserve_b, 0);
}

#[test]
fn test_initialize_min_liquidity_defaults_to_zero() {
    let (env, client, _admin, _token_a, _token_b) = setup();
    assert_eq!(client.get_min_liquidity(), 0);
}

#[test]
#[should_panic(expected = "2")] // AlreadyInitialized
fn test_initialize_cannot_be_called_twice() {
    let (env, client, admin, token_a, token_b) = setup();
    client.initialize(&admin, &token_a, &token_b);
}

// -----------------------------------------------------------------------
// Not-initialized guard
// -----------------------------------------------------------------------

#[test]
#[should_panic(expected = "1")] // NotInitialized
fn test_operations_reject_uninitialized_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    // Do NOT initialize; should panic on any view that requires init.
    client.get_admin();
}

#[test]
#[should_panic(expected = "1")] // NotInitialized
fn test_get_reserves_rejects_uninitialized() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    client.get_reserves();
}

// -----------------------------------------------------------------------
// Admin setter (set_min_liquidity)
// -----------------------------------------------------------------------

#[test]
fn test_set_min_liquidity_admin_only_succeeds() {
    let (_env, client, _admin, _token_a, _token_b) = setup();
    assert_eq!(client.get_min_liquidity(), 0);
    client.set_min_liquidity(&1_000).unwrap();
    assert_eq!(client.get_min_liquidity(), 1_000);
}

#[test]
fn test_set_min_liquidity_zero_is_accepted() {
    let (_env, client, _admin, _token_a, _token_b) = setup();
    client.set_min_liquidity(&0).unwrap();
    assert_eq!(client.get_min_liquidity(), 0);
}

#[test]
fn test_set_min_liquidity_rejects_negative() {
    let (_env, client, _admin, _token_a, _token_b) = setup();
    let res = client.try_set_min_liquidity(&(-100));
    assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));
}

#[test]
fn test_set_min_liquidity_requires_admin() {
    let env = Env::default();
    let id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Initialize with explicit admin auth only
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &id,
            fn_name: "initialize",
            args: (
                admin.clone(),
                token_a.clone(),
                token_b.clone(),
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin, &token_a, &token_b).unwrap();

    // Non-admin tries to set min liquidity (no auth mock) → should panic
    let attacker = Address::generate(&env);
    let res = client.try_set_min_liquidity(&500);
    // With no auth for the attacker, the env will fail require_auth
    assert!(res.is_err());
}

// -----------------------------------------------------------------------
// Floor 0 no-op (backward compatible)
// -----------------------------------------------------------------------

#[test]
fn test_floor_zero_remove_liquidity_full_withdrawal() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // Floor is 0 by default; removing all liquidity should succeed.
    let (a, b) = client.remove_liquidity(&Address::generate(&_env), &INIT_A, &INIT_B).unwrap();
    assert_eq!(a, INIT_A);
    assert_eq!(b, INIT_B);
    let reserves = client.get_reserves();
    assert_eq!(reserves.reserve_a, 0);
    assert_eq!(reserves.reserve_b, 0);
}

#[test]
fn test_floor_zero_swap_allowed_regardless_of_outgoing_reserve() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // With floor=0, a large swap that significantly reduces reserve B is still permitted.
    // amount_b_out = (2_000_000 * 10_000_000) / (1_000_000 + 10_000_000) ≈ 1_818,181
    // new_reserve_b = 2_000_000 - 1_818,181 = 181,819
    // With floor=0 this should succeed since 181,819 >= 0.
    let result = client.try_swap_exact_a_for_b(&Address::generate(&env), &(INIT_A * 10), &0);
    assert!(result.is_ok());
}

// -----------------------------------------------------------------------
// remove_liquidity floor enforcement
// -----------------------------------------------------------------------

#[test]
fn test_remove_liquidity_exactly_to_floor_allowed() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // Set floor to 100_000
    client.set_min_liquidity(&100_000).unwrap();
    // Remove INIT_A - 100_000 of A and INIT_B - 100_000 of B
    let remove_a = INIT_A - 100_000;
    let remove_b = INIT_B - 100_000;
    let (a, b) = client.remove_liquidity(&Address::generate(&env), &remove_a, &remove_b).unwrap();
    assert_eq!(a, remove_a);
    assert_eq!(b, remove_b);
    let reserves = client.get_reserves();
    assert_eq!(reserves.reserve_a, 100_000);
    assert_eq!(reserves.reserve_b, 100_000);
}

#[test]
fn test_remove_liquidity_below_floor_rejected() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    client.set_min_liquidity(&100_000).unwrap();
    // Try to remove more than reserve - floor
    let remove_a = INIT_A - 50_000; // would leave 50_000 < 100_000 floor
    let remove_b = INIT_B - 100_000; // would leave 100_000 >= 100_000 floor
    let res = client.try_remove_liquidity(&Address::generate(&_env), &remove_a, &remove_b);
    assert!(matches!(res, Err(Ok(AmmError::BelowMinLiquidity))));
}

#[test]
fn test_remove_liquidity_below_floor_b_only() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    client.set_min_liquidity(&100_000).unwrap();
    // Remove all of B (would leave 0 B, below floor)
    let res = client.try_remove_liquidity(&Address::generate(&_env), &1, &INIT_B);
    assert!(matches!(res, Err(Ok(AmmError::BelowMinLiquidity))));
}

#[test]
fn test_remove_liquidity_below_floor_a_only() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    client.set_min_liquidity(&100_000).unwrap();
    // Remove all of A (would leave 0 A, below floor)
    let res = client.try_remove_liquidity(&Address::generate(&_env), &INIT_A, &1);
    assert!(matches!(res, Err(Ok(AmmError::BelowMinLiquidity))));
}

// -----------------------------------------------------------------------
// Swap floor enforcement
// -----------------------------------------------------------------------

#[test]
fn test_swap_a_for_b_below_floor_rejected() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // Set floor high enough that draining B below floor will be blocked
    // INIT_B = 2_000_000. Set floor to 1_500_000.
    client.set_min_liquidity(&1_500_000).unwrap();
    // Swap in enough A to reduce B below 1_500_000
    // amount_b_out = (reserve_b * amount_a_in) / (reserve_a + amount_a_in)
    // We want amount_b_out > 500_000 (to bring reserve_b below 1_500_000)
    // amount_b_out = (2_000_000 * X) / (1_000_000 + X) > 500_000
    // 2_000_000 * X > 500_000 * (1_000_000 + X)
    // 2_000_000 * X > 500_000_000_000 + 500_000 * X
    // 1_500_000 * X > 500_000_000_000
    // X > 333_333.33...
    // So 500_000 should be enough
    let res = client.try_swap_exact_a_for_b(&Address::generate(&env), &500_000, &0);
    assert!(matches!(res, Err(Ok(AmmError::BelowMinLiquidity))));
}

#[test]
fn test_swap_b_for_a_below_floor_rejected() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // Set floor high enough that draining A below floor will be blocked
    // INIT_A = 1_000_000. Set floor to 800_000.
    client.set_min_liquidity(&800_000).unwrap();
    // Swap in enough B to reduce A below 800_000
    // amount_a_out = (reserve_a * amount_b_in) / (reserve_b + amount_b_in)
    // amount_a_out = (1_000_000 * X) / (2_000_000 + X) > 200_000
    // 1_000_000 * X > 200_000 * (2_000_000 + X)
    // 1_000_000 * X > 400_000_000_000 + 200_000 * X
    // 800_000 * X > 400_000_000_000
    // X > 500_000
    let res = client.try_swap_exact_b_for_a(&Address::generate(&env), &600_000, &0);
    assert!(matches!(res, Err(Ok(AmmError::BelowMinLiquidity))));
}

#[test]
fn test_swap_a_for_b_at_floor_allowed() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // Set floor that allows a partial drain.
    client.set_min_liquidity(&1_800_000).unwrap();
    // amount_b_out = (2_000_000 * X) / (1_000_000 + X)
    // We want amount_b_out <= 200_000 so new_reserve_b >= 1_800_000
    // (2_000_000 * X) / (1_000_000 + X) <= 200_000
    // 2_000_000 * X <= 200_000 * (1_000_000 + X)
    // 2_000_000 * X <= 200_000_000_000 + 200_000 * X
    // 1_800_000 * X <= 200_000_000_000
    // X <= 111_111.11...
    // Use 100_000 for safety: amount_b_out = (2_000_000 * 100_000) / 1_100_000 ≈ 181,818
    // new_reserve_b = 2_000_000 - 181,818 = 1,818,182 >= 1,800_000 ✓
    let result = client.swap_exact_a_for_b(&Address::generate(&env), &100_000, &0).unwrap();
    assert!(result > 0);
    let reserves = client.get_reserves();
    assert!(reserves.reserve_b >= 1_800_000);
}

// -----------------------------------------------------------------------
// K-invariant preservation
// -----------------------------------------------------------------------

#[test]
fn test_k_invariant_increases_on_swap() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    let reserves_before = client.get_reserves();
    let k_before = reserves_before.reserve_a * reserves_before.reserve_b;

    // Do a swap
    let out = client.swap_exact_a_for_b(&Address::generate(&env), &50_000, &0).unwrap();
    assert!(out > 0);

    let reserves_after = client.get_reserves();
    let k_after = reserves_after.reserve_a * reserves_after.reserve_b;

    // In real arithmetic the constant-product formula preserves k exactly:
    //   k' = (x+Δx)*(y - y·Δx/(x+Δx)) = x·y = k
    // With integer division, `amount_b_out = y·Δx/(x+Δx)` is truncated,
    // so the pool retains slightly more of the output token than the
    // ideal formula gives.  This means k always stays the same or grows.
    assert!(k_after >= k_before, "K-invariant must not decrease");
}

#[test]
fn test_k_invariant_increases_on_add_liquidity() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    let reserves_before = client.get_reserves();
    let k_before = reserves_before.reserve_a * reserves_before.reserve_b;

    // Add more liquidity
    let amount_b = client.add_liquidity(&Address::generate(&env), &500_000, &0).unwrap();
    assert!(amount_b > 0);

    let reserves_after = client.get_reserves();
    let k_after = reserves_after.reserve_a * reserves_after.reserve_b;

    // K should increase with added liquidity
    assert!(k_after > k_before, "K must increase on add_liquidity");
}

// -----------------------------------------------------------------------
// Amount validation
// -----------------------------------------------------------------------

#[test]
fn test_remove_liquidity_rejects_zero_amount() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    let res = client.try_remove_liquidity(&Address::generate(&_env), &0, &100);
    assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));

    let res = client.try_remove_liquidity(&Address::generate(&_env), &100, &0);
    assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));
}

#[test]
fn test_add_liquidity_rejects_zero_amount() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    let res = client.try_add_liquidity(&Address::generate(&_env), &0, &0);
    assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));
}

#[test]
fn test_swap_rejects_zero_input() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    let res = client.try_swap_exact_a_for_b(&Address::generate(&env), &0, &0);
    assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));

    let res = client.try_swap_exact_b_for_a(&Address::generate(&env), &0, &0);
    assert!(matches!(res, Err(Ok(AmmError::InvalidAmount))));
}

// -----------------------------------------------------------------------
// Insufficient liquidity
// -----------------------------------------------------------------------

#[test]
fn test_remove_liquidity_rejects_exceeding_reserves() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    let res = client.try_remove_liquidity(
        &Address::generate(&_env),
        &(INIT_A + 1),
        &INIT_B,
    );
    assert!(matches!(res, Err(Ok(AmmError::InsufficientLiquidity))));
}

#[test]
fn test_swap_rejects_empty_pool() {
    let (env, client, _admin, _token_a, _token_b) = setup();
    // Pool is empty after init
    let res = client.try_swap_exact_a_for_b(&Address::generate(&env), &100, &0);
    assert!(matches!(res, Err(Ok(AmmError::InsufficientLiquidity))));
}

// -----------------------------------------------------------------------
// Slippage protection
// -----------------------------------------------------------------------

#[test]
fn test_add_liquidity_slippage_protection() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // When pool has (1M, 2M), adding 100k A requires 200k B
    // If we set amount_b_min too high, it should pass if exactly right
    let result = client.add_liquidity(&Address::generate(&_env), &100_000, &200_000).unwrap();
    assert_eq!(result, 200_000);
}

#[test]
fn test_add_liquidity_slippage_exceeded() {
    let (_env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // amount_b_min is too high
    let res = client.try_add_liquidity(&Address::generate(&_env), &100_000, &300_000);
    assert!(matches!(res, Err(Ok(AmmError::SlippageExceeded))));
}

#[test]
fn test_swap_slippage_exceeded() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // amount_b_out_min is higher than the actual output
    // swap_exact_a_for_b: amount_b_out = (2_000_000 * 100_000) / 1_100_000 ≈ 181,818
    let res = client.try_swap_exact_a_for_b(&Address::generate(&env), &100_000, &200_000);
    assert!(matches!(res, Err(Ok(AmmError::SlippageExceeded))));
}

// -----------------------------------------------------------------------
// First deposit (empty pool) edge cases
// -----------------------------------------------------------------------

#[test]
fn test_first_deposit_accepts_any_ratio() {
    let (env, client, _admin, _token_a, _token_b) = setup();
    // First deposit: amount_b is set to amount_b_min max 0
    let out_b = client.add_liquidity(&Address::generate(&env), &500, &300).unwrap();
    assert_eq!(out_b, 300);
    let reserves = client.get_reserves();
    assert_eq!(reserves.reserve_a, 500);
    assert_eq!(reserves.reserve_b, 300);
}

#[test]
fn test_first_deposit_with_zero_b_min() {
    let (env, client, _admin, _token_a, _token_b) = setup();
    let out_b = client.add_liquidity(&Address::generate(&env), &1_000, &0).unwrap();
    assert_eq!(out_b, 0);
    let reserves = client.get_reserves();
    assert_eq!(reserves.reserve_a, 1_000);
    assert_eq!(reserves.reserve_b, 0);
}

// -----------------------------------------------------------------------
// Swap direction consistency
// -----------------------------------------------------------------------

#[test]
fn test_swap_both_directions() {
    let (env, client, _admin, _token_a, _token_b) = setup_with_liquidity();
    // Swap A → B
    let out = client.swap_exact_a_for_b(&Address::generate(&env), &100_000, &0).unwrap();
    assert!(out > 0);
    let reserves = client.get_reserves();
    // K must not decrease with integer-division rounding
    assert!(
        reserves.reserve_a * reserves.reserve_b >= INIT_A * INIT_B,
        "K invariant must not decrease after A→B swap"
    );

    // Swap B → A back
    let out_back = client.swap_exact_b_for_a(&Address::generate(&env), &out, &0).unwrap();
    // Should get approximately the original amount back
    assert!(out_back > 0);
    let reserves_after = client.get_reserves();
    assert!(
        reserves_after.reserve_a * reserves_after.reserve_b >= INIT_A * INIT_B,
        "K invariant must not decrease after round-trip swap"
    );
}
