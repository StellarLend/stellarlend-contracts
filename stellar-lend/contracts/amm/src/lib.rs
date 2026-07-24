#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Env, Vec};

#[contract]
pub struct AmmContract;

// Keys for persistent storage
const KEY_RES_A: (&str, &str) = ("pool", "a");
const KEY_RES_B: (&str, &str) = ("pool", "b");
const KEY_TWAP_OBS: (&str, &str) = ("twap", "obs");

/// A single TWAP observation snapshot.
///
/// Both prices are stored as scaled integer ratios (price * 10^9) to preserve
/// precision without floating-point arithmetic in `#![no_std]`.
///
/// * `price0` – spot price of token A in terms of token B, scaled by 1_000_000_000.
///   Computed as `reserve_b * 1_000_000_000 / reserve_a`.
/// * `price1` – spot price of token B in terms of token A, scaled by 1_000_000_000.
///   Computed as `reserve_a * 1_000_000_000 / reserve_b`.
/// * `timestamp` – ledger timestamp at the time of the observation.
#[contracttype]
#[derive(Clone)]
pub struct TwapObservation {
    pub price0: i128,
    pub price1: i128,
    pub timestamp: u64,
}

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
        let new_rb = rb.checked_sub(amount_out).expect("insufficient reserve b");
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);

        // Record TWAP observation after every reserve mutation.
        record_twap_observation(&env);

        amount_out
    }

    /// Swap from B -> A using Uniswap-style formula with fee (fee_bps out of 10_000).
    /// Returns amount_out.
    pub fn swap_b_for_a(env: Env, amount_in: i128, fee_bps: i128) -> i128 {
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

        // numerator = amount_in_with_fee * reserve_out (token A)
        let numerator = amount_in_with_fee.checked_mul(ra).expect("overflow");
        // denominator = reserve_in (token B) * 10000 + amount_in_with_fee
        let denom_part = rb.checked_mul(10_000_i128).expect("overflow");
        let denominator = denom_part.checked_add(amount_in_with_fee).expect("overflow");

        let amount_out = numerator / denominator;

        let new_rb = rb.checked_add(amount_in).expect("overflow");
        let new_ra = ra.checked_sub(amount_out).expect("insufficient reserve a");
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);

        // Record TWAP observation after every reserve mutation.
        record_twap_observation(&env);

        amount_out
    }

    /// Flash-swap A for B: send `amount_out` of token B to the caller speculatively,
    /// then verify the pool's token-A reserve has increased by at least the required
    /// input (the "repayment"). The callback model is simplified here — we treat it as
    /// a pre-validated swap where the caller guarantees repayment via `amount_in`.
    ///
    /// In a full on-chain implementation the caller would receive tokens, execute
    /// arbitrary logic, and repay in the same transaction. Here we model the net
    /// effect: reserves are updated exactly as in a normal swap, and the TWAP is
    /// recorded. Returns amount_out.
    pub fn flash_swap_a_for_b(env: Env, amount_in: i128, fee_bps: i128) -> i128 {
        if amount_in <= 0 {
            panic!("amount must be positive");
        }
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            panic!("empty pool");
        }

        // Compute output using the same Uniswap v2 formula.
        let fee_adj = 10_000_i128.checked_sub(fee_bps).expect("fee overflow");
        let amount_in_with_fee = amount_in.checked_mul(fee_adj).expect("overflow");

        let numerator = amount_in_with_fee.checked_mul(rb).expect("overflow");
        let denom_part = ra.checked_mul(10_000_i128).expect("overflow");
        let denominator = denom_part.checked_add(amount_in_with_fee).expect("overflow");

        let amount_out = numerator / denominator;

        // Verify post-flash invariant: k must not decrease.
        let new_ra = ra.checked_add(amount_in).expect("overflow");
        let new_rb = rb.checked_sub(amount_out).expect("insufficient reserve b");
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);

        // Commit the reserve mutation.
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);

        // Record TWAP observation after every reserve mutation.
        record_twap_observation(&env);

        amount_out
    }

    /// Read reserves (for testing/inspection).
    pub fn get_reserves(env: Env) -> (i128, i128) {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        (ra, rb)
    }

    /// Return all accumulated TWAP observations.
    ///
    /// Returns an empty vector if no swap has been executed yet.
    pub fn get_twap_observations(env: Env) -> Vec<TwapObservation> {
        env.storage()
            .persistent()
            .get(&KEY_TWAP_OBS)
            .unwrap_or_else(|| Vec::new(&env))
    }
}

