#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Env};

#[contract]
pub struct AmmContract;

// Keys for persistent storage
const KEY_RES_A: (&str, &str) = ("pool", "a");
const KEY_RES_B: (&str, &str) = ("pool", "b");

#[contractimpl]
impl AmmContract {
    /// Initialize pool reserves (admin only in real code).
    pub fn init_pool(env: Env, a: i128, b: i128) {
        env.storage().persistent().set(&KEY_RES_A, &a);
        env.storage().persistent().set(&KEY_RES_B, &b);
    }

    /// Simple add liquidity: increase reserves and assert k monotonicity (k must not decrease).
    pub fn add_liquidity(env: Env, add_a: i128, add_b: i128) {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        let new_ra = ra.checked_add(add_a).expect("overflow");
        let new_rb = rb.checked_add(add_b).expect("overflow");
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
    }

    /// Simple remove liquidity: decrease reserves and assert k monotonicity (k must not increase).
    pub fn remove_liquidity(env: Env, rem_a: i128, rem_b: i128) {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if rem_a > ra || rem_b > rb {
            panic!("Insufficient reserves");
        }
        let new_ra = ra - rem_a;
        let new_rb = rb - rem_b;
        assert_k_monotonic(ra, rb, new_ra, new_rb, false);
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
    }

    /// Swap from A -> B using Uniswap-style formula with fee (fee_bps out of 10_000).
    /// Returns amount_out.
    pub fn swap_a_for_b(env: Env, amount_in: i128, fee_bps: i128) -> i128 {
        if amount_in <= 0 {
            panic!("amount must be positive");
        }
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            panic!("empty pool");
        }

        // Uniswap v2 style: amount_in_with_fee = amount_in * (10000 - fee_bps)
        let fee_adj = 10_000_i128.checked_sub(fee_bps).expect("fee overflow");
        let amount_in_with_fee = amount_in.checked_mul(fee_adj).expect("overflow");

        // numerator = amount_in_with_fee * reserve_out
        let numerator = amount_in_with_fee.checked_mul(rb).expect("overflow");
        // denominator = reserve_in * 10000 + amount_in_with_fee
        let denom_part = ra.checked_mul(10_000_i128).expect("overflow");
        let denominator = denom_part.checked_add(amount_in_with_fee).expect("overflow");

        let amount_out = numerator / denominator;

        let new_ra = ra.checked_add(amount_in).expect("overflow");
        let new_rb = rb.checked_sub(amount_out);
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        amount_out
    }

    /// Read reserves (for testing/inspection)
    pub fn get_reserves(env: Env) -> (i128, i128) {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        (ra, rb)
    }
}

// ---------------------------------------------------------------------------
// Core invariant helper
// ---------------------------------------------------------------------------
fn assert_k_monotonic(before_a: i128, before_b: i128, after_a: i128, after_b: i128, expect_increase: bool) {
    let k_before = before_a.checked_mul(before_b).expect("k overflow before");
    let k_after = after_a.checked_mul(after_b).expect("k overflow after");
    if expect_increase {
        if k_after < k_before {
            panic!("Invariant violation: k decreased (before={}, after={})", k_before, k_after);
        }
    } else {
        if k_after > k_before {
            panic!("Invariant violation: k increased on removal (before={}, after={})", k_before, k_after);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: fuzz-style sweeping of reserves and swap amounts
// ---------------------------------------------------------------------------
#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn fuzz_swap_k_monotonic() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);

        let reserve_sizes = [1_000_i128, 10_000, 100_000, 1_000_000];
        let amounts = [1_i128, 10, 100, 1_000, 10_000];

        for &ra in reserve_sizes.iter() {
            for &rb in reserve_sizes.iter() {
                for &amt in amounts.iter() {
                    client.init_pool(&ra, &rb);
                    // swap with 30 bps fee
                    let _out = client.swap_a_for_b(&amt, &30);
                    let (new_ra, new_rb) = client.get_reserves();
                    let k_before = ra.checked_mul(rb).unwrap();
                    let k_after = new_ra.checked_mul(new_rb).unwrap();
                    assert!(k_after >= k_before, "k decreased: ra={}, rb={}, amt={}, k_before={}, k_after={}", ra, rb, amt, k_before, k_after);
                }
            }
        }
    }

    #[test]
    fn test_add_and_remove_liquidity_monotonicity() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);

        client.init_pool(&1000, &2000);
        client.add_liquidity(&100, &200);
        let (ra1, rb1) = client.get_reserves();
        let k1 = ra1.checked_mul(rb1).unwrap();

        client.remove_liquidity(&50, &100);
        let (ra2, rb2) = client.get_reserves();
        let k2 = ra2.checked_mul(rb2).unwrap();

        assert!(k2 <= k1, "k should not increase on removal");
    }
}
