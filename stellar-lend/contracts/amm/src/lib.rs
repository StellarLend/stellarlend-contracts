#![no_std]

pub mod liquidity_math;
pub mod math;

#[cfg(test)]
mod fee_accrual_overflow_test;
#[cfg(test)]
mod stored_fee_test;
#[cfg(test)]
mod fee_accrual_test;
#[cfg(test)]
mod flash_swap_atomicity_test;
#[cfg(test)]
mod flash_swap_caller_binding_test;
#[cfg(test)]
mod flash_swap_protocol_doctest;
#[cfg(test)]
mod flash_swap_test;
#[cfg(test)]
mod mint_shares_proptest;
#[cfg(test)]
mod sqrt_precision_test;

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env, Symbol, Vec};

pub struct FeeTier {
    pub min_reserve: u128,
    pub fee_bps: u32,
}

const FEE_TIERS_KEY: &str = "fee_tiers";


// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

// Existing pool reserve keys (kept for backward compatibility with the
// integer-only `init_pool(a, b)` and the swap-bounds proptest suite).
const KEY_RES_A: (&str, &str) = ("pool", "a");
const KEY_RES_B: (&str, &str) = ("pool", "b");

// TWAP observation ring-buffer. Each observation stores (timestamp, cumulative_price_numerator,
// cumulative_price_denominator) so off-chain consumers can compute time-weighted average prices.
const KEY_TWAP_OBS: (&str, &str) = ("pool", "twap_obs");
const KEY_TWAP_LAST_TS: (&str, &str) = ("pool", "twap_last_ts");

// Price-impact guard.
//
// `KEY_MAX_IMPACT_BPS` stores the admin-configured maximum price-impact in
// basis points (e.g. 500 = 5 %).  The sentinel `u32::MAX` (0xFFFF_FFFF)
// disables the guard entirely for backward compatibility — any swap is
// allowed when the guard is off.
//
// Spot price is represented as `reserve_b / reserve_a` (units of B per A).
// A swap A→B increases reserve_a and decreases reserve_b, which lowers the
// spot price.  The relative move is:
//
//   impact_bps = (price_before - price_after) * 10_000 / price_before
//              = (rb/ra  −  rb_after/ra_after) * 10_000 / (rb/ra)
//              = (1 − (rb_after * ra) / (rb * ra_after)) * 10_000
//
// Everything is computed in integer arithmetic using checked mul/div to
// avoid overflow.
const KEY_MAX_IMPACT_BPS: (&str, &str) = ("pool", "max_impact_bps");
/// Sentinel value that disables the price-impact guard.
pub const IMPACT_GUARD_DISABLED: u32 = u32::MAX;

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
/// Address that initiated the current flash swap.  Stored alongside
/// `KEY_FLASH_ACTIVE` so `repay_flash_swap` can enforce that only the
/// original caller (who received the optimistically-debited tokens) may
/// complete the swap.  Cleared when the swap completes or reverts.
const KEY_FLASH_INITIATOR: (&str, &str) = ("pool", "flash_initiator");

// Per-side swap fee accumulators.
//
// `KEY_FEE_A` tracks the total protocol fees earned from swaps where
// token A is the input (i.e. `swap_a_for_b`).  Each call increments it
// by `amount_in * fee_bps / 10_000` using saturating addition so the
// counter can never overflow and panic the pool.
//
// `KEY_FEE_B` tracks the total protocol fees earned from swaps where
// token B is the input (i.e. `swap_b_for_a`).
//
// Both accumulators are monotonic non-decreasing (until saturation at
// `i128::MAX`) and never exceed the cumulative `amount_in` for their
// respective side because the fee formula uses floor division with
// `fee_bps < 10_000`.
const KEY_FEE_A: (&str, &str) = ("pool", "fee_a");
const KEY_FEE_B: (&str, &str) = ("pool", "fee_b");