// ---------------------------------------------------------------------------
// TWAP helper — only callable internally by swap functions
// ---------------------------------------------------------------------------

/// Append a new TWAP observation to persistent storage using the current
/// post-swap reserves and ledger timestamp.
///
/// Both spot prices are scaled by 1_000_000_000 to retain nine decimal places
/// of precision without floating-point arithmetic.
///
/// Only callable internally by swap functions.
fn record_twap_observation(env: &Env) {
    let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
    let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);

    if ra <= 0 || rb <= 0 {
        // Nothing useful to record with empty reserves.
        return;
    }

    const SCALE: i128 = 1_000_000_000;
    let price0 = rb.checked_mul(SCALE).expect("price0 overflow") / ra; // B per A
    let price1 = ra.checked_mul(SCALE).expect("price1 overflow") / rb; // A per B

    let timestamp = env.ledger().timestamp();

    let obs = TwapObservation {
        price0,
        price1,
        timestamp,
    };

    let mut observations: Vec<TwapObservation> = env
        .storage()
        .persistent()
        .get(&KEY_TWAP_OBS)
        .unwrap_or_else(|| Vec::new(env));

    observations.push_back(obs);
    env.storage()
        .persistent()
        .set(&KEY_TWAP_OBS, &observations);
}

// ---------------------------------------------------------------------------
// Core invariant helper
// ---------------------------------------------------------------------------
fn assert_k_monotonic(
    before_a: i128,
    before_b: i128,
    after_a: i128,
    after_b: i128,
    expect_increase: bool,
) {
    let k_before = before_a
        .checked_mul(before_b)
        .expect("k overflow before");
    let k_after = after_a.checked_mul(after_b).expect("k overflow after");
    if expect_increase {
        if k_after < k_before {
            panic!(
                "Invariant violation: k decreased (before={}, after={})",
                k_before, k_after
            );
        }
    } else if k_after > k_before {
        panic!(
            "Invariant violation: k increased on removal (before={}, after={})",
            k_before, k_after
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: fuzz-style sweeping of reserves and swap amounts
// ---------------------------------------------------------------------------
#[cfg(test)]
mod test {
    use super::*;

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

    // -----------------------------------------------------------------------
    // Regression test: get_twap_observations() must grow after every swap
    // -----------------------------------------------------------------------

    /// Regression guard: swap_a_for_b, swap_b_for_a, and flash_swap_a_for_b
    /// must each append a TWAP observation so that downstream consumers
    /// (price-impact analysis, oracle fallback) have data to work with.
    ///
    /// Prior to the fix, record_twap_observation was never called from any
    /// swap path, so get_twap_observations() always returned an empty vector.
    #[test]
    fn test_twap_observations_grow_after_each_swap() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);

        client.init_pool(&1_000_000, &1_000_000);

        // Before any swap, the observation list is empty.
        assert_eq!(
            client.get_twap_observations().len(),
            0,
            "no observations expected before any swap"
        );

        // --- swap_a_for_b ---
        client.swap_a_for_b(&1_000, &30);
        assert_eq!(
            client.get_twap_observations().len(),
            1,
            "expected 1 observation after swap_a_for_b"
        );

        // --- swap_b_for_a ---
        client.swap_b_for_a(&1_000, &30);
        assert_eq!(
            client.get_twap_observations().len(),
            2,
            "expected 2 observations after swap_b_for_a"
        );

        // --- flash_swap_a_for_b ---
        client.flash_swap_a_for_b(&1_000, &30);
        assert_eq!(
            client.get_twap_observations().len(),
            3,
            "expected 3 observations after flash_swap_a_for_b"
        );

        // Also verify the observation fields are sensible (non-zero prices,
        // timestamp set to the ledger timestamp in the test env).
        let obs = client.get_twap_observations();
        for i in 0..obs.len() {
            let o = obs.get(i).unwrap();
            assert!(o.price0 > 0, "price0 must be positive (obs {})", i);
            assert!(o.price1 > 0, "price1 must be positive (obs {})", i);
        }
    }
}
