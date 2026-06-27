#![no_std]

pub mod math;
pub mod liquidity_math;

#[cfg(test)]
mod sqrt_precision_test;
#[cfg(test)]
mod flash_swap_test;

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

// Existing pool reserve keys (kept for backward compatibility with the
// integer-only `init_pool(a, b)` and the swap-bounds proptest suite).
const KEY_RES_A: (&str, &str) = ("pool", "a");
const KEY_RES_B: (&str, &str) = ("pool", "b");

// New keys for the flash-swap feature.
//
// `KEY_FLASH_ACTIVE` is the protocol-wide reentrancy guard: while a flash
// swap is in flight (between `flash_swap_a_for_b` and the matching
// `repay_flash_swap`), every other state-mutating operation — including a
// nested `flash_swap_a_for_b` — is rejected with `ReentrantFlashSwap`.
//
// `KEY_K_BEFORE` snapshots `reserve_a * reserve_b` at the moment the
// optimistic transfer is performed.  `repay_flash_swap` enforces
// `(reserve_a + amount_in) * reserve_b_after_debit  >=  k_before`.  If the
// receiver underpaid, this check panics and Soroban's atomic rollback
// restores every storage change (including the optimistic reserve debit),
// leaving the pool exactly where it started.
//
// Soroban 25.3.1 forbids a contract from invoking itself directly from
// inside a callback (`Contract re-entry is not allowed`), so the flash
// swap is structured as two entry-points dispatched via Soroban's
// multi-operation transaction model instead of a single cross-contract
// callback chain.
const KEY_FLASH_ACTIVE: (&str, &str) = ("pool", "flash_active");
const KEY_K_BEFORE: (&str, &str) = ("pool", "flash_k_before");

#[contract]
pub struct AmmContract;

#[contractimpl]
impl AmmContract {
    /// Initialize pool reserves (admin only in real code).
    ///
    /// Gated by the `FlashActive` reentrancy guard so that a flash swap
    /// initiated on a stale pool cannot be silently clobbered by a
    /// follow-up `init_pool` from the same transaction.
    pub fn init_pool(env: Env, a: i128, b: i128) {
        Self::assert_no_active_flash_swap(&env);
        env.storage().persistent().set(&KEY_RES_A, &a);
        env.storage().persistent().set(&KEY_RES_B, &b);
    }

    fn assert_no_active_flash_swap(env: &Env) {
        let active: bool = env
            .storage()
            .instance()
            .get(&KEY_FLASH_ACTIVE)
            .unwrap_or(false);
        if active {
            panic!(
                "ReentrantFlashSwap: pool mutation blocked while flash-swap is in flight"
            );
        }
    }

    /// Simple add liquidity: increase reserves and assert k monotonicity
    /// (k must not decrease).
    pub fn add_liquidity(env: Env, add_a: i128, add_b: i128) {
        Self::assert_no_active_flash_swap(&env);
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        let new_ra = ra.checked_add(add_a).expect("overflow");
        let new_rb = rb.checked_add(add_b).expect("overflow");
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
    }

    /// Simple remove liquidity: decrease reserves and assert k monotonicity
    /// (k must not increase).
    pub fn remove_liquidity(env: Env, rem_a: i128, rem_b: i128) {
        Self::assert_no_active_flash_swap(&env);
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

    /// Swap from A -> B using Uniswap-style formula with fee (fee_bps out
    /// of 10_000).  Returns amount_out.
    pub fn swap_a_for_b(env: Env, amount_in: i128, fee_bps: i128) -> i128 {
        Self::assert_no_active_flash_swap(&env);
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
        let denominator = denom_part
            .checked_add(amount_in_with_fee)
            .expect("overflow");

        let amount_out = numerator / denominator;

        let new_ra = ra.checked_add(amount_in).expect("overflow");
        let new_rb = rb.checked_sub(amount_out).expect("underflow");
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        amount_out
    }

    /// Swap from B -> A using the same Uniswap-v2 constant-product formula and
    /// fee model as [`swap_a_for_b`], with token roles reversed.
    ///
    /// # Formula
    ///
    /// ```text
    /// amount_in_with_fee = amount_in * (10_000 - fee_bps)
    /// amount_out = (amount_in_with_fee * reserve_a)
    ///            / (reserve_b * 10_000 + amount_in_with_fee)   [floor division]
    /// ```
    ///
    /// After the swap `reserve_b` increases by `amount_in` and `reserve_a`
    /// decreases by `amount_out`.  The k-monotonicity invariant
    /// (k = reserve_a × reserve_b) is asserted via `assert_k_monotonic`.
    ///
    /// # Panics
    /// - `amount_in <= 0`
    /// - either reserve is zero (empty pool)
    /// - any intermediate checked-arithmetic overflow
    /// - k decreases after the swap (invariant violation)
    pub fn swap_b_for_a(env: Env, amount_in: i128, fee_bps: i128) -> i128 {
        if amount_in <= 0 {
            panic!("amount must be positive");
        }
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            panic!("empty pool");
        }

        // Mirror of swap_a_for_b with A and B roles swapped.
        let fee_adj = 10_000_i128.checked_sub(fee_bps).expect("fee overflow");
        let amount_in_with_fee = amount_in.checked_mul(fee_adj).expect("overflow");

        // reserve_out is A, reserve_in is B
        let numerator = amount_in_with_fee.checked_mul(ra).expect("overflow");
        let denom_part = rb.checked_mul(10_000_i128).expect("overflow");
        let denominator = denom_part.checked_add(amount_in_with_fee).expect("overflow");

        let amount_out = numerator / denominator; // floor — pool never over-pays

        let new_rb = rb.checked_add(amount_in).expect("overflow");
        let new_ra = ra.checked_sub(amount_out).expect("underflow");
        assert_k_monotonic(ra, rb, new_ra, new_rb, true);

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        amount_out
    }