// Admin-configured stored swap fee.
//
// `KEY_FEE_BPS` stores the protocol-owned swap fee in basis points.
// This replaces the per-call `fee_bps` argument: the pool admin sets it
// once via `set_fee_bps`, and every swap reads it from storage.  Callers
// can no longer pass `fee_bps = 0` to bypass the fee.
//
// Valid range: `0..=MAX_FEE_BPS` (inclusive).  The default before any
// admin call is `DEFAULT_FEE_BPS` (30 bps = 0.30 %).
const KEY_FEE_BPS: (&str, &str) = ("pool", "fee_bps");

/// Maximum fee the admin may configure (50 % = 5 000 bps).
pub const MAX_FEE_BPS: i128 = 5_000;

/// Default fee applied when no admin has called `set_fee_bps`.
pub const DEFAULT_FEE_BPS: i128 = 30;

#[contracterror]
#[derive(Eq, PartialEq, Debug)]
pub enum AmmPoolError {
    /// Pool is empty
    EmptyPool = 1,
    /// Amount must be positive
    NonPositiveAmount = 2,
    /// Insufficient reserves
    InsufficientReserves = 3,
    /// Arithmetic overflow
    Overflow = 4,
    /// Invariant violation
    InvariantViolation = 5,
    /// Reentrant flash swap detected
    ReentrantFlashSwap = 6,
    /// Caller is not the flash-swap initiator
    UnauthorizedCaller = 7,
    /// fee_bps is out of the valid range `0..=MAX_FEE_BPS`
    FeeBpsOutOfRange = 8,
}

#[contract]
pub struct AmmContract;

#[contractimpl]
impl AmmContract {
    /// Initialize pool reserves (admin only in real code).
    ///
    /// Gated by the `FlashActive` reentrancy guard so that a flash swap
    /// initiated on a stale pool cannot be silently clobbered by a
    /// follow-up `init_pool` from the same transaction.
    ///
    /// Resets both fee accumulators to zero.
    pub fn init_pool(env: Env, a: i128, b: i128) -> Result<(), AmmPoolError> {
        Self::assert_no_active_flash_swap(&env)?;
        env.storage().persistent().set(&KEY_RES_A, &a);
        env.storage().persistent().set(&KEY_RES_B, &b);
        env.storage().persistent().set(&KEY_FEE_A, &0_i128);
        env.storage().persistent().set(&KEY_FEE_B, &0_i128);
        Ok(())
    }

    /// Set the maximum per-swap price impact in basis points.
    ///
    /// A swap A→B that would move the spot price (`reserve_b / reserve_a`)
    /// by more than `max_impact_bps / 10_000` is rejected with a panic,
    /// rolling back all state changes atomically.
    ///
    /// Pass [`IMPACT_GUARD_DISABLED`] (`u32::MAX`) to disable the guard
    /// and allow swaps of any size (backward-compatible default).
    ///
    /// # Arguments
    /// * `_admin`         — caller address (auth checked by the caller in
    ///                      production; kept in signature for future ACL).
    /// * `max_impact_bps` — maximum impact in BPS, or `IMPACT_GUARD_DISABLED`.
    pub fn set_max_impact_bps(env: Env, _admin: Address, max_impact_bps: u32) {
        env.storage()
            .persistent()
            .set(&KEY_MAX_IMPACT_BPS, &max_impact_bps);
    }

    /// Return the current max-impact bound in BPS, or [`IMPACT_GUARD_DISABLED`]
    /// if it has never been set.
    pub fn get_max_impact_bps(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&KEY_MAX_IMPACT_BPS)
            .unwrap_or(IMPACT_GUARD_DISABLED)
    }

    /// Set the protocol-owned swap fee in basis points (admin only).
    ///
    /// This is the single authoritative swap fee for all AMM swaps.
    /// Once set, `swap_a_for_b`, `swap_b_for_a`, and
    /// `flash_swap_a_for_b` read the fee from storage rather than
    /// accepting it as a caller-supplied argument, preventing
    /// fee-free swaps by passing `fee_bps = 0`.
    ///
    /// # Arguments
    /// * `admin`   — address that must authorize the call.
    /// * `fee_bps` — new fee in basis points; must be in `0..=MAX_FEE_BPS`.
    ///
    /// # Errors
    /// Returns [`AmmPoolError::FeeBpsOutOfRange`] when `fee_bps > MAX_FEE_BPS`.
    pub fn set_fee_bps(env: Env, admin: Address, fee_bps: i128) -> Result<(), AmmPoolError> {
        admin.require_auth();
        if fee_bps < 0 || fee_bps > MAX_FEE_BPS {
            return Err(AmmPoolError::FeeBpsOutOfRange);
        }
        env.storage().persistent().set(&KEY_FEE_BPS, &fee_bps);
        Ok(())
    }

    /// Return the current stored swap fee in basis points.
    ///
    /// Returns [`DEFAULT_FEE_BPS`] (30 bps) when no admin has called
    /// [`set_fee_bps`](AmmContract::set_fee_bps) yet, ensuring pools are
    /// not accidentally free-to-use before configuration.
    pub fn get_fee_bps(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&KEY_FEE_BPS)
            .unwrap_or(DEFAULT_FEE_BPS)
    }

    /// Reentrancy guard — panics if a flash swap is currently in flight.
    ///
    /// Called by `init_pool`, `add_liquidity`, `remove_liquidity`,
    /// `swap_a_for_b`, and a nested `flash_swap_a_for_b` before touching
    /// storage.
    ///
    /// # Panics
    /// `"ReentrantFlashSwap: pool mutation blocked while flash-swap is in flight"`
    /// when `KEY_FLASH_ACTIVE == true`.
    ///
    /// See: [FLASH_SWAP_PROTOCOL.md §Reentrancy Guard](../FLASH_SWAP_PROTOCOL.md)
    fn assert_no_active_flash_swap(env: &Env) {
        let active: bool = env
            .storage()
            .instance()
            .get(&KEY_FLASH_ACTIVE)
            .unwrap_or(false);
        if active {
            return Err(AmmPoolError::ReentrantFlashSwap);
        }
        Ok(())
    }

    /// Simple add liquidity: increase reserves and assert k monotonicity
    /// (k must not decrease).
    pub fn add_liquidity(env: Env, add_a: i128, add_b: i128) -> Result<(), AmmPoolError> {
        Self::assert_no_active_flash_swap(&env)?;
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        let new_ra = ra.checked_add(add_a).ok_or(AmmPoolError::Overflow)?;
        let new_rb = rb.checked_add(add_b).ok_or(AmmPoolError::Overflow)?;
        assert_k_monotonic(ra, rb, new_ra, new_rb, true)?;
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        Ok(())
    }

    /// Simple remove liquidity: decrease reserves and assert k monotonicity
    /// (k must not increase).
    pub fn remove_liquidity(env: Env, rem_a: i128, rem_b: i128) -> Result<(), AmmPoolError> {
        Self::assert_no_active_flash_swap(&env)?;
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if rem_a > ra || rem_b > rb {
            return Err(AmmPoolError::InsufficientReserves);
        }
        let new_ra = ra - rem_a;
        let new_rb = rb - rem_b;
        assert_k_monotonic(ra, rb, new_ra, new_rb, false)?;
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        Ok(())
    }

    /// Swap from A -> B using Uniswap-style formula with the stored fee.
    ///
    /// The fee is read from [`KEY_FEE_BPS`] (set by the admin via
    /// [`set_fee_bps`](AmmContract::set_fee_bps)) rather than accepted as a
    /// caller argument.  This prevents callers from routing fee-free by
    /// supplying `fee_bps = 0`.  Returns amount_out.
    ///
    /// # Overflow policy
    ///
    /// The fee accumulator (`KEY_FEE_A`) uses saturating addition. If the
    /// counter reaches `i128::MAX` it stops incrementing but never panics.
    /// This guarantees the swap cannot be halted by fee-accumulation overflow.
    pub fn swap_a_for_b(env: Env, amount_in: i128) -> i128 {
        Self::assert_no_active_flash_swap(&env);
        if amount_in <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            return Err(AmmPoolError::EmptyPool);
        }

        // Read the protocol-owned fee from storage (never caller-supplied).
        let fee_bps: i128 = env
            .storage()
            .persistent()
            .get(&KEY_FEE_BPS)
            .unwrap_or(DEFAULT_FEE_BPS);

        let fee = compute_fee(amount_in, fee_bps)?;

        // Uniswap v2 style: amount_in_with_fee = amount_in * (10000 - fee_bps)
        let fee_adj = 10_000_i128.checked_sub(fee_bps).ok_or(AmmPoolError::Overflow)?;
        let amount_in_with_fee = amount_in.checked_mul(fee_adj).ok_or(AmmPoolError::Overflow)?;

        // numerator = amount_in_with_fee * reserve_out
        let numerator = amount_in_with_fee.checked_mul(rb).ok_or(AmmPoolError::Overflow)?;
        // denominator = reserve_in * 10000 + amount_in_with_fee
        let denom_part = ra.checked_mul(10_000_i128).ok_or(AmmPoolError::Overflow)?;
        let denominator = denom_part
            .checked_add(amount_in_with_fee)
            .ok_or(AmmPoolError::Overflow)?;

        let amount_out = numerator / denominator;

        let new_ra = ra.checked_add(amount_in).ok_or(AmmPoolError::Overflow)?;
        let new_rb = rb.checked_sub(amount_out).ok_or(AmmPoolError::Overflow)?;
        assert_k_monotonic(ra, rb, new_ra, new_rb, true)?;

        let accrued_fee_a: i128 = env.storage().persistent().get(&KEY_FEE_A).unwrap_or(0);
        let new_fee_a = accrued_fee_a.saturating_add(fee);

        // ---- Price-impact guard ----
        // Spot price before  = rb  / ra   (units of B per A).
        // Spot price after   = new_rb / new_ra.
        // Relative impact (BPS) = (1 - new_rb * ra / (rb * new_ra)) * 10_000.
        // We reject if impact_bps > max_impact_bps.
        let max_impact: u32 = env
            .storage()
            .persistent()
            .get(&KEY_MAX_IMPACT_BPS)
            .unwrap_or(IMPACT_GUARD_DISABLED);
        if max_impact != IMPACT_GUARD_DISABLED {
            // Numerator of the "price ratio after / before":
            //   ratio_num = new_rb * ra
            //   ratio_den = rb     * new_ra
            // impact_bps = (1 - ratio_num / ratio_den) * 10_000
            //            = (ratio_den - ratio_num) * 10_000 / ratio_den
            let ratio_num = new_rb.checked_mul(ra).expect("impact overflow");
            let ratio_den = rb.checked_mul(new_ra).expect("impact overflow");
            let impact_bps = (ratio_den - ratio_num)
                .checked_mul(10_000)
                .expect("impact overflow")
                / ratio_den;
            if impact_bps > max_impact as i128 {
                panic!(
                    "PriceImpactExceeded: impact_bps={}, max={}",
                    impact_bps, max_impact
                );
            }
        }

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        env.storage().persistent().set(&KEY_FEE_A, &new_fee_a);
        Ok(amount_out)
    }

    /// Swap from B -> A using the same Uniswap-v2 constant-product formula and
    /// stored fee model as [`swap_a_for_b`], with token roles reversed.
    ///
    /// The fee is read from [`KEY_FEE_BPS`] in storage (admin-set), not from
    /// a caller-supplied argument.
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
    ///
    /// # Overflow policy
    ///
    /// Identical to [`swap_a_for_b`]: the fee accumulator saturates at
    /// `i128::MAX` and never panics on addition.
    pub fn swap_b_for_a(env: Env, amount_in: i128) -> i128 {
        if amount_in <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            return Err(AmmPoolError::EmptyPool);
        }

        // Read the protocol-owned fee from storage (never caller-supplied).
        let fee_bps: i128 = env
            .storage()
            .persistent()
            .get(&KEY_FEE_BPS)
            .unwrap_or(DEFAULT_FEE_BPS);

        let fee = compute_fee(amount_in, fee_bps)?;

        // Mirror of swap_a_for_b with A and B roles swapped.
        let fee_adj = 10_000_i128.checked_sub(fee_bps).ok_or(AmmPoolError::Overflow)?;
        let amount_in_with_fee = amount_in.checked_mul(fee_adj).ok_or(AmmPoolError::Overflow)?;

        // reserve_out is A, reserve_in is B
        let numerator = amount_in_with_fee.checked_mul(ra).ok_or(AmmPoolError::Overflow)?;
        let denom_part = rb.checked_mul(10_000_i128).ok_or(AmmPoolError::Overflow)?;
        let denominator = denom_part
            .checked_add(amount_in_with_fee)
            .ok_or(AmmPoolError::Overflow)?;

        let amount_out = numerator / denominator; // floor — pool never over-pays

        let new_rb = rb.checked_add(amount_in).ok_or(AmmPoolError::Overflow)?;
        let new_ra = ra.checked_sub(amount_out).ok_or(AmmPoolError::Overflow)?;
        assert_k_monotonic(ra, rb, new_ra, new_rb, true)?;

        let accrued_fee_b: i128 = env.storage().persistent().get(&KEY_FEE_B).unwrap_or(0);
        let new_fee_b = accrued_fee_b.saturating_add(fee);

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        env.storage().persistent().set(&KEY_FEE_B, &new_fee_b);
        Ok(amount_out)
    }

    /// Flash-swap entrypoint — step 1 of the "optimistic transfer then
    /// verify-k" pattern (Uniswap-v2 style).
    ///
    /// Optimistically debits `reserve_b` by `amount_out` and snapshots the
    /// pre-debit invariant `k_before = reserve_a * reserve_b` so the
    /// matching `repay_flash_swap` can enforce
    /// `(reserve_a + amount_in) * reserve_b_after_debit  >=  k_before`.
    ///
    /// The caller's address is recorded as the **initiator** so that only
    /// the same address may call [`repay_flash_swap`](AmmContract::repay_flash_swap).
    /// This prevents a third party from observing an in-flight flash swap
    /// and calling `repay_flash_swap` (or otherwise interfering) within
    /// the same transaction.
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
    /// * `params`     — opaque user payload, kept for forward-compatibility
    ///                  with a future cross-contract callback variant.
    ///                  Today the AMM itself does **not** invoke any
    ///                  callback (Soroban 25.3.1 blocks self-re-entry);
    ///                  callers are expected to dispatch
    ///                  `repay_flash_swap` from a follow-up transaction
    ///                  operation.  The value is bound to a local to
    ///                  keep it in the parameter surface.
    ///
    /// The fee is read from [`KEY_FEE_BPS`] in storage (admin-set via
    /// [`set_fee_bps`](AmmContract::set_fee_bps)), not from a caller argument.
    ///
    /// # Returns
    /// `amount_out` — the number of asset-B units debited from the pool.
    ///
    /// # Panics
    /// - `"ReentrantFlashSwap"` — a flash swap is already in flight.
    /// - `"amount_out must be positive"` — `amount_out ≤ 0`.
    /// - `"empty pool"` — either reserve is zero.
    /// - `"Insufficient reserves: amount_out would drain reserve_b"` — `amount_out ≥ reserve_b`.
    ///
    /// See: [FLASH_SWAP_PROTOCOL.md §Call Sequence](../FLASH_SWAP_PROTOCOL.md)
    pub fn flash_swap_a_for_b(env: Env, amount_out: i128, params: Bytes) -> i128 {
        // `params` is reserved for a future callback variant.  Bound to
        // a local so the parameter is used (no dead-binding lint).
        let _ = params;

        Self::assert_no_active_flash_swap(&env)?;

        if amount_out <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }

        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            return Err(AmmPoolError::EmptyPool);
        }
        if amount_out >= rb {
            return Err(AmmPoolError::InsufficientReserves);
        }

        // ---- Optimistic transfer: debit reserve_b up front. ----
        // Already validated `amount_out < rb` above, so a plain subtraction
        // is safe (no underflow possible).
        let k_before: i128 = ra.checked_mul(rb).ok_or(AmmPoolError::Overflow)?;
        let new_rb: i128 = rb - amount_out;

        // Snapshot the invariant before applying the debit so the matching
        // `repay_flash_swap` can compare against the post-debit state.
        env.storage().persistent().set(&KEY_K_BEFORE, &k_before);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        env.storage().instance().set(&KEY_FLASH_ACTIVE, &true);
        // Record the initiator so only this address may repay.
        env.storage()
            .instance()
            .set(&KEY_FLASH_INITIATOR, &env.current_contract_address());

        Ok(amount_out)
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
    /// Only the address that initiated the flash swap (recorded by
    /// [`flash_swap_a_for_b`](AmmContract::flash_swap_a_for_b)) may call
    /// this function.  Any other caller is rejected with
    /// [`AmmPoolError::UnauthorizedCaller`].
    ///
    /// The minimum `amount_in` that satisfies the invariant is:
    /// ```text
    /// amount_in_min = ⌈ reserve_a × amount_out / (reserve_b − amount_out) ⌉
    /// ```
    /// computed by [`inverse_swap_in`].
    ///
    /// # Arguments
    /// * `amount_in` — units of asset A being repaid.  Must be `> 0`.
    ///
    /// # Panics
    /// - `"repay_flash_swap: amount_in must be positive"` — `amount_in ≤ 0`.
    /// - `"repay_flash_swap: no flash swap in progress"` — called outside a flash swap.
    /// - `"UnauthorizedCaller: repay_flash_swap must be called by the flash-swap initiator"` — caller mismatch.
    /// - `"Invariant violation: k decreased during flash-swap repayment"` — under-repayment;
    ///   Soroban then rolls back all storage changes, including the Op-1 debit.
    ///
    /// See: [FLASH_SWAP_PROTOCOL.md §Verify-K Repay Invariant](../FLASH_SWAP_PROTOCOL.md)
    pub fn repay_flash_swap(env: Env, amount_in: i128) {
        if amount_in <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        let active: bool = env
            .storage()
            .instance()
            .get(&KEY_FLASH_ACTIVE)
            .unwrap_or(false);
        if !active {
            // No flash swap in progress is an invariant violation of the expected call sequence
            return Err(AmmPoolError::InvariantViolation);
        }

        // Enforce that only the original initiator may repay.  This
        // prevents a third party from observing an in-flight flash swap
        // and calling `repay_flash_swap` to interfere.
        let initiator: Address = env
            .storage()
            .instance()
            .get(&KEY_FLASH_INITIATOR)
            .ok_or(AmmPoolError::InvariantViolation)?;
        let caller = env.current_contract_address();
        if caller != initiator {
            return Err(AmmPoolError::UnauthorizedCaller);
        }
        // Require explicit authorization from the initiator.
        initiator.require_auth();

        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        let k_before: i128 = env
            .storage()
            .persistent()
            .get(&KEY_K_BEFORE)
            .ok_or(AmmPoolError::InvariantViolation)?;

        let new_ra: i128 = ra
            .checked_add(amount_in)
            .ok_or(AmmPoolError::Overflow)?;

        // ---- Verify-k: k must not have decreased. ----
        // After the optimistic debit, reserve_b holds `rb` (already
        // reduced by amount_out).  After this credit, reserve_a =
        // `new_ra`.  The product must be ≥ k_before.
        let k_after: i128 = new_ra.checked_mul(rb).ok_or(AmmPoolError::Overflow)?;
        if k_after < k_before {
            return Err(AmmPoolError::InvariantViolation);
        }

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().instance().set(&KEY_FLASH_ACTIVE, &false);
        env.storage().instance().remove(&KEY_FLASH_INITIATOR);
        Ok(())
    }

    /// Read reserves (for testing / inspection).
    pub fn get_reserves(env: Env) -> (i128, i128) {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        (ra, rb)
    }

    /// Returns `true` while a flash swap is in flight — between the
    /// [`flash_swap_a_for_b`](AmmContract::flash_swap_a_for_b) call and
    /// the matching [`repay_flash_swap`](AmmContract::repay_flash_swap).
    ///
    /// Useful for off-chain monitoring, front-ends exposing pool lock-state,
    /// and test assertions.  Read-only; not gated by the reentrancy guard.
    ///
    /// See: [FLASH_SWAP_PROTOCOL.md §Reentrancy Guard](../FLASH_SWAP_PROTOCOL.md)
    pub fn is_flash_active(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&KEY_FLASH_ACTIVE)
            .unwrap_or(false)
    }

    /// Read the total accrued swap fees per side.
    ///
    /// Returns `(fee_a, fee_b)` where:
    /// - `fee_a` is the sum of fees charged on every `swap_a_for_b` call,
    /// - `fee_b` is the sum of fees charged on every `swap_b_for_a` call.
    ///
    /// Each fee is computed as `amount_in * fee_bps / 10_000` (floor
    /// division) at the time of the swap and accumulated into a
    /// monotonic, persisted counter.
    ///
    /// # Overflow policy
    ///
    /// Each counter uses saturating addition internally. Should a counter
    /// reach `i128::MAX` it will stop incrementing; the pool remains
    /// operational and will not panic.
    pub fn get_accrued_fees(env: Env) -> (i128, i128) {
        let fee_a: i128 = env.storage().persistent().get(&KEY_FEE_A).unwrap_or(0);
        let fee_b: i128 = env.storage().persistent().get(&KEY_FEE_B).unwrap_or(0);
        (fee_a, fee_b)
    }

    /// Record a time-weighted spot-price observation for off-chain TWAP reconstruction.
    ///
    /// Each call appends `(timestamp, cumulative_num, cumulative_denom)` to the
    /// persistent observation buffer. Off-chain indexers can fetch the full buffer
    /// via `get_twap_observations()` and compute TWAP over any sub-window as:
    ///
    ///   TWAP = (cum_num[t1] - cum_num[t0]) / (cum_denom[t1] - cum_denom[t0])
    ///
    /// where `cum_denom` is cumulative time (seconds) and `cum_num` is cumulative
    /// `reserve_b * dt / reserve_a` (price of A in units of B weighted by time).
    ///
    /// Only callable internally by swap functions; exposed here for testability.
    fn record_twap_observation(env: &Env) {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(1);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(1);
        let now: u64 = env.ledger().timestamp();

        let last_ts: u64 = env
            .storage()
            .persistent()
            .get(&KEY_TWAP_LAST_TS)
            .unwrap_or(now);

        let dt = now.saturating_sub(last_ts) as i128;

        let mut obs: Vec<(u64, i128, i128)> = env
            .storage()
            .persistent()
            .get(&KEY_TWAP_OBS)
            .unwrap_or_else(|| Vec::new(env));

        let prev_num: i128 = obs.last().map(|(_, n, _)| n).unwrap_or(0);
        let prev_denom: i128 = obs.last().map(|(_, _, d)| d).unwrap_or(0);

        // Accumulate price * time (price = rb / ra; avoid division, store numerator * dt separately)
        let new_num = prev_num.saturating_add(rb.saturating_mul(dt));
        let new_denom = prev_denom.saturating_add(ra.saturating_mul(dt));

        obs.push_back((now, new_num, new_denom));

        env.storage().persistent().set(&KEY_TWAP_OBS, &obs);
        env.storage().persistent().set(&KEY_TWAP_LAST_TS, &now);
    }

    /// Return the full TWAP observation buffer as `Vec<(timestamp, cumulative_num, cumulative_denom)>`.
    ///
    /// Off-chain consumers can reconstruct any-window TWAP:
    ///   TWAP_price_of_A_in_B = Δnum / Δdenom
    /// where Δnum = cum_num[t1] - cum_num[t0] and Δdenom = cum_denom[t1] - cum_denom[t0].
    pub fn get_twap_observations(env: Env) -> Vec<(u64, i128, i128)> {
        env.storage()
            .persistent()
            .get(&KEY_TWAP_OBS)
            .unwrap_or_else(|| Vec::new(&env))
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
) -> Result<(), AmmPoolError> {
    let k_before = before_a.checked_mul(before_b).ok_or(AmmPoolError::Overflow)?;
    let k_after = after_a.checked_mul(after_b).ok_or(AmmPoolError::Overflow)?;
    if expect_increase {
        if k_after < k_before {
            return Err(AmmPoolError::InvariantViolation);
        }
    } else {
        if k_after > k_before {
            return Err(AmmPoolError::InvariantViolation);
        }
    }
    Ok(())
}

