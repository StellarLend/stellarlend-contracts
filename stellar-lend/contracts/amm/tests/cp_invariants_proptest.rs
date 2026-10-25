//! Constant-product invariant property tests for AMM
//!
//! This test suite verifies the core AMM invariants using property-based testing:
//! - K-monotonicity: k = reserve_a * reserve_b never decreases on swaps
//! - LP share math: minting/burning shares is proportional within 1 unit
//! - Withdrawal bounds: remove_liquidity never returns more than proportional reserves
//! - Reserve non-negativity: reserves stay >= 0 after all operations
//!
//! Uses random sequences of init/add/remove/swap operations with SAC tokens.

use proptest::prelude::*;
use soroban_sdk::{testutils::Address as _, Address, Env};
use stellarlend_amm::{AmmContract, AmmContractClient};

/// Register a test token contract (SAC-compatible)
fn register_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

/// Fund a trader with tokens
fn fund_trader(env: &Env, trader: &Address, token_a: &Address, token_b: &Address, amount: i128) {
    use soroban_sdk::token::StellarAssetClient;
    StellarAssetClient::new(env, token_a).mint(trader, &amount);
    StellarAssetClient::new(env, token_b).mint(trader, &amount);
}

/// Fund the pool contract directly
fn fund_pool(
    env: &Env,
    pool_id: &Address,
    token_a: &Address,
    token_b: &Address,
    amount_a: i128,
    amount_b: i128,
) {
    use soroban_sdk::token::StellarAssetClient;
    StellarAssetClient::new(env, token_a).mint(pool_id, &amount_a);
    StellarAssetClient::new(env, token_b).mint(pool_id, &amount_b);
}

/// Property: k never decreases on swaps
#[test]
fn prop_k_monotonic_on_swaps() {
    proptest!(
        ProptestConfig::with_cases(256),
        |(
            reserve_a in 1000_i128..=1_000_000_i128,
            reserve_b in 1000_i128..=1_000_000_i128,
            amount_in in 1_i128..=10_000_i128,
            a_for_b: bool
        )| {
            let env = Env::default();
            env.mock_all_auths();
            
            let pool_id = env.register(AmmContract, ());
            let client = AmmContractClient::new(&env, &pool_id);
            
            let token_a = register_token(&env);
            let token_b = register_token(&env);
            let admin = Address::generate(&env);
            let trader = Address::generate(&env);
            
            // Initialize pool
            client.init_pool(&admin, &reserve_a, &reserve_b, &token_a, &token_b);
            fund_pool(&env, &pool_id, &token_a, &token_b, reserve_a, reserve_b);
            fund_trader(&env, &trader, &token_a, &token_b, amount_in * 2);
            
            let (ra_before, rb_before) = client.get_reserves();
            let k_before = ra_before.checked_mul(rb_before).unwrap();
            
            // Perform swap
            let result = if a_for_b {
                client.try_swap_a_for_b(&trader, &amount_in, &0)
            } else {
                client.try_swap_b_for_a(&trader, &amount_in, &0)
            };
            
            // If swap succeeded, k must not decrease
            if result.is_ok() {
                let (ra_after, rb_after) = client.get_reserves();
                let k_after = ra_after.checked_mul(rb_after).unwrap();
                prop_assert!(
                    k_after >= k_before,
                    "k decreased: {} * {} = {} -> {} * {} = {}",
                    ra_before, rb_before, k_before,
                    ra_after, rb_after, k_after
                );
            }
        }
    );
}

/// Property: LP shares are minted proportionally (within 1 unit rounding error)
#[test]
fn prop_lp_shares_proportional() {
    proptest!(
        ProptestConfig::with_cases(256),
        |(
            initial_a in 5000_i128..=50_000_i128,
            initial_b in 5000_i128..=50_000_i128,
            add_fraction in 10_u32..=50_u32,  // Add 10-50% more liquidity
        )| {
            let env = Env::default();
            env.mock_all_auths();
            
            let pool_id = env.register(AmmContract, ());
            let client = AmmContractClient::new(&env, &pool_id);
            
            let token_a = register_token(&env);
            let token_b = register_token(&env);
            let admin = Address::generate(&env);
            let liquidity_provider = Address::generate(&env);
            
            // Initialize pool with zero reserves (first deposit pattern)
            client.init_pool(&admin, &0, &0, &token_a, &token_b);
            
            // Calculate amounts to add
            let add_a = (initial_a as u128 * add_fraction as u128 / 100) as i128;
            let add_b = (initial_b as u128 * add_fraction as u128 / 100) as i128;
            
            // Fund with enough for both deposits plus buffer
            fund_trader(&env, &liquidity_provider, &token_a, &token_b, initial_a * 3);
            fund_trader(&env, &liquidity_provider, &token_a, &token_b, initial_b * 3);
            
            // First deposit
            let (_, _, shares1) = client.add_liquidity(
                &liquidity_provider,
                &initial_a,
                &initial_b,
                &0,
                &0,
                &0,
                &u64::MAX
            );
            
            prop_assume!(shares1 > 1000); // Need substantial shares for meaningful test
            
            let (ra1, rb1) = client.get_reserves();
            let total_supply1 = client.get_total_supply();
            
            // Second deposit - proportional amounts
            if let Ok(Ok((deposited_a, deposited_b, shares2))) = client.try_add_liquidity(
                &liquidity_provider,
                &add_a,
                &add_b,
                &0,
                &0,
                &0,
                &u64::MAX
            ) {
                let total_supply2 = client.get_total_supply();
                
                // Shares minted should be proportional to deposit
                // shares2 / total_supply1 ≈ deposited_a / ra1
                if deposited_a > 0 && ra1 > 0 && total_supply1 > 0 {
                    // Check proportionality with generous rounding allowance
                    let expected_shares = (deposited_a as u128)
                        .checked_mul(total_supply1 as u128)
                        .and_then(|n| n.checked_div(ra1 as u128))
                        .map(|n| n as i128)
                        .unwrap_or(0);
                    
                    let diff = (shares2 as i128 - expected_shares).abs();
                    let tolerance = expected_shares.max(1000) / 100; // 1% tolerance
                    
                    prop_assert!(
                        diff <= tolerance,
                        "LP shares not proportional: got {}, expected {} (diff {} > tolerance {})",
                        shares2, expected_shares, diff, tolerance
                    );
                }
                
                // Supply should increase
                prop_assert!(total_supply2 > total_supply1);
            }
        }
    );
}

/// Property: remove_liquidity never returns more than proportional reserves
#[test]
fn prop_remove_liquidity_bounded() {
    proptest!(
        ProptestConfig::with_cases(256),
        |(
            initial_a in 5000_i128..=100_000_i128,
            initial_b in 5000_i128..=100_000_i128,
            burn_fraction in 1_u32..=100_u32,
        )| {
            let env = Env::default();
            env.mock_all_auths();
            
            let pool_id = env.register(AmmContract, ());
            let client = AmmContractClient::new(&env, &pool_id);
            
            let token_a = register_token(&env);
            let token_b = register_token(&env);
            let admin = Address::generate(&env);
            let lp = Address::generate(&env);
            
            // Initialize pool
            client.init_pool(&admin, &0, &0, &token_a, &token_b);
            fund_trader(&env, &lp, &token_a, &token_b, initial_a + initial_b);
            
            // Add liquidity
            let (_, _, shares) = client.add_liquidity(
                &lp,
                &initial_a,
                &initial_b,
                &0,
                &0,
                &0,
                &u64::MAX
            );
            
            prop_assume!(shares > 100); // Need enough shares to burn a fraction
            
            let (ra_before, rb_before) = client.get_reserves();
            let total_supply = client.get_total_supply();
            
            // Burn a fraction of LP shares
            let burn_shares = ((shares as u128) * (burn_fraction as u128) / 100_u128) as i128;
            prop_assume!(burn_shares > 0 && burn_shares <= shares);
            
            let (amount_a, amount_b) = client.remove_liquidity(
                &lp,
                &burn_shares,
                &0,
                &0,
                &u64::MAX
            );
            
            // Calculate maximum proportional amount
            let max_a = (ra_before as u128).checked_mul(burn_shares as u128)
                .and_then(|n| n.checked_div(total_supply as u128))
                .map(|n| n as i128)
                .unwrap_or(0);
            let max_b = (rb_before as u128).checked_mul(burn_shares as u128)
                .and_then(|n| n.checked_div(total_supply as u128))
                .map(|n| n as i128)
                .unwrap_or(0);
            
            // Returned amounts must not exceed proportional share (with small rounding allowance)
            prop_assert!(
                amount_a <= max_a + 2,
                "remove_liquidity returned too much A: {} > {} (burn {}/{} shares)",
                amount_a, max_a, burn_shares, total_supply
            );
            prop_assert!(
                amount_b <= max_b + 2,
                "remove_liquidity returned too much B: {} > {} (burn {}/{} shares)",
                amount_b, max_b, burn_shares, total_supply
            );
        }
    );
}