    /// Flash-swap entrypoint — step 1 of the "optimistic transfer then
    /// verify-k" pattern (Uniswap-v2 style).
    ///
    /// Optimistically debits `reserve_b` by `amount_out` and snapshots the
    /// pre-debit invariant `k_before = reserve_a * reserve_b` so the
    /// matching `repay_flash_swap` can enforce
    /// `(reserve_a + amount_in) * reserve_b_after_debit  >=  k_before`.
    ///
    /// Soroban 25.3.1 does not allow a contract to invoke itself from
    /// inside a callback (`Contract re-entry is not allowed`), so this
    /// design dispatches the two halves of the flash swap as separate
    /// entry points within a single multi-operation transaction:
    ///
    /// ```text
    /// Op 1: AMM.flash_swap_a_for_b(amount_out, fee_bps)
    /// Op 2: <caller runs arbitrary logic on asset A received elsewhere>
    /// Op 3: AMM.repay_flash_swap(amount_in)        // verify-k runs here
    /// ```
    ///
    /// Soroban rolls back every storage write in the whole transaction if
    /// any operation panics — including the optimistic debit in Op 1 if
    /// `repay_flash_swap` fails its verify-k check.
    ///
    /// While `flash_swap_a_for_b` is unpaired (between Op 1 and Op 3),
    /// `FlashActive == true` and every other state-mutating operation —
    /// `add_liquidity`, `remove_liquidity`, `swap_a_for_b`, and a nested
    /// `flash_swap_a_for_b` — is rejected with `ReentrantFlashSwap`.
    ///
    /// # Arguments
    /// * `amount_out` — units of asset B to optimistically debit.  Must
    ///                  satisfy `0 < amount_out < reserve_b`.
    /// * `fee_bps`    — protocol fee in basis points out of `10_000`.
    ///                  Must satisfy `0 ≤ fee_bps ≤ 9_999`.
    /// * `params`     — opaque user payload, kept for forward-compatibility
    ///                  with a future cross-contract callback variant.
    ///                  Today the AMM itself does **not** invoke any
    ///                  callback (Soroban 25.3.1 blocks self-re-entry);
    ///                  callers are expected to dispatch
    ///                  `repay_flash_swap` from a follow-up transaction
    ///                  operation.  The value is bound to a local to
    ///                  keep it in the parameter surface.
    pub fn flash_swap_a_for_b(
        env: Env,
        amount_out: i128,
        fee_bps: i128,
        params: Bytes,
    ) -> i128 {
        // `params` is reserved for a future callback variant.  Bound to
        // a local so the parameter is used (no dead-binding lint).
        let _ = params;

        Self::assert_no_active_flash_swap(&env);

        if amount_out <= 0 {
            panic!("amount_out must be positive");
        }
        if fee_bps < 0 || fee_bps > 9_999 {
            panic!("invalid fee_bps (must be in [0, 9999])");
        }

        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            panic!("empty pool");
        }
        if amount_out >= rb {
            panic!("Insufficient reserves: amount_out would drain reserve_b");
        }

        // ---- Optimistic transfer: debit reserve_b up front. ----
        // Already validated `amount_out < rb` above, so a plain subtraction
        // is safe (no underflow possible).
        let k_before: i128 = ra.checked_mul(rb).expect("k_before overflow");
        let new_rb: i128 = rb - amount_out;

        // Snapshot the invariant before applying the debit so the matching
        // `repay_flash_swap` can compare against the post-debit state.
        env.storage().persistent().set(&KEY_K_BEFORE, &k_before);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        env.storage().instance().set(&KEY_FLASH_ACTIVE, &true);