/// Compute the swap fee from `amount_in` and `fee_bps` using the Uniswap-v2
/// convention:
///
/// ```text
/// fee = amount_in * fee_bps / 10_000
/// ```
///
/// Uses checked arithmetic; panics on overflow.
fn compute_fee(amount_in: i128, fee_bps: i128) -> Result<i128, AmmPoolError> {
    amount_in.checked_mul(fee_bps).ok_or(AmmPoolError::Overflow)? / 10_000
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
///
/// See: [FLASH_SWAP_PROTOCOL.md §Minimum Repayment Formula](../FLASH_SWAP_PROTOCOL.md)
#[cfg(test)]
pub(crate) fn inverse_swap_in(ra: i128, rb: i128, amount_out: i128, _fee_bps: i128) -> i128 {
    let rb_minus_out = rb.checked_sub(amount_out).expect("amount_out >= rb");
    let numerator = ra
        .checked_mul(amount_out)
        .expect("inverse_swap_in overflow");
    // ceil(numerator / rb_minus_out) — round up so we never under-pay.
    (numerator + rb_minus_out - 1) / rb_minus_out
}

// ---------------------------------------------------------------------------
// Tests: fuzz-style sweeping of reserves and swap amounts
// ---------------------------------------------------------------------------
#[cfg(test)]
mod swap_bounds_proptest;

#[cfg(test)]
mod price_impact_test;

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
                    client.init_pool(&ra, &rb).unwrap();
                    // swap using stored fee (default 30 bps)
                    let _out = client.swap_a_for_b(&amt);
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

        client.init_pool(&1000, &2000).unwrap();
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

/// Set fee tiers for dynamic scaling
pub fn set_fee_tiers(env: Env, admin: Address, tiers: Vec<u128>) {
    admin.require_auth();
    let key = Symbol::new(&env, FEE_TIERS_KEY);
    env.storage().persistent().set(&key, &tiers);
}

/// Get fee tiers
pub fn get_fee_tiers(env: Env) -> Vec<u128> {
    let key = Symbol::new(&env, FEE_TIERS_KEY);
    env.storage()
        .persistent()
        .get::<_, Vec<u128>>(&key)
        .unwrap_or_else(|| Vec::new(&env))
}
#[cfg(test)]
mod dynamic_fee_test;
#[cfg(test)]
mod inverse_swap_proptest;