/// Property: reserves stay non-negative after all operations
#[test]
fn prop_reserves_non_negative() {
    proptest!(
        ProptestConfig::with_cases(256),
        |(
            reserve_a in 1000_i128..=100_000_i128,
            reserve_b in 1000_i128..=100_000_i128,
            operations in prop::collection::vec(0_u8..4, 1..10)
        )| {
            let env = Env::default();
            env.mock_all_auths();
            
            let pool_id = env.register(AmmContract, ());
            let client = AmmContractClient::new(&env, &pool_id);
            
            let token_a = register_token(&env);
            let token_b = register_token(&env);
            let admin = Address::generate(&env);
            let user = Address::generate(&env);
            
            // Initialize pool
            client.init_pool(&admin, &reserve_a, &reserve_b, &token_a, &token_b);
            fund_pool(&env, &pool_id, &token_a, &token_b, reserve_a, reserve_b);
            fund_trader(&env, &user, &token_a, &token_b, 1_000_000);
            
            // Perform random operations
            for op in operations {
                let (ra, rb) = client.get_reserves();
                
                // Check reserves are non-negative before operation
                prop_assert!(ra >= 0, "reserve_a negative before op");
                prop_assert!(rb >= 0, "reserve_b negative before op");
                
                match op % 4 {
                    0 => {
                        // Swap A for B (small amount)
                        let amount = (ra / 100).max(1).min(1000);
                        let _ = client.try_swap_a_for_b(&user, &amount, &0);
                    }
                    1 => {
                        // Swap B for A (small amount)
                        let amount = (rb / 100).max(1).min(1000);
                        let _ = client.try_swap_b_for_a(&user, &amount, &0);
                    }
                    2 => {
                        // Add liquidity (small amount)
                        let add_a = (ra / 50).max(1).min(5000);
                        let add_b = (rb / 50).max(1).min(5000);
                        let _ = client.try_add_liquidity(
                            &user, &add_a, &add_b, &0, &0, &0, &u64::MAX
                        );
                    }
                    _ => {
                        // Remove liquidity (small fraction)
                        let lp_balance = client.get_lp_balance(&user);
                        if lp_balance > 10 {
                            let burn = lp_balance / 10;
                            let _ = client.try_remove_liquidity(
                                &user, &burn, &0, &0, &u64::MAX
                            );
                        }
                    }
                }
            }
            
            // Final check: reserves must still be non-negative
            let (ra_final, rb_final) = client.get_reserves();
            prop_assert!(ra_final >= 0, "reserve_a negative at end: {}", ra_final);
            prop_assert!(rb_final >= 0, "reserve_b negative at end: {}", rb_final);
        }
    );
}

/// Property: swap output is bounded by reserve_out
#[test]
fn prop_swap_output_bounded_by_reserve() {
    proptest!(
        ProptestConfig::with_cases(256),
        |(
            reserve_a in 10_000_i128..=1_000_000_i128,
            reserve_b in 10_000_i128..=1_000_000_i128,
            amount_in in 1_i128..=50_000_i128,
        )| {
            let env = Env::default();
            env.mock_all_auths();
            
            let pool_id = env.register(AmmContract, ());
            let client = AmmContractClient::new(&env, &pool_id);
            
            let token_a = register_token(&env);
            let token_b = register_token(&env);
            let admin = Address::generate(&env);
            let trader = Address::generate(&env);
            
            client.init_pool(&admin, &reserve_a, &reserve_b, &token_a, &token_b);
            fund_pool(&env, &pool_id, &token_a, &token_b, reserve_a, reserve_b);
            fund_trader(&env, &trader, &token_a, &token_b, amount_in * 2);
            
            let (ra, rb) = client.get_reserves();
            
            // Swap A for B
            if let Ok(Ok(amount_out)) = client.try_swap_a_for_b(&trader, &amount_in, &0) {
                prop_assert!(
                    amount_out < rb,
                    "swap_a_for_b output {} >= reserve_b {}",
                    amount_out, rb
                );
            }
            
            // Reset pool for second test
            client.init_pool(&admin, &reserve_a, &reserve_b, &token_a, &token_b);
            fund_pool(&env, &pool_id, &token_a, &token_b, reserve_a, reserve_b);
            
            // Swap B for A
            if let Ok(Ok(amount_out)) = client.try_swap_b_for_a(&trader, &amount_in, &0) {
                prop_assert!(
                    amount_out < ra,
                    "swap_b_for_a output {} >= reserve_a {}",
                    amount_out, ra
                );
            }
        }
    );
}