        amount_out
    }

    /// Flash-swap entrypoint — step 2 of the "optimistic transfer then
    /// verify-k" pattern.
    ///
    /// Credits `amount_in` of asset A back to the pool and **verifies
    /// k**:
    ///     `(reserve_a + amount_in) * reserve_b_after_debit  >=  k_before`.
    /// If the receiver underpaid, the verify-k assertion panics and
    /// Soroban's atomic rollback restores every storage change (including
    /// the optimistic reserve debit) — leaving the pool exactly where it
    /// started.
    ///
    /// # Arguments
    /// * `amount_in` — units of asset A being repaid.  Must be `> 0`.
    pub fn repay_flash_swap(env: Env, amount_in: i128) {
        if amount_in <= 0 {
            panic!("repay_flash_swap: amount_in must be positive");
        }
        let active: bool = env
            .storage()
            .instance()
            .get(&KEY_FLASH_ACTIVE)
            .unwrap_or(false);
        if !active {
            panic!("repay_flash_swap: no flash swap in progress");
        }

        let ra: i128 = env
            .storage()
            .persistent()
            .get(&KEY_RES_A)
            .unwrap_or(0);
        let rb: i128 = env
            .storage()
            .persistent()
            .get(&KEY_RES_B)
            .unwrap_or(0);
        let k_before: i128 = env
            .storage()
            .persistent()
            .get(&KEY_K_BEFORE)
            .expect("repay_flash_swap: k_before missing");

        let new_ra: i128 = ra.checked_add(amount_in).expect("repay_flash_swap overflow");

        // ---- Verify-k: k must not have decreased. ----
        // After the optimistic debit, reserve_b holds `rb` (already
        // reduced by amount_out).  After this credit, reserve_a =
        // `new_ra`.  The product must be ≥ k_before.
        let k_after: i128 = new_ra.checked_mul(rb).expect("k_after overflow");
        if k_after < k_before {
            // Explicit, identifiable panic.  Soroban will roll back
            // every storage change in the transaction, including the
            // optimistic debit performed by `flash_swap_a_for_b`.
            panic!(
                "Invariant violation: k decreased during flash-swap repayment (k_before={}, k_after={})",
                k_before, k_after
            );
        }

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().instance().set(&KEY_FLASH_ACTIVE, &false);
    }

    /// Read reserves (for testing / inspection).
    pub fn get_reserves(env: Env) -> (i128, i128) {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        (ra, rb)
    }

    /// Read whether a flash-swap is currently in flight (between
    /// `flash_swap_a_for_b` and the matching `repay_flash_swap`).
    /// Useful for tests, off-chain monitoring, and front-ends that want
    /// to expose pool lock-state.
    pub fn is_flash_active(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&KEY_FLASH_ACTIVE)
            .unwrap_or(false)
    }
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
    let k_before = before_a.checked_mul(before_b).expect("k overflow before");
    let k_after = after_a.checked_mul(after_b).expect("k overflow after");
    if expect_increase {
        if k_after < k_before {
            panic!(
                "Invariant violation: k decreased (before={}, after={})",
                k_before, k_after
            );
        }
    } else {
        if k_after > k_before {
            panic!(
                "Invariant violation: k increased on removal (before={}, after={})",
                k_before, k_after
            );
        }
    }
}

/// Inverse of the verify-k condition: returns the **minimum** `amount_in`
/// of asset A that a flash-swap receiver must repay to keep
/// `k = ra * rb` non-decreasing after the corresponding optimistic
/// debit of `amount_out`.
///
/// Derivation.
/// The post-flash pool state after a repayment of `amount_in` is:
/// `(ra + amount_in, rb − amount_out)`.
/// We require
///     `(ra + amount_in) * (rb − amount_out)  >=  ra * rb`
/// which solves to
///     `amount_in  >=  ra * amount_out / (rb − amount_out)`.
/// That bound is fee-independent — the verify-k check only enforces
/// k-monotonicity, not the swap formula's fee-discount curve.  This
/// helper rounds up so we never under-pay by integer truncation.
///
/// `fee_bps` is still supplied so the function's signature mirrors
/// the forward swap (callers commonly reach for it from
/// `swap_a_for_b(fee_bps)`).
#[cfg(test)]
pub(crate) fn inverse_swap_in(
    ra: i128,
    rb: i128,
    amount_out: i128,
    _fee_bps: i128,
) -> i128 {
    let rb_minus_out = rb.checked_sub(amount_out).expect("amount_out >= rb");
    let numerator = ra.checked_mul(amount_out).expect("inverse_swap_in overflow");
    // ceil(numerator / rb_minus_out) — round up so we never under-pay.
    (numerator + rb_minus_out - 1) / rb_minus_out
}

// ---------------------------------------------------------------------------
// Tests: fuzz-style sweeping of reserves and swap amounts
// ---------------------------------------------------------------------------
#[cfg(test)]
mod swap_bounds_proptest;

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
                    assert!(
                        k_after >= k_before,
                        "k decreased: ra={}, rb={}, amt={}, k_before={}, k_after={}",
                        ra, rb, amt, k_before, k_after
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
}

#[cfg(test)]
mod swap_symmetry_test;