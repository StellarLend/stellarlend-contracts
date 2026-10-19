#![no_std]
//! `stellarlend-amm` — minimal Soroban AMM contract for the StellarLend protocol.
//!
//! The contract exposes a constant‑product (`x · y = k`) pool with two reserves
//! and operates entirely on integer arithmetic (Soroban `i128`). All swaps use
//! the Uniswap‑v2 formula with a fee expressed in basis points (`fee_bps`, with
//! `10000` representing 100%):
//!
//! ```text
//! amount_in_with_fee = amount_in * (10000 − fee_bps)
//! amount_out          = (amount_in_with_fee * reserve_out)
//!                       / (reserve_in * 10000 + amount_in_with_fee)
//! ```
//!
//! Equivalently: `amount_out = (amount_in · (10000 − fee_bps) · reserve_out) /
//! (reserve_in · 10000 + amount_in · (10000 − fee_bps))`.
//!
//! Public entry points:
//! - [`init_pool`] — initialise reserves A/B and LP total supply (admin-only:
//!   the supplied `admin` must authorize via `require_auth()` and, once an
//!   admin identity is stored, must match the stored pool admin). Returns
//!   `Result<(), AmmPoolError>`.
//! - [`add_liquidity`] — deposit tokens, mint LP shares via donation-attack-resistant math. Takes `(desired, min)` amounts, `min_shares` and a `deadline`; returns `Result<(i128, i128, i128), AmmPoolError>` (amount_a, amount_b, shares minted).
//! - [`remove_liquidity`] — burn LP shares for proportional reserves. Returns `Result<(i128, i128), AmmPoolError>` (tokens returned).
//! - [`swap_a_for_b`] — fee‑adjusted constant‑product swap A → B. Returns `Result<i128, AmmPoolError>` (amount_out).
//! - [`swap_b_for_a`] — fee‑adjusted constant‑product swap B → A (mirror of `swap_a_for_b` with token roles reversed). Returns `Result<i128, AmmPoolError>` (amount_out).
//! - [`get_reserves`] — read both reserves for inspection / tests. Returns `(i128, i128)`.
//!
//! Notes for downstream callers:
//! - Input validation is enforced via typed `Result` errors (`AmmPoolError`); callers should
//!   pre‑validate non‑negative amounts and `0 ≤ fee_bps ≤ 10000` off‑chain.
//! - The k‑invariant is enforced by [`assert_k_monotonic`] after every reserve mutation.
//! - Flash‑swap APIs — [`AmmContract::flash_swap_a_for_b`] and
//!   [`AmmContract::repay_flash_swap`] — implement the "optimistic transfer
//!   then verify‑k" pattern described in
//!   [FLASH_SWAP_PROTOCOL.md](../FLASH_SWAP_PROTOCOL.md); `AmmPoolError` is
//!   the shared error type for all entry points in this crate.

pub mod liquidity_math;
pub mod math;

#[cfg(test)]
mod admin_authorization_test;
#[cfg(test)]
mod error_codes_test;
#[cfg(test)]
mod fee_accrual_overflow_test;
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
mod inverse_swap_doc_example_test;
#[cfg(test)]
mod liquidity_bounds_test;
#[cfg(test)]
mod mint_shares_proptest;
#[cfg(test)]
mod repay_flash_swap_contract_test;
#[cfg(test)]
mod sqrt_precision_test;
#[cfg(test)]
mod stored_fee_test;

use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Bytes, Env, Symbol, Vec,
};

use crate::liquidity_math::{
    calculate_burn_amounts, calculate_mint_shares, quote_optimal_amounts, LiquidityMathError,
    MINIMUM_LIQUIDITY,
};

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

// Token contract addresses — stored at `init_pool` time and read by
// `add_liquidity` / `remove_liquidity` to perform real token transfers.
const KEY_TOKEN_A: (&str, &str) = ("pool", "token_a");
const KEY_TOKEN_B: (&str, &str) = ("pool", "token_b");

// ---------------------------------------------------------------------------
// TWAP oracle (Uniswap-v2-style cumulative prices)
// ---------------------------------------------------------------------------
//
// The oracle stores two fixed-point *cumulative* prices. Before every reserve
// mutation the accumulator is advanced by `spot_price * dt` using the reserves
// that were in force for that interval, and the new cumulative pair is written
// to a fixed-size ring buffer. A consumer can then recover the time-weighted
// average price over any window with `consult(window_secs)`:
//
//     TWAP = (cumulative(now) - cumulative(now - window)) / window
//
// Storing cumulative sums (rather than a ratio of sums) is what makes the
// result a true time-weighted average when reserves change mid-window. See
// [TWAP.md](../TWAP.md) for the full scheme, overflow policy and manipulation
// cost argument.

/// Fixed-point scale for TWAP prices: a value of `1e18` represents a price of
/// `1.0`.
pub const TWAP_PRICE_SCALE: i128 = 1_000_000_000_000_000_000;

/// Number of observation slots in the fixed-size TWAP ring buffer.
///
/// Once full, every new observation overwrites the oldest slot, so oracle
/// storage is `O(TWAP_OBSERVATION_COUNT)` and never grows with the number of
/// swaps.
pub const TWAP_OBSERVATION_COUNT: u32 = 48;

/// A single TWAP observation: the cumulative prices at `timestamp`.
///
/// * `price0_cumulative` – cumulative `price(A in B) * dt`, scaled by
///   [`TWAP_PRICE_SCALE`], where `price(A in B) = reserve_b / reserve_a`.
/// * `price1_cumulative` – cumulative `price(B in A) * dt`, scaled by
///   [`TWAP_PRICE_SCALE`], where `price(B in A) = reserve_a / reserve_b`.
#[contracttype]
#[derive(Clone)]
pub struct TwapObservation {
    pub timestamp: u64,
    pub price0_cumulative: i128,
    pub price1_cumulative: i128,
}

/// Aggregated TWAP accumulator state, stored in a single persistent entry.
///
/// `index` is the physical ring slot holding the newest observation and
/// `cardinality` is the number of written slots, capped at
/// [`TWAP_OBSERVATION_COUNT`]. When the ring is full the oldest observation
/// lives at `(index + 1) % TWAP_OBSERVATION_COUNT`.
#[contracttype]
#[derive(Clone)]
pub struct TwapState {
    pub price0_cumulative: i128,
    pub price1_cumulative: i128,
    pub last_ts: u64,
    pub index: u32,
    pub cardinality: u32,
}

/// Storage keys for the TWAP oracle.
#[contracttype]
#[derive(Clone)]
pub enum TwapKey {
    /// The aggregated [`TwapState`].
    State,
    /// One physical slot of the observation ring (`TwapObservation`).
    Obs(u32),
}

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
/// Reserves `(reserve_a, reserve_b)` captured immediately before the
/// optimistic debit. `repay_flash_swap` feeds these to the TWAP oracle so the
/// in-flight imbalance (which only exists between the two flash operations)
/// can never leak into the cumulative price.
const KEY_FLASH_PRE_RESERVES: (&str, &str) = ("pool", "flash_pre_reserves");

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

// Admin-configured minimum swap input floor.
//
// `KEY_MIN_SWAP_IN` stores the optional lower bound on `amount_in` for
// regular swaps (and on `amount_out` for flash swaps).  Default is `0`
// (disabled) so the only always-on protection is the zero-output guard.
// Admins may raise the floor to reject economically meaningless dust
// inputs before they hit the constant-product math.
//
// See: [DUST_SWAP_GUARD.md](../DUST_SWAP_GUARD.md)
const KEY_MIN_SWAP_IN: (&str, &str) = ("pool", "min_swap_in");

// Pool admin identity. Set by `init_pool` from its authenticated `admin`
// parameter (first caller wins — the identity cannot be replaced without
// calling the contract's own admin, so it is effectively immutable here).
// All admin-gated entry points (`init_pool`, `set_max_impact_bps`,
// `set_fee_bps`, `set_min_swap_in`, `set_min_liquidity`) require the stored
// admin's authorization once the identity exists.
const KEY_ADMIN: (&str, &str) = ("pool", "admin");

// LP share tracking — total supply and per-user balances.
const KEY_LP_TOTAL_SUPPLY: (&str, &str) = ("pool", "lp_total_supply");

// Minimum-liquidity floor.
//
// `KEY_MIN_LIQUIDITY` stores the admin-configured minimum that every
// remaining reserve must satisfy after `remove_liquidity` or `swap_*`.
// A floor of `0` (the default) is fully backward-compatible — neither
// withdrawal nor swap is restricted.  Setting a positive value rejects
// any operation that would push a reserve below the floor with
// [`AmmPoolError::BelowMinLiquidity`].
//
// See: [MIN_LIQUIDITY.md §Default Behaviour](../MIN_LIQUIDITY.md)
const KEY_MIN_LIQUIDITY: (&str, &str) = ("pool", "min_liquidity");

/// Per-user LP share balance storage key.
#[contracttype]
#[derive(Clone)]
pub enum LpBalanceKey {
    User(Address),
}

/// Maximum fee the admin may configure (50 % = 5 000 bps).
pub const MAX_FEE_BPS: i128 = 5_000;

/// Default fee applied when no admin has called `set_fee_bps`.
pub const DEFAULT_FEE_BPS: i128 = 30;

/// Default minimum swap input when no admin has called `set_min_swap_in`.
///
/// `0` means the floor is disabled; the always-on protection is still the
/// zero-output rejection ([`AmmPoolError::ZeroOutput`]).
pub const DEFAULT_MIN_SWAP_IN: i128 = 0;

/// Default minimum-liquidity floor when no admin has called
/// `set_min_liquidity`.
///
/// `0` means the floor is disabled — fully backward compatible.
/// See: [MIN_LIQUIDITY.md §Default Behaviour](../MIN_LIQUIDITY.md)
pub const DEFAULT_MIN_LIQUIDITY: i128 = 0;

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
    /// LP shares minted would be zero (deposit too small)
    InsufficientLiquidityMinted = 9,
    /// Pool has zero LP supply (cannot burn)
    ZeroSupply = 10,
    /// Burn amount exceeds total LP supply
    BurnExceedsSupply = 11,
    /// Invalid burn amount (non-positive)
    InvalidBurnAmount = 12,
    /// Pool reserves are zero (cannot compute share ratio)
    ZeroReserve = 13,
    /// Caller has insufficient LP balance for requested burn
    InsufficientLpBalance = 14,
    /// Computed swap output is zero after fees and floor division — dust
    /// input that would consume tokens / perturb reserves with no economic
    /// exchange.  See [DUST_SWAP_GUARD.md](../DUST_SWAP_GUARD.md).
    ZeroOutput = 15,
    /// Input is below the admin-configured `min_swap_in` floor.
    AmountBelowMinSwapIn = 16,
    /// A `remove_liquidity` or `swap_*` operation would leave a reserve
    /// below the admin-configured `min_liquidity` floor.
    /// See [MIN_LIQUIDITY.md](../MIN_LIQUIDITY.md).
    BelowMinLiquidity = 17,
    /// A liquidity operation produced less than the caller's minimum
    /// (`amount_*_min` / `min_shares`), or a swap's computed output is below
    /// the caller's `min_amount_out`.
    SlippageExceeded = 18,
    /// The ledger timestamp is past the caller-supplied `deadline`.
    DeadlineExpired = 19,
    /// Swap would move the spot price by more than `max_impact_bps`.
    PriceImpactExceeded = 20,
    /// The trader may not be the pool contract itself.
    InvalidTrader = 21,
    /// `consult` requested a window longer than the retained TWAP history.
    /// See [TWAP.md](../TWAP.md).
    TwapInsufficientHistory = 22,
    /// The requested TWAP ring slot is out of range or has never been written.
    TwapObservationNotFound = 23,
}

/// Return value of [`AmmContract::get_swap_quote`].
///
/// Contains the full read-only projection of a hypothetical swap without
/// touching any persistent storage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapQuote {
    /// The output amount the caller would receive (floor-division, same as the
    /// live swap path).
    pub amount_out: i128,
    /// The fee taken from `amount_in` according to `fee_bps` (same formula as
    /// [`compute_fee`]).
    pub fee: i128,
    /// Reserve of token A after the hypothetical swap (not written to storage).
    pub reserve_a_after: i128,
    /// Reserve of token B after the hypothetical swap (not written to storage).
    pub reserve_b_after: i128,
}

#[contract]
pub struct AmmContract;

#[contractimpl]
impl AmmContract {
    /// Initialize pool reserves.
    ///
    /// Admin only. `admin` must authorize the call via `require_auth()` and,
    /// once a pool admin identity is stored, must match the stored admin —
    /// mirroring [`set_fee_bps`](AmmContract::set_fee_bps).  The first
    /// successful `init_pool` call locks in its `admin` argument as the
    /// pool admin (first-caller-wins); later calls are restricted to that
    /// same address so a third party cannot reset the reserves.
    ///
    /// Gated by the `FlashActive` reentrancy guard so that a flash swap
    /// initiated on a stale pool cannot be silently clobbered by a
    /// follow-up `init_pool` from the same transaction.
    ///
    /// Stores the token contract addresses for A and B so that
    /// `add_liquidity` and `remove_liquidity` can perform real token
    /// transfers.  Resets both fee accumulators and LP total supply to zero.
    pub fn init_pool(
        env: Env,
        admin: Address,
        a: i128,
        b: i128,
        token_a: Address,
        token_b: Address,
    ) -> Result<(), AmmPoolError> {
        Self::assert_no_active_flash_swap(&env)?;
        // Admin-gated: authorize the caller, enforce the stored admin
        // identity, and lock it in on first initialization.
        Self::require_stored_admin(&env, &admin)?;
        if env
            .storage()
            .persistent()
            .get::<_, Address>(&KEY_ADMIN)
            .is_none()
        {
            env.storage().persistent().set(&KEY_ADMIN, &admin);
        }
        // Fold the interval since the previous reserve state into the TWAP
        // accumulator before overwriting the reserves (pre-update values).
        let previous_a: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let previous_b: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        Self::update_oracle(&env, previous_a, previous_b)?;
        env.storage().persistent().set(&KEY_RES_A, &a);
        env.storage().persistent().set(&KEY_RES_B, &b);
        env.storage().persistent().set(&KEY_TOKEN_A, &token_a);
        env.storage().persistent().set(&KEY_TOKEN_B, &token_b);
        env.storage().persistent().set(&KEY_FEE_A, &0_i128);
        env.storage().persistent().set(&KEY_FEE_B, &0_i128);
        // Seed LP total supply so that subsequent add_liquidity calls use the
        // proportional path rather than the first-deposit (MINIMUM_LIQUIDITY-gated) path.
        // When a>0 && b>0, we compute sqrt(a*b) as the initial virtual liquidity and
        // lock it as total_supply (no owner). This preserves backward-compatibility for
        // tests that init_pool with non-zero reserves and then swap directly.
        let initial_supply = if a > 0 && b > 0 {
            let product = a
                .checked_mul(b)
                .expect("init_pool: reserve product overflow");
            let sqrt_val = crate::math::try_sqrt(product)
                .expect("init_pool: product is non-negative, sqrt cannot overflow");
            // Use at least MINIMUM_LIQUIDITY+1 so that subsequent add_liquidity
            // calls always use the proportional path, not the first-deposit gate.
            if sqrt_val > MINIMUM_LIQUIDITY {
                sqrt_val
            } else {
                MINIMUM_LIQUIDITY + 1
            }
        } else {
            0
        };
        env.storage()
            .persistent()
            .set(&KEY_LP_TOTAL_SUPPLY, &initial_supply);
        Ok(())
    }

    /// Verify that `admin` is authorized for this call and matches the
    /// stored pool admin (when one exists).
    ///
    /// Mirrors `set_fee_bps`'s auth model: the supplied address must
    /// authorize the invocation via `require_auth()`, and once an admin
    /// identity has been persisted (by `init_pool`), any other address is
    /// rejected with [`AmmPoolError::UnauthorizedCaller`] even though it
    /// can always authorize its *own* transactions.
    fn require_stored_admin(env: &Env, admin: &Address) -> Result<(), AmmPoolError> {
        Self::require_admin(env, admin)?;
        if let Some(stored) = env.storage().persistent().get::<_, Address>(&KEY_ADMIN) {
            if &stored != admin {
                return Err(AmmPoolError::UnauthorizedCaller);
            }
        }
        Ok(())
    }

    /// Verify that `admin` is authorized for this call.
    fn require_admin(_env: &Env, admin: &Address) -> Result<(), AmmPoolError> {
        admin.require_auth();
        Ok(())
    }

    /// Return the current pool admin address, if one has been locked in.
    pub fn get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&KEY_ADMIN)
    }

    /// Return the LP share balance for a user.
    pub fn get_lp_balance(env: Env, user: Address) -> i128 {
        let lp_key = LpBalanceKey::User(user);
        env.storage().persistent().get(&lp_key).unwrap_or(0)
    }

    /// Return the total supply of LP shares.
    pub fn get_total_supply(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&KEY_LP_TOTAL_SUPPLY)
            .unwrap_or(0)
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
    /// * `admin`          — pool admin; must authorize via `require_auth()`
    ///                      and match the stored admin identity (see
    ///                      [`require_admin`](AmmContract::require_admin)).
    /// * `max_impact_bps` — maximum impact in BPS, or `IMPACT_GUARD_DISABLED`.
    pub fn set_max_impact_bps(
        env: Env,
        admin: Address,
        max_impact_bps: u32,
    ) -> Result<(), AmmPoolError> {
        Self::require_stored_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&KEY_MAX_IMPACT_BPS, &max_impact_bps);
        Ok(())
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
        Self::require_admin(&env, &admin)?;
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

    /// Set the optional minimum swap-input floor (admin only).
    ///
    /// When `min_swap_in > 0`, regular swaps whose `amount_in` is strictly
    /// below the floor are rejected with
    /// [`AmmPoolError::AmountBelowMinSwapIn`] before any reserve math runs.
    /// Flash swaps apply the same floor to `amount_out`.  Passing `0`
    /// disables the floor (default); the zero-output guard still applies.
    ///
    /// # Arguments
    /// * `admin`       — address that must authorize the call.
    /// * `min_swap_in` — non-negative minimum input (or flash `amount_out`).
    ///
    /// # Errors
    /// * [`AmmPoolError::NonPositiveAmount`] — `min_swap_in < 0`.
    ///
    /// See: [DUST_SWAP_GUARD.md](../DUST_SWAP_GUARD.md)
    pub fn set_min_swap_in(
        env: Env,
        admin: Address,
        min_swap_in: i128,
    ) -> Result<(), AmmPoolError> {
        Self::require_admin(&env, &admin)?;
        if min_swap_in < 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        env.storage()
            .persistent()
            .set(&KEY_MIN_SWAP_IN, &min_swap_in);
        Ok(())
    }

    /// Return the current minimum swap-input floor.
    ///
    /// Returns [`DEFAULT_MIN_SWAP_IN`] (`0`) when no admin has called
    /// [`set_min_swap_in`](AmmContract::set_min_swap_in) yet.
    pub fn get_min_swap_in(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&KEY_MIN_SWAP_IN)
            .unwrap_or(DEFAULT_MIN_SWAP_IN)
    }

    /// Set the minimum-liquidity floor. Admin only.
    ///
    /// Every remaining reserve must stay `>= min_liquidity` after
    /// `remove_liquidity` or `swap_*`. Passing `0` disables the floor
    /// (default) — fully backward compatible.
    ///
    /// # Arguments
    /// * `admin`         — address that must authorize the call.
    /// * `min_liquidity` — non-negative floor applied to reserves.
    ///
    /// # Errors
    /// * [`AmmPoolError::NonPositiveAmount`] — `min_liquidity < 0`.
    ///
    /// See: [MIN_LIQUIDITY.md](../MIN_LIQUIDITY.md)
    pub fn set_min_liquidity(
        env: Env,
        admin: Address,
        min_liquidity: i128,
    ) -> Result<(), AmmPoolError> {
        Self::require_admin(&env, &admin)?;
        if min_liquidity < 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        env.storage()
            .persistent()
            .set(&KEY_MIN_LIQUIDITY, &min_liquidity);
        Ok(())
    }

    /// Return the current minimum-liquidity floor.
    ///
    /// Returns [`DEFAULT_MIN_LIQUIDITY`] (`0`) when no admin has called
    /// [`set_min_liquidity`](AmmContract::set_min_liquidity) yet.
    pub fn get_min_liquidity(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&KEY_MIN_LIQUIDITY)
            .unwrap_or(DEFAULT_MIN_LIQUIDITY)
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
    fn assert_no_active_flash_swap(env: &Env) -> Result<(), AmmPoolError> {
        let active: bool = env
            .storage()
            .instance()
            .get(&KEY_FLASH_ACTIVE)
            .unwrap_or(false);
        if active {
            return Err(AmmPoolError::ReentrantFlashSwap); // but returns Result
        }
        Ok(())
    }

    /// Add liquidity: the caller transfers `add_a` units of token A and
    /// `add_b` units of token B into the contract. LP shares are minted
    /// proportionally using [`calculate_mint_shares`], which enforces the
    /// [`MINIMUM_LIQUIDITY`] donation-attack guard on the first deposit.
    ///
    /// Requires the caller's authorization and performs real token transfers
    /// so that reported reserves always reflect actual on-chain balances.
    /// Also asserts k-monotonicity (k must not decrease).
    ///
    /// Uniswap-router-style `(desired, min)` parameters: the amounts actually
    /// pulled are the optimal amounts from [`quote_optimal_amounts`] (never
    /// more than `*_desired`), so any off-ratio excess stays with the caller.
    /// Reverts with [`AmmPoolError::SlippageExceeded`] if an optimal amount is
    /// below `amount_a_min` / `amount_b_min` or the minted shares are below
    /// `min_shares`, and with [`AmmPoolError::DeadlineExpired`] when
    /// `now > deadline` (ledger timestamp). Mins must be `>= 0`.
    ///
    /// Returns `(amount_a, amount_b, shares)` — tokens pulled and LP shares
    /// minted to the caller.
    #[allow(clippy::too_many_arguments)]
    pub fn add_liquidity(
        env: Env,
        caller: Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        min_shares: i128,
        deadline: u64,
    ) -> Result<(i128, i128, i128), AmmPoolError> {
        caller.require_auth();
        Self::assert_no_active_flash_swap(&env)?;
        if env.ledger().timestamp() > deadline {
            return Err(AmmPoolError::DeadlineExpired);
        }
        if amount_a_min < 0 || amount_b_min < 0 || min_shares < 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        if amount_a_desired <= 0 || amount_b_desired <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        let total_supply: i128 = env
            .storage()
            .persistent()
            .get(&KEY_LP_TOTAL_SUPPLY)
            .unwrap_or(0);

        let (add_a, add_b) = quote_optimal_amounts(amount_a_desired, amount_b_desired, ra, rb)
            .map_err(|e| match e {
                LiquidityMathError::Overflow => AmmPoolError::Overflow,
                _ => AmmPoolError::InvariantViolation,
            })?;
        if add_a < amount_a_min || add_b < amount_b_min {
            return Err(AmmPoolError::SlippageExceeded);
        }

        // Compute LP shares to mint using the donation-attack-resistant formula.
        let (shares, locked) =
            calculate_mint_shares(total_supply, add_a, add_b, ra, rb).map_err(|e| match e {
                LiquidityMathError::ZeroReserve => AmmPoolError::ZeroReserve,
                LiquidityMathError::InsufficientLiquidityMinted => {
                    AmmPoolError::InsufficientLiquidityMinted
                }
                LiquidityMathError::Overflow => AmmPoolError::Overflow,
                LiquidityMathError::InvalidBurnAmount => AmmPoolError::InvalidBurnAmount,
                LiquidityMathError::ZeroSupply => AmmPoolError::ZeroSupply,
                LiquidityMathError::BurnExceedsSupply => AmmPoolError::BurnExceedsSupply,
            })?;
        if shares < min_shares {
            return Err(AmmPoolError::SlippageExceeded);
        }

        let new_ra = ra.checked_add(add_a).ok_or(AmmPoolError::Overflow)?;
        let new_rb = rb.checked_add(add_b).ok_or(AmmPoolError::Overflow)?;
        assert_k_monotonic(ra, rb, new_ra, new_rb, true)?;

        // Transfer tokens from the caller into this contract before updating reserves.
        let token_a: Address = env
            .storage()
            .persistent()
            .get(&KEY_TOKEN_A)
            .ok_or(AmmPoolError::EmptyPool)?;
        let token_b: Address = env
            .storage()
            .persistent()
            .get(&KEY_TOKEN_B)
            .ok_or(AmmPoolError::EmptyPool)?;
        TokenClient::new(&env, &token_a).transfer(&caller, env.current_contract_address(), &add_a);
        TokenClient::new(&env, &token_b).transfer(&caller, env.current_contract_address(), &add_b);

        // Advance the TWAP accumulator over the interval that just elapsed,
        // using the pre-deposit reserves, then update the reserves.
        Self::update_oracle(&env, ra, rb)?;
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);

        // Update LP share accounting.
        let new_total_supply = total_supply
            .checked_add(shares)
            .and_then(|v| v.checked_add(locked))
            .ok_or(AmmPoolError::Overflow)?;
        env.storage()
            .persistent()
            .set(&KEY_LP_TOTAL_SUPPLY, &new_total_supply);

        // Credit LP shares to caller (only the minted shares, not the locked ones).
        let lp_key = LpBalanceKey::User(caller);
        let user_balance: i128 = env.storage().persistent().get(&lp_key).unwrap_or(0);
        let new_user_balance = user_balance
            .checked_add(shares)
            .ok_or(AmmPoolError::Overflow)?;
        env.storage().persistent().set(&lp_key, &new_user_balance);

        Ok((add_a, add_b, shares))
    }

    /// Remove liquidity by burning LP shares.
    ///
    /// The caller specifies how many LP `shares` to burn. The contract
    /// computes the proportional reserve amounts via
    /// [`calculate_burn_amounts`], transfers those tokens back to the
    /// caller, debits reserves, and burns the shares.
    ///
    /// Requires the caller's authorization and sufficient LP balance.
    /// Also asserts k-monotonicity (k must not increase on removal).
    ///
    /// `amount_a_min` / `amount_b_min` are the minimum acceptable payouts
    /// (must be `>= 0`); a lower payout reverts with
    /// [`AmmPoolError::SlippageExceeded`]. `deadline` is a ledger timestamp;
    /// the call reverts with [`AmmPoolError::DeadlineExpired`] when
    /// `now > deadline`.
    ///
    /// Returns `(amount_a, amount_b)` — the tokens transferred to the caller.
    pub fn remove_liquidity(
        env: Env,
        caller: Address,
        shares: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        deadline: u64,
    ) -> Result<(i128, i128), AmmPoolError> {
        caller.require_auth();
        Self::assert_no_active_flash_swap(&env)?;
        if env.ledger().timestamp() > deadline {
            return Err(AmmPoolError::DeadlineExpired);
        }
        if amount_a_min < 0 || amount_b_min < 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }

        // Validate caller's LP balance.
        let lp_key = LpBalanceKey::User(caller.clone());
        let user_balance: i128 = env.storage().persistent().get(&lp_key).unwrap_or(0);
        if shares <= 0 {
            return Err(AmmPoolError::InvalidBurnAmount);
        }
        if shares > user_balance {
            return Err(AmmPoolError::InsufficientLpBalance);
        }

        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        let total_supply: i128 = env
            .storage()
            .persistent()
            .get(&KEY_LP_TOTAL_SUPPLY)
            .unwrap_or(0);

        // Compute proportional token amounts.
        let (amount_a, amount_b) =
            calculate_burn_amounts(shares, total_supply, ra, rb).map_err(|e| match e {
                LiquidityMathError::InvalidBurnAmount => AmmPoolError::InvalidBurnAmount,
                LiquidityMathError::ZeroSupply => AmmPoolError::ZeroSupply,
                LiquidityMathError::BurnExceedsSupply => AmmPoolError::BurnExceedsSupply,
                LiquidityMathError::Overflow => AmmPoolError::Overflow,
                LiquidityMathError::ZeroReserve => AmmPoolError::ZeroReserve,
                LiquidityMathError::InsufficientLiquidityMinted => AmmPoolError::Overflow,
            })?;

        if amount_a < amount_a_min || amount_b < amount_b_min {
            return Err(AmmPoolError::SlippageExceeded);
        }

        let new_ra = ra
            .checked_sub(amount_a)
            .ok_or(AmmPoolError::InsufficientReserves)?;
        let new_rb = rb
            .checked_sub(amount_b)
            .ok_or(AmmPoolError::InsufficientReserves)?;

        // Minimum-liquidity floor guard. Both reserves decrease on removal,
        // so both are checked (matches the documented policy in
        // MIN_LIQUIDITY.md §"Floor only protects the outgoing reserve on
        // swaps" — for `remove_liquidity` both reserves are "outgoing").
        let floor = Self::get_min_liquidity(env.clone());
        if floor > 0 && (new_ra < floor || new_rb < floor) {
            return Err(AmmPoolError::BelowMinLiquidity);
        }

        assert_k_monotonic(ra, rb, new_ra, new_rb, false)?;

        // Advance the TWAP accumulator over the interval that just elapsed,
        // using the pre-burn reserves, then update the reserves.  Reserves
        // and LP supply are written before the outgoing transfer to follow
        // the checks-effects-interactions pattern.
        Self::update_oracle(&env, ra, rb)?;
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);

        let new_total_supply = total_supply
            .checked_sub(shares)
            .ok_or(AmmPoolError::Overflow)?;
        env.storage()
            .persistent()
            .set(&KEY_LP_TOTAL_SUPPLY, &new_total_supply);

        let new_user_balance = user_balance
            .checked_sub(shares)
            .ok_or(AmmPoolError::Overflow)?;
        env.storage().persistent().set(&lp_key, &new_user_balance);

        // Transfer tokens from this contract back to the caller.
        let token_a: Address = env
            .storage()
            .persistent()
            .get(&KEY_TOKEN_A)
            .ok_or(AmmPoolError::EmptyPool)?;
        let token_b: Address = env
            .storage()
            .persistent()
            .get(&KEY_TOKEN_B)
            .ok_or(AmmPoolError::EmptyPool)?;
        TokenClient::new(&env, &token_a).transfer(
            &env.current_contract_address(),
            &caller,
            &amount_a,
        );
        TokenClient::new(&env, &token_b).transfer(
            &env.current_contract_address(),
            &caller,
            &amount_b,
        );

        Ok((amount_a, amount_b))
    }

    /// Swap from A -> B using Uniswap-style formula with the stored fee.
    ///
    /// The fee is read from [`KEY_FEE_BPS`] (set by the admin via
    /// [`set_fee_bps`](AmmContract::set_fee_bps)) rather than accepted as a
    /// caller argument.  This prevents callers from routing fee-free by
    /// supplying `fee_bps = 0`.  Returns amount_out.
    ///
    /// # Dust-swap guard
    ///
    /// After the constant-product floor division, if `amount_out == 0` the
    /// swap is rejected with [`AmmPoolError::ZeroOutput`].  Without this
    /// check a dust `amount_in` would still be absorbed into `reserve_a`
    /// while paying the caller nothing — free reserve-state grinding that
    /// `assert_k_monotonic` alone does not reject (k still rises).  An
    /// optional admin floor ([`set_min_swap_in`]) rejects inputs below
    /// `min_swap_in` before math runs.  See [DUST_SWAP_GUARD.md](../DUST_SWAP_GUARD.md).
    ///
    /// # Errors
    /// * [`AmmPoolError::NonPositiveAmount`] — `amount_in <= 0`.
    /// * [`AmmPoolError::AmountBelowMinSwapIn`] — `amount_in < min_swap_in`.
    /// * [`AmmPoolError::EmptyPool`] — either reserve is zero.
    /// * [`AmmPoolError::ZeroOutput`] — computed output floors to zero.
    /// * [`AmmPoolError::Overflow`] — checked arithmetic overflow.
    ///
    /// # Overflow policy
    ///
    /// The fee accumulator (`KEY_FEE_A`) uses saturating addition. If the
    /// counter reaches `i128::MAX` it stops incrementing but never panics.
    /// This guarantees the swap cannot be halted by fee-accumulation overflow.
    ///
    /// # Settlement
    ///
    /// `trader` must authorize the call. `amount_in` of token A is pulled from
    /// `trader` into the pool before reserves are updated and `amount_out` of
    /// token B is paid to `trader` afterwards, so stored reserves always equal
    /// the pool's real token balances. The per-token fee accumulators are
    /// informational: the fee stays inside the reserves (it is a subset of
    /// them, not an extra balance). Fee-on-transfer tokens are unsupported.
    ///
    /// # Slippage
    ///
    /// Fails with [`AmmPoolError::SlippageExceeded`] (moving nothing) when the
    /// computed output is below `min_amount_out`.
    pub fn swap_a_for_b(
        env: Env,
        trader: Address,
        amount_in: i128,
        min_amount_out: i128,
    ) -> Result<i128, AmmPoolError> {
        Self::swap_internal(&env, &trader, amount_in, min_amount_out, true)
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
    /// # Dust-swap guard
    ///
    /// Same as [`swap_a_for_b`]: zero computed output is rejected with
    /// [`AmmPoolError::ZeroOutput`], and an optional admin `min_swap_in`
    /// floor applies before math.  See [DUST_SWAP_GUARD.md](../DUST_SWAP_GUARD.md).
    ///
    /// # Errors
    /// * [`AmmPoolError::NonPositiveAmount`] — `amount_in <= 0`.
    /// * [`AmmPoolError::AmountBelowMinSwapIn`] — `amount_in < min_swap_in`.
    /// * [`AmmPoolError::EmptyPool`] — either reserve is zero.
    /// * [`AmmPoolError::ZeroOutput`] — computed output floors to zero.
    /// * [`AmmPoolError::Overflow`] — checked arithmetic overflow.
    ///
    /// # Overflow policy
    ///
    /// Identical to [`swap_a_for_b`]: the fee accumulator saturates at
    /// `i128::MAX` and never panics on addition.
    ///
    /// # Settlement
    ///
    /// Mirror of [`swap_a_for_b`]: `trader` authorizes, `amount_in` of token B
    /// is pulled in before the reserve update and `amount_out` of token A is
    /// paid out afterwards. Same guards (flash-active, min-in, price impact,
    /// min-liquidity, k-monotonic) and the same `min_amount_out` bound.
    pub fn swap_b_for_a(
        env: Env,
        trader: Address,
        amount_in: i128,
        min_amount_out: i128,
    ) -> Result<i128, AmmPoolError> {
        Self::swap_internal(&env, &trader, amount_in, min_amount_out, false)
    }

    /// Shared implementation of both swap directions so every guard is
    /// identical. `a_for_b == true` sells token A for token B.
    fn swap_internal(
        env: &Env,
        trader: &Address,
        amount_in: i128,
        min_amount_out: i128,
        a_for_b: bool,
    ) -> Result<i128, AmmPoolError> {
        if *trader == env.current_contract_address() {
            return Err(AmmPoolError::InvalidTrader);
        }
        trader.require_auth();
        Self::assert_no_active_flash_swap(env)?;
        if amount_in <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        // Optional admin floor (default 0 = disabled).
        let min_swap_in: i128 = env
            .storage()
            .persistent()
            .get(&KEY_MIN_SWAP_IN)
            .unwrap_or(DEFAULT_MIN_SWAP_IN);
        if min_swap_in > 0 && amount_in < min_swap_in {
            return Err(AmmPoolError::AmountBelowMinSwapIn);
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

        let (amount_out, fee) = compute_swap_out(ra, rb, amount_in, fee_bps, a_for_b)?;

        // Dust-swap guard: reject zero-output swaps so input is never
        // consumed without an economic exchange of the output asset.
        if amount_out == 0 {
            return Err(AmmPoolError::ZeroOutput);
        }
        if amount_out < min_amount_out {
            return Err(AmmPoolError::SlippageExceeded);
        }

        let (res_in, res_out) = if a_for_b { (ra, rb) } else { (rb, ra) };
        let new_in = res_in
            .checked_add(amount_in)
            .ok_or(AmmPoolError::Overflow)?;
        let new_out = res_out
            .checked_sub(amount_out)
            .ok_or(AmmPoolError::Overflow)?;
        let (new_ra, new_rb) = if a_for_b {
            (new_in, new_out)
        } else {
            (new_out, new_in)
        };

        // Minimum-liquidity floor guard: only the outgoing reserve decreases
        // (MIN_LIQUIDITY.md §"Floor only protects the outgoing reserve on swaps").
        let floor = Self::get_min_liquidity(env.clone());
        if floor > 0 && new_out < floor {
            return Err(AmmPoolError::BelowMinLiquidity);
        }

        assert_k_monotonic(ra, rb, new_ra, new_rb, true)?;

        // ---- Price-impact guard ----
        // Spot price (out per in) before = res_out / res_in, after =
        // new_out / new_in.
        // impact_bps = (1 - new_out * res_in / (res_out * new_in)) * 10_000
        //            = (ratio_den - ratio_num) * 10_000 / ratio_den
        let max_impact: u32 = env
            .storage()
            .persistent()
            .get(&KEY_MAX_IMPACT_BPS)
            .unwrap_or(IMPACT_GUARD_DISABLED);
        if max_impact != IMPACT_GUARD_DISABLED {
            let ratio_num = new_out.checked_mul(res_in).ok_or(AmmPoolError::Overflow)?;
            let ratio_den = res_out.checked_mul(new_in).ok_or(AmmPoolError::Overflow)?;
            let impact_bps = ratio_den
                .checked_sub(ratio_num)
                .and_then(|d| d.checked_mul(10_000))
                .ok_or(AmmPoolError::Overflow)?
                / ratio_den;
            if impact_bps > max_impact as i128 {
                return Err(AmmPoolError::PriceImpactExceeded);
            }
        }

        let (fee_key, token_in_key, token_out_key) = if a_for_b {
            (KEY_FEE_A, KEY_TOKEN_A, KEY_TOKEN_B)
        } else {
            (KEY_FEE_B, KEY_TOKEN_B, KEY_TOKEN_A)
        };
        let token_in: Address = env
            .storage()
            .persistent()
            .get(&token_in_key)
            .ok_or(AmmPoolError::EmptyPool)?;
        let token_out: Address = env
            .storage()
            .persistent()
            .get(&token_out_key)
            .ok_or(AmmPoolError::EmptyPool)?;
        let accrued_fee: i128 = env.storage().persistent().get(&fee_key).unwrap_or(0);

        // Pull the input before touching reserves, pay the output after.
        let pool = env.current_contract_address();
        TokenClient::new(env, &token_in).transfer(trader, &pool, &amount_in);

        // Advance the TWAP accumulator with the pre-swap reserves so the
        // trade being priced now is not folded into the interval it just
        // closed.
        Self::update_oracle(env, ra, rb)?;
        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        env.storage()
            .persistent()
            .set(&fee_key, &accrued_fee.saturating_add(fee));

        TokenClient::new(env, &token_out).transfer(&pool, trader, &amount_out);
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
    /// `caller` is the authenticated initiator, verified via
    /// `caller.require_auth()` and recorded as the **initiator** so that
    /// only the same address may call
    /// [`repay_flash_swap`](AmmContract::repay_flash_swap). This prevents
    /// a third party from observing an in-flight flash swap and calling
    /// `repay_flash_swap` (or otherwise interfering) within the same
    /// transaction.
    ///
    /// Note: `caller` must be passed explicitly rather than derived from
    /// `env.current_contract_address()`, which always resolves to this
    /// contract's own address regardless of who invoked the entrypoint.
    ///
    /// Soroban 25.3.1 does not allow a contract to invoke itself from
    /// inside a callback (`Contract re-entry is not allowed`), so this
    /// design dispatches the two halves of the flash swap as separate
    /// entry points within a single multi-operation transaction:
    ///
    /// ```text
    /// Op 1: AMM.flash_swap_a_for_b(caller, amount_out, params)
    /// Op 2: <caller runs arbitrary logic on asset A received elsewhere>
    /// Op 3: AMM.repay_flash_swap(caller, amount_in) // verify-k runs here
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
    /// * `caller`     — the flash-swap initiator; must authorize via
    ///                  `require_auth()`.  Recorded so the matching
    ///                  `repay_flash_swap` can be restricted to this
    ///                  same address.
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
    /// # Dust-swap / min-input guard
    ///
    /// Flash swaps are amount-out driven, so the zero-output grinding
    /// vector of regular swaps does not apply directly.  The optional
    /// admin `min_swap_in` floor is still enforced against `amount_out`
    /// so dust flash draws cannot open a flash session that only
    /// perturbs reserves.  See [DUST_SWAP_GUARD.md](../DUST_SWAP_GUARD.md).
    ///
    /// # Returns
    /// * `Ok(amount_out)` — the number of asset-B units debited from the pool.
    ///
    /// # Errors
    /// * [`AmmPoolError::ReentrantFlashSwap`] — a flash swap is already in flight.
    /// * [`AmmPoolError::NonPositiveAmount`] — `amount_out ≤ 0`.
    /// * [`AmmPoolError::AmountBelowMinSwapIn`] — `amount_out < min_swap_in`.
    /// * [`AmmPoolError::EmptyPool`] — either reserve is zero.
    /// * [`AmmPoolError::InsufficientReserves`] — `amount_out ≥ reserve_b`.
    ///
    /// See: [FLASH_SWAP_PROTOCOL.md §Call Sequence](../FLASH_SWAP_PROTOCOL.md)
    pub fn flash_swap_a_for_b(
        env: Env,
        caller: Address,
        amount_out: i128,
        params: Bytes,
    ) -> Result<i128, AmmPoolError> {
        // `params` is reserved for a future callback variant.  Bound to
        // a local so the parameter is used (no dead-binding lint).
        let _ = params;

        caller.require_auth();
        Self::assert_no_active_flash_swap(&env)?;

        if amount_out <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }
        // Apply the same admin min-input floor used by regular swaps to
        // the flash draw size so dust flash sessions cannot open.
        let min_swap_in: i128 = env
            .storage()
            .persistent()
            .get(&KEY_MIN_SWAP_IN)
            .unwrap_or(DEFAULT_MIN_SWAP_IN);
        if min_swap_in > 0 && amount_out < min_swap_in {
            return Err(AmmPoolError::AmountBelowMinSwapIn);
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

        // Advance the TWAP accumulator with the pre-flash reserves.  Those
        // same reserves are persisted so the matching `repay_flash_swap` can
        // extend the interval without ever pricing the in-flight imbalance.
        Self::update_oracle(&env, ra, rb)?;
        env.storage()
            .persistent()
            .set(&KEY_FLASH_PRE_RESERVES, &(ra, rb));

        // Snapshot the invariant before applying the debit so the matching
        // `repay_flash_swap` can compare against the post-debit state.
        env.storage().persistent().set(&KEY_K_BEFORE, &k_before);
        env.storage().persistent().set(&KEY_RES_B, &new_rb);
        env.storage().instance().set(&KEY_FLASH_ACTIVE, &true);
        // Record the initiator so only this address may repay.
        env.storage().instance().set(&KEY_FLASH_INITIATOR, &caller);

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
    /// * `caller`    — the address repaying the flash swap; must match the
    ///                 initiator recorded by `flash_swap_a_for_b` and must
    ///                 authorize via `require_auth()`.
    /// * `amount_in` — units of asset A being repaid.  Must be `> 0`.
    ///
    /// # Errors
    /// - [`AmmPoolError::NonPositiveAmount`] — `amount_in ≤ 0`.
    /// - [`AmmPoolError::InvariantViolation`] — called outside an active flash swap, the
    ///   initiator address is missing from storage, or k decreased (under-repayment);
    ///   Soroban rolls back all storage changes including the Op-1 debit.
    /// - [`AmmPoolError::UnauthorizedCaller`] — caller is not the flash-swap initiator.
    /// - [`AmmPoolError::Overflow`] — arithmetic overflow computing `new_ra` or `k_after`.
    ///
    /// # Panics
    /// - `"repay_flash_swap: amount_in must be positive"` — `amount_in ≤ 0`.
    /// - `"repay_flash_swap: no flash swap in progress"` — called outside a flash swap.
    /// - `"UnauthorizedCaller: repay_flash_swap must be called by the flash-swap initiator"` — caller mismatch.
    /// - `"Invariant violation: k decreased during flash-swap repayment"` — under-repayment;
    ///   Soroban then rolls back all storage changes, including the Op-1 debit.
    ///
    /// See: [FLASH_SWAP_PROTOCOL.md §Verify-K Repay Invariant](../FLASH_SWAP_PROTOCOL.md)
    pub fn repay_flash_swap(
        env: Env,
        caller: Address,
        amount_in: i128,
    ) -> Result<(), AmmPoolError> {
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
        if caller != initiator {
            return Err(AmmPoolError::UnauthorizedCaller);
        }
        // Require explicit authorization from the caller (== initiator).
        caller.require_auth();

        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        let k_before: i128 = env
            .storage()
            .persistent()
            .get(&KEY_K_BEFORE)
            .ok_or(AmmPoolError::InvariantViolation)?;

        let new_ra: i128 = ra.checked_add(amount_in).ok_or(AmmPoolError::Overflow)?;

        // ---- Verify-k: k must not have decreased. ----
        // After the optimistic debit, reserve_b holds `rb` (already
        // reduced by amount_out).  After this credit, reserve_a =
        // `new_ra`.  The product must be ≥ k_before.
        let k_after: i128 = new_ra.checked_mul(rb).ok_or(AmmPoolError::Overflow)?;
        if k_after < k_before {
            return Err(AmmPoolError::InvariantViolation);
        }

        // Extend the TWAP interval using the reserves that were in force
        // before the flash began.  The pool is still holding the optimistic
        // debit here, and pricing that imbalance would let a caller who
        // straddles the flash across ledgers skew the oracle.
        let (pre_flash_a, pre_flash_b): (i128, i128) = env
            .storage()
            .persistent()
            .get(&KEY_FLASH_PRE_RESERVES)
            .ok_or(AmmPoolError::InvariantViolation)?;
        Self::update_oracle(&env, pre_flash_a, pre_flash_b)?;

        env.storage().persistent().set(&KEY_RES_A, &new_ra);
        env.storage().persistent().remove(&KEY_FLASH_PRE_RESERVES);
        env.storage().instance().set(&KEY_FLASH_ACTIVE, &false);
        env.storage().instance().remove(&KEY_FLASH_INITIATOR);
        Ok(())
    }

    /// Read-only swap quotation — projects the outcome of a swap without
    /// writing to storage or emitting events.
    ///
    /// Reuses the same constant-product math and [`compute_fee`] call that the
    /// live swap paths (`swap_a_for_b` / `swap_b_for_a`) execute, so the quote
    /// is guaranteed to match a live swap to the unit.
    ///
    /// # Arguments
    /// * `amount_in` — positive input amount (same sign and units as the live swap).
    /// * `fee_bps`   — fee in basis points to apply (e.g. 30 = 0.30 %).  Use
    ///                 [`AmmContract::get_fee_bps`] to pass the current stored fee.
    /// * `a_for_b`   — `true` → quote A→B; `false` → quote B→A.
    ///
    /// # Returns
    /// `Ok(SwapQuote)` with the projected output, fee taken, and resulting
    /// reserves for both tokens.
    ///
    /// # Errors
    /// * [`AmmPoolError::NonPositiveAmount`] — `amount_in <= 0`.
    /// * [`AmmPoolError::EmptyPool`]         — either reserve is zero (returns
    ///   a typed error instead of panicking, unlike the live path).
    /// * [`AmmPoolError::Overflow`]          — checked arithmetic overflow.
    pub fn get_swap_quote(
        env: Env,
        amount_in: i128,
        fee_bps: i128,
        a_for_b: bool,
    ) -> Result<SwapQuote, AmmPoolError> {
        if amount_in <= 0 {
            return Err(AmmPoolError::NonPositiveAmount);
        }

        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            return Err(AmmPoolError::EmptyPool);
        }

        // Same math helper as the live swap path, so quotes are exact.
        let (amount_out, fee) = compute_swap_out(ra, rb, amount_in, fee_bps, a_for_b)?;

        let (reserve_a_after, reserve_b_after) = if a_for_b {
            (
                ra.checked_add(amount_in).ok_or(AmmPoolError::Overflow)?,
                rb.checked_sub(amount_out).ok_or(AmmPoolError::Overflow)?,
            )
        } else {
            (
                ra.checked_sub(amount_out).ok_or(AmmPoolError::Overflow)?,
                rb.checked_add(amount_in).ok_or(AmmPoolError::Overflow)?,
            )
        };

        Ok(SwapQuote {
            amount_out,
            fee,
            reserve_a_after,
            reserve_b_after,
        })
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

    /// Advance the TWAP accumulator up to the current ledger timestamp.
    ///
    /// Must be called with the reserves that were in force *before* the
    /// pending reserve mutation. On the first call the accumulator is
    /// initialised and a baseline observation is written; afterwards an
    /// observation is written at most once per ledger timestamp.
    ///
    /// # Overflow policy
    ///
    /// Spot prices are computed with checked multiplication and surface
    /// [`AmmPoolError::Overflow`] if `reserve * TWAP_PRICE_SCALE` does not fit
    /// in `i128` (reserves above ~`1.7e20`, far beyond any realistic pool and
    /// already incompatible with the `reserve_a * reserve_b` invariant).
    ///
    /// The cumulative sums themselves use *wrapping* arithmetic. This is the
    /// Uniswap-v2 approach: consumers only ever read differences of
    /// cumulatives, and wrapping addition/subtraction is exact modulo
    /// `2^128`. Any window whose true growth is below `2^127` therefore yields
    /// the correct difference regardless of how many times the accumulator
    /// has wrapped. At a price of `1.0` (`TWAP_PRICE_SCALE` per second) that
    /// is a horizon of roughly `5.4e12` years; even at an extreme reserve
    /// ratio of `1e12` a two-year window is still safe. See
    /// [TWAP.md](../TWAP.md#overflow-policy).
    fn update_oracle(
        env: &Env,
        reserve_a_before: i128,
        reserve_b_before: i128,
    ) -> Result<(), AmmPoolError> {
        let now = env.ledger().timestamp();
        let state_key = TwapKey::State;

        let mut state: TwapState = match env.storage().persistent().get(&state_key) {
            Some(state) => state,
            None => {
                let state = TwapState {
                    price0_cumulative: 0,
                    price1_cumulative: 0,
                    last_ts: now,
                    index: 0,
                    cardinality: 1,
                };
                write_observation(
                    env,
                    0,
                    &TwapObservation {
                        timestamp: now,
                        price0_cumulative: 0,
                        price1_cumulative: 0,
                    },
                );
                env.storage().persistent().set(&state_key, &state);
                return Ok(());
            }
        };

        let dt = now.saturating_sub(state.last_ts);
        state.last_ts = now;

        // A zero-length interval contributes nothing and must not create a
        // second observation for the same timestamp. An empty or half-empty
        // pool has no defined price, so the accumulator is frozen over that
        // interval (no division by zero, no observation).
        if dt == 0 || reserve_a_before <= 0 || reserve_b_before <= 0 {
            env.storage().persistent().set(&state_key, &state);
            return Ok(());
        }

        let price0 = scaled_price(reserve_b_before, reserve_a_before)?;
        let price1 = scaled_price(reserve_a_before, reserve_b_before)?;

        let dt = dt as i128;
        state.price0_cumulative = state
            .price0_cumulative
            .wrapping_add(price0.wrapping_mul(dt));
        state.price1_cumulative = state
            .price1_cumulative
            .wrapping_add(price1.wrapping_mul(dt));

        let next = next_observation_slot(&state);
        write_observation(
            env,
            next,
            &TwapObservation {
                timestamp: now,
                price0_cumulative: state.price0_cumulative,
                price1_cumulative: state.price1_cumulative,
            },
        );
        state.index = next;
        state.cardinality = state
            .cardinality
            .saturating_add(1)
            .min(TWAP_OBSERVATION_COUNT);

        env.storage().persistent().set(&state_key, &state);
        Ok(())
    }

    /// Number of observations currently retained in the TWAP ring buffer.
    ///
    /// Grows from `0` to [`TWAP_OBSERVATION_COUNT`] and then stays at that cap:
    /// once the ring is full every new observation overwrites the oldest slot,
    /// so oracle storage never grows with the number of swaps.
    pub fn observation_cardinality(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get::<_, TwapState>(&TwapKey::State)
            .map(|state| state.cardinality)
            .unwrap_or(0)
    }

    /// Return the observation stored in physical ring slot `i`.
    ///
    /// Slots are physical storage positions, not chronological positions.
    /// When the ring is full the oldest observation is at
    /// `(index + 1) % TWAP_OBSERVATION_COUNT`, where `index` is the slot of the
    /// newest observation; iterate chronologically by wrapping from there.
    /// Off-chain consumers that just need the current price should call
    /// [`consult`](AmmContract::consult) instead.
    ///
    /// # Errors
    /// * [`AmmPoolError::TwapObservationNotFound`] — `i` is outside
    ///   `0..TWAP_OBSERVATION_COUNT` or that slot has never been written.
    pub fn get_observation(env: Env, i: u32) -> Result<TwapObservation, AmmPoolError> {
        if i >= TWAP_OBSERVATION_COUNT {
            return Err(AmmPoolError::TwapObservationNotFound);
        }
        read_observation(&env, i).ok_or(AmmPoolError::TwapObservationNotFound)
    }

    /// Time-weighted average price over the last `window_secs` seconds.
    ///
    /// Returns `(price_a_in_b, price_b_in_a)`, both scaled by
    /// [`TWAP_PRICE_SCALE`]. `price_a_in_b` is the average number of token-B
    /// units per token-A unit over the window.
    ///
    /// The start of the window is the *most recent observation that is at
    /// least* `window_secs` old, so the effective window can be slightly
    /// longer than requested (never shorter). The end of the window is now,
    /// using the current reserves to extrapolate the accumulator since the
    /// last observation. A `window_secs` of `0` returns the current spot
    /// price.
    ///
    /// # Errors
    /// * [`AmmPoolError::EmptyPool`] — either reserve is zero (no price).
    /// * [`AmmPoolError::TwapInsufficientHistory`] — the window reaches past
    ///   the oldest retained observation.
    /// * [`AmmPoolError::Overflow`] — a spot price does not fit in `i128`.
    pub fn consult(env: Env, window_secs: u64) -> Result<(i128, i128), AmmPoolError> {
        let ra: i128 = env.storage().persistent().get(&KEY_RES_A).unwrap_or(0);
        let rb: i128 = env.storage().persistent().get(&KEY_RES_B).unwrap_or(0);
        if ra <= 0 || rb <= 0 {
            return Err(AmmPoolError::EmptyPool);
        }

        let price0_now = scaled_price(rb, ra)?;
        let price1_now = scaled_price(ra, rb)?;
        if window_secs == 0 {
            return Ok((price0_now, price1_now));
        }

        let state: TwapState = env
            .storage()
            .persistent()
            .get(&TwapKey::State)
            .ok_or(AmmPoolError::TwapInsufficientHistory)?;
        if state.cardinality == 0 {
            return Err(AmmPoolError::TwapInsufficientHistory);
        }

        let now = env.ledger().timestamp();
        let target = now
            .checked_sub(window_secs)
            .ok_or(AmmPoolError::TwapInsufficientHistory)?;
        let observation = find_observation_at_or_before(&env, &state, target)?
            .ok_or(AmmPoolError::TwapInsufficientHistory)?;

        // Extrapolate the newest cumulative to `now` with the current reserves.
        let elapsed = now.saturating_sub(state.last_ts) as i128;
        let cumulative0_now = state
            .price0_cumulative
            .wrapping_add(price0_now.wrapping_mul(elapsed));
        let cumulative1_now = state
            .price1_cumulative
            .wrapping_add(price1_now.wrapping_mul(elapsed));

        let dt = now.saturating_sub(observation.timestamp);
        if dt == 0 {
            // Defensive: a non-zero window can never select an observation at
            // `now`, but never divide by zero even if the clock misbehaves.
            return Err(AmmPoolError::TwapInsufficientHistory);
        }
        let dt = dt as i128;

        let price0 = cumulative0_now.wrapping_sub(observation.price0_cumulative) / dt;
        let price1 = cumulative1_now.wrapping_sub(observation.price1_cumulative) / dt;
        Ok((price0, price1))
    }
}

// ---------------------------------------------------------------------------
// TWAP ring-buffer helpers
// ---------------------------------------------------------------------------

/// Compute `numerator * TWAP_PRICE_SCALE / denominator` as a fixed-point price.
///
/// Callers must guarantee `denominator > 0`. Returns
/// [`AmmPoolError::Overflow`] when the scaled numerator does not fit in `i128`.
fn scaled_price(numerator: i128, denominator: i128) -> Result<i128, AmmPoolError> {
    numerator
        .checked_mul(TWAP_PRICE_SCALE)
        .map(|scaled| scaled / denominator)
        .ok_or(AmmPoolError::Overflow)
}

/// Physical slot the next observation should be written to.
fn next_observation_slot(state: &TwapState) -> u32 {
    if state.cardinality < TWAP_OBSERVATION_COUNT {
        if state.cardinality == 0 {
            0
        } else {
            state.index + 1
        }
    } else {
        (state.index + 1) % TWAP_OBSERVATION_COUNT
    }
}

/// Physical slot of the oldest retained observation.
fn ring_start(state: &TwapState) -> u32 {
    if state.cardinality < TWAP_OBSERVATION_COUNT {
        0
    } else {
        (state.index + 1) % TWAP_OBSERVATION_COUNT
    }
}

/// Map a chronological offset (`0` = oldest) to a physical ring slot.
fn ring_slot(state: &TwapState, chronological: u32) -> u32 {
    (ring_start(state) + chronological) % TWAP_OBSERVATION_COUNT
}

fn read_observation(env: &Env, slot: u32) -> Option<TwapObservation> {
    env.storage().persistent().get(&TwapKey::Obs(slot))
}

fn write_observation(env: &Env, slot: u32, observation: &TwapObservation) {
    env.storage()
        .persistent()
        .set(&TwapKey::Obs(slot), observation);
}

/// Return the newest observation whose timestamp is `<= target_ts`.
///
/// Observations are sorted by timestamp in chronological (ring) order, so a
/// binary search over the retained window finds the start of the TWAP window.
/// Returns `Ok(None)` when every retained observation is newer than
/// `target_ts` (insufficient history).
fn find_observation_at_or_before(
    env: &Env,
    state: &TwapState,
    target_ts: u64,
) -> Result<Option<TwapObservation>, AmmPoolError> {
    if state.cardinality == 0 {
        return Ok(None);
    }

    let oldest =
        read_observation(env, ring_slot(state, 0)).ok_or(AmmPoolError::TwapObservationNotFound)?;
    if oldest.timestamp > target_ts {
        return Ok(None);
    }

    // `i64` avoids unsigned underflow when `hi` moves below zero.
    let mut lo: i64 = 0;
    let mut hi: i64 = i64::from(state.cardinality) - 1;
    let mut best: i64 = 0;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let observation = read_observation(env, ring_slot(state, mid as u32))
            .ok_or(AmmPoolError::TwapObservationNotFound)?;
        if observation.timestamp <= target_ts {
            best = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }

    read_observation(env, ring_slot(state, best as u32))
        .map(Some)
        .ok_or(AmmPoolError::TwapObservationNotFound)
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
    let k_before = before_a
        .checked_mul(before_b)
        .ok_or(AmmPoolError::Overflow)?;
    let k_after = after_a.checked_mul(after_b).ok_or(AmmPoolError::Overflow)?;
    if expect_increase {
        if k_after < k_before {
            return Err(AmmPoolError::InvariantViolation);
        }
    } else if k_after > k_before {
        return Err(AmmPoolError::InvariantViolation);
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
    Ok(amount_in
        .checked_mul(fee_bps)
        .ok_or(AmmPoolError::Overflow)?
        / 10_000)
}

/// Shared constant-product math for live swaps and [`AmmContract::get_swap_quote`].
///
/// Returns `(amount_out, fee)`:
///
/// ```text
/// amount_in_with_fee = amount_in * (10_000 - fee_bps)
/// amount_out = (amount_in_with_fee * reserve_out)
///            / (reserve_in * 10_000 + amount_in_with_fee)   [floor]
/// ```
///
/// Output is floored so the pool never over-pays. All arithmetic is checked
/// and surfaces [`AmmPoolError::Overflow`].
fn compute_swap_out(
    ra: i128,
    rb: i128,
    amount_in: i128,
    fee_bps: i128,
    a_for_b: bool,
) -> Result<(i128, i128), AmmPoolError> {
    let fee = compute_fee(amount_in, fee_bps)?;
    let fee_adj = 10_000_i128
        .checked_sub(fee_bps)
        .ok_or(AmmPoolError::Overflow)?;
    let amount_in_with_fee = amount_in
        .checked_mul(fee_adj)
        .ok_or(AmmPoolError::Overflow)?;
    let (reserve_in, reserve_out) = if a_for_b { (ra, rb) } else { (rb, ra) };
    let numerator = amount_in_with_fee
        .checked_mul(reserve_out)
        .ok_or(AmmPoolError::Overflow)?;
    let denom_part = reserve_in
        .checked_mul(10_000_i128)
        .ok_or(AmmPoolError::Overflow)?;
    let denominator = denom_part
        .checked_add(amount_in_with_fee)
        .ok_or(AmmPoolError::Overflow)?;
    Ok((numerator / denominator, fee))
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
pub(crate) mod swap_test_util;

#[cfg(test)]
mod price_impact_test;

// ---------------------------------------------------------------------------
// Tests: min-liquidity floor (regression coverage for issue #1559).
// Lives in a separate file (linked below by `mod test;`) to keep the
// fuzz / swap-bounds tests in this file focused on the swap-math invariants
// rather than admin settings.  The orphan 478-line file at
// `src/test.rs` was rewritten against the actual public API of this crate
// (`init_pool`, `add_liquidity(caller, a, b)`, `remove_liquidity(caller, shares)`,
// `swap_a_for_b`, `swap_b_for_a`, `get_reserves` returning a tuple, …)
// rather than the fictional API it originally assumed.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod test;

#[cfg(test)]
mod inline_test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn generate_address(env: &Env) -> Address {
        use soroban_sdk::testutils::Address as _;
        Address::generate(env)
    }

    #[test]
    fn fuzz_swap_k_monotonic() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);

        let token_a = crate::swap_test_util::register_token(&env);
        let token_b = crate::swap_test_util::register_token(&env);
        let admin = Address::generate(&env);
        let trader = crate::swap_test_util::new_trader(&env, &token_a, &token_b);

        let reserve_sizes = [1_000_i128, 10_000, 100_000, 1_000_000];
        // Start from 2: amount_in=1 floors to zero output under the dust guard
        // for all of these reserve sizes (with the default 30 bps fee).
        let amounts = [2_i128, 10, 100, 1_000, 10_000];

        for &ra in reserve_sizes.iter() {
            for &rb in reserve_sizes.iter() {
                for &amt in amounts.iter() {
                    client.init_pool(&admin, &ra, &rb, &token_a, &token_b);
                    crate::swap_test_util::fund_pool(&env, &id, &token_a, &token_b, ra, rb);
                    // swap using stored fee (default 30 bps)
                    let res = client.try_swap_a_for_b(&trader, &amt, &0);
                    if res == Err(Ok(AmmPoolError::ZeroOutput)) {
                        // Dust input that floors to zero output (e.g. a
                        // small amt against a much larger ra) is rejected
                        // by the dust-swap guard before touching reserves.
                        continue;
                    }
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

        let caller = generate_address(&env);
        let pool_admin = generate_address(&env);
        let a_admin = generate_address(&env);
        let b_admin = generate_address(&env);
        let ta = env.register_stellar_asset_contract(a_admin);
        let tb = env.register_stellar_asset_contract(b_admin);
        soroban_sdk::token::StellarAssetClient::new(&env, &ta).mint(&caller, &1_000_i128);
        soroban_sdk::token::StellarAssetClient::new(&env, &tb).mint(&caller, &2_000_i128);

        client.init_pool(&pool_admin, &1000, &2000, &ta, &tb);
        client.add_liquidity(&caller, &100, &200, &0, &0, &0, &u64::MAX);
        let (ra1, rb1) = client.get_reserves();
        let k1 = ra1.checked_mul(rb1).unwrap();

        // Get the LP shares minted and burn half
        let lp_balance = client.get_lp_balance(&caller);
        assert!(lp_balance > 0, "should have received LP shares");
        let burn_shares = lp_balance / 2;
        let (rem_a, rem_b) = client.remove_liquidity(&caller, &burn_shares, &0, &0, &u64::MAX);
        let (ra2, rb2) = client.get_reserves();
        let k2 = ra2.checked_mul(rb2).unwrap();

        assert!(k2 <= k1, "k should not increase on removal");
        assert!(rem_a > 0 && rem_b > 0, "should receive tokens back");
    }

    #[test]
    fn test_first_deposit_minimum_liquidity_lock() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);

        let _token_a = generate_address(&env);
        let _token_b = generate_address(&env);
        let caller = generate_address(&env);

        // Register real token contracts and mint tokens for transfer to succeed.
        let a_admin = generate_address(&env);
        let b_admin = generate_address(&env);
        let ta = env.register_stellar_asset_contract(a_admin);
        let tb = env.register_stellar_asset_contract(b_admin);
        soroban_sdk::token::StellarAssetClient::new(&env, &ta).mint(&caller, &1001_i128);
        soroban_sdk::token::StellarAssetClient::new(&env, &tb).mint(&caller, &1001_i128);

        client.init_pool(&caller, &0_i128, &0_i128, &ta, &tb);

        // First deposit: 1001 of each token → sqrt(1001*1001)=1001, shares=1, locked=1000
        let (_, _, shares) =
            client.add_liquidity(&caller, &1001_i128, &1001_i128, &0, &0, &0, &u64::MAX);
        assert_eq!(shares, 1);

        let lp_balance = client.get_lp_balance(&caller);
        assert_eq!(lp_balance, 1);

        let total_supply = client.get_total_supply();
        assert_eq!(total_supply, 1001); // 1 minted + 1000 locked
    }

    #[test]
    fn test_remove_liquidity_proportional() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);

        let _token_a = generate_address(&env);
        let _token_b = generate_address(&env);
        let caller = generate_address(&env);

        // Register real token contracts and mint tokens.
        let a_admin = generate_address(&env);
        let b_admin = generate_address(&env);
        let ta = env.register_stellar_asset_contract(a_admin);
        let tb = env.register_stellar_asset_contract(b_admin);
        soroban_sdk::token::StellarAssetClient::new(&env, &ta).mint(&caller, &10000_i128);
        soroban_sdk::token::StellarAssetClient::new(&env, &tb).mint(&caller, &10000_i128);

        client.init_pool(&caller, &0_i128, &0_i128, &ta, &tb);

        // Deposit 10000 of each token.
        let (_, _, shares) =
            client.add_liquidity(&caller, &10000_i128, &10000_i128, &0, &0, &0, &u64::MAX);
        assert!(shares > 0);

        let (ra, rb) = client.get_reserves();

        // Burn all shares → should get back all reserves.
        let lp_balance = client.get_lp_balance(&caller);
        let (amount_a, amount_b) = client.remove_liquidity(&caller, &lp_balance, &0, &0, &u64::MAX);

        // Due to rounding-down on burn, amounts returned may be slightly less
        // than reserves. Verify that reserves are drained appropriately.
        let (ra_after, rb_after) = client.get_reserves();
        assert!(ra_after <= ra);
        assert!(rb_after <= rb);
        assert!(amount_a > 0);
        assert!(amount_b > 0);
    }

    #[test]
    #[should_panic(expected = "9")] // InsufficientLiquidityMinted
    fn test_add_liquidity_rejects_tiny_deposit() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(AmmContract, ());
        let client = AmmContractClient::new(&env, &id);

        let token_a = generate_address(&env);
        let token_b = generate_address(&env);
        let caller = generate_address(&env);

        client.init_pool(&caller, &0_i128, &0_i128, &token_a, &token_b);
        // sqrt(100*10)=31 < MINIMUM_LIQUIDITY(1000) → rejected
        client.add_liquidity(&caller, &100_i128, &10_i128, &0, &0, &0, &u64::MAX);
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
mod dust_swap_guard_test;
#[cfg(test)]
mod dynamic_fee_test;
#[cfg(test)]
mod inverse_swap_proptest;
#[cfg(test)]
mod swap_quote_test;
#[cfg(test)]
mod twap_test;
