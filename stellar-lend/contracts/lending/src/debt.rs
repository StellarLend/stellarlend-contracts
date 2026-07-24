use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol, Vec};

use crate::math::split_interest_by_reserve_factor;
use crate::rounding_strategy::{calculate_interest_with_rounding, RoundingError, RoundingMode};
use crate::{rate_model, write_utilization_sample, DataKey};
use stellar_lend_common::BPS_DENOM;


pub const DEFAULT_APR_BPS: i128 = 500;

/// Reserve factor used when no explicit value is configured: 0% (protocol takes nothing).
pub const DEFAULT_RESERVE_FACTOR_BPS: u32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterestSplit {
    /// Gross interest accrued during the period.
    pub total_interest: i128,
    /// Share of interest owed to depositors (supply-side yield).
    pub depositor_yield: i128,
    /// Share of interest retained by the protocol reserve.
    pub reserve_cut: i128,
}

// ─── Core position type ───────────────────────────────────────────────────────

/// Fixed-point scale for the global borrow index (10^7 = 7 decimal places).
///
/// The index starts at `INDEX_SCALE` (representing 1.0) and grows
/// monotonically as interest accrues.  A position's current debt is:
///
/// ```text
/// current_debt = principal × current_index / borrow_index_snapshot
/// ```
pub const INDEX_SCALE: i128 = 10_000_000; // 10^7

/// Seconds in a 365-day year, shared with rounding_strategy.
const SECONDS_PER_YEAR: u64 = 365 * 24 * 60 * 60; // 31_536_000

// ---------------------------------------------------------------------------
// DebtPosition
// ---------------------------------------------------------------------------

/// Per-borrower debt record.
///
/// Layout change (global-borrow-index feature):
/// - `last_update` is **removed**; the global `LastIndexUpdate` timestamp
///   drives time tracking.
/// - `borrow_index_snapshot` is added; it holds the value of
///   `DataKey::BorrowIndex` at the time the position was last touched.
///
/// Migration: pre-existing positions without a snapshot are treated as
/// having `borrow_index_snapshot == 0`, which `migrate_positions` fixes
/// by writing the current index into every such record before normal
/// operations resume.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtPosition {
    /// Recorded principal at last touch (does not include un-accrued interest).
    pub principal: i128,
    /// Snapshot of the global borrow index at the time this position was last
    /// modified.  Zero signals "pre-migration; treat as current index".
    pub borrow_index_snapshot: i128,
    /// Wall-clock timestamp of the last explicit settlement, kept for
    /// backward-compatible reads.  Updated on every position write.
    pub last_update: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateSnapshot {
    pub total_debt: i128,
    pub total_supply: i128,
    pub params: Option<rate_model::RateParams>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowRateCache {
    pub ledger_sequence: u32,
    pub rate_bps: i128,
}

/// Aggregate borrow-rate calculation output for one storage snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BorrowRateComputation {
    /// Current utilization in basis points.
    pub utilization_bps: i128,
    /// Borrow APR in basis points.
    pub rate_bps: i128,
}


// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebtError {
    Overflow,
    InvalidAmount,
    IndexInvariantViolated,
}

impl From<&'static str> for DebtError {
    fn from(_: &'static str) -> Self {
        DebtError::Overflow
    }
}

impl From<rate_model::RateModelError> for DebtError {
    fn from(_: rate_model::RateModelError) -> Self {
        DebtError::Overflow
    }
}

impl From<RoundingError> for DebtError {
    fn from(_: RoundingError) -> Self {
        DebtError::Overflow
    }
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

/// Load a debt position from persistent storage.
///
/// Returns a default zero-principal position if none is stored.
/// The default snapshot is set to `INDEX_SCALE` (1.0) so that a brand-new
/// position accrues no phantom interest.
pub fn load_debt(env: &Env, user: &Address) -> DebtPosition {
    let key = DataKey::Debt(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(DebtPosition {
            principal: 0,
            borrow_index_snapshot: INDEX_SCALE,
            last_update: env.ledger().timestamp(),
        })
}

/// Persist a debt position to storage.
pub fn save_debt(env: &Env, user: &Address, position: &DebtPosition) {
    let key = DataKey::Debt(user.clone());
    env.storage().persistent().set(&key, position);
}

// ---------------------------------------------------------------------------
// Global borrow index helpers
// ---------------------------------------------------------------------------

/// Load the current global borrow index.
///
/// Returns `INDEX_SCALE` (1.0) if the index has not yet been written
/// (first-ever call before `initialize`).
pub fn load_borrow_index(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::BorrowIndex)
        .unwrap_or(INDEX_SCALE)
}

/// Persist the global borrow index.
pub fn save_borrow_index(env: &Env, index: i128) {
    env.storage()
        .instance()
        .set(&DataKey::BorrowIndex, &index);
}

/// Load the timestamp of the last index update.
///
/// Returns the current ledger timestamp if none is stored (bootstrapping).
pub fn load_last_index_update(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::LastIndexUpdate)
        .unwrap_or_else(|| env.ledger().timestamp())
}

/// Persist the last-index-update timestamp.
pub fn save_last_index_update(env: &Env, ts: u64) {
    env.storage()
        .instance()
        .set(&DataKey::LastIndexUpdate, &ts);
}

// ---------------------------------------------------------------------------
// Index accrual
// ---------------------------------------------------------------------------

/// Advance the global borrow index by `elapsed` seconds at `rate_bps`.
///
/// Formula:
/// ```text
/// new_index = current_index + current_index * rate_bps * elapsed
///             / (SECONDS_PER_YEAR * BPS_DENOM)
/// ```
///
/// All intermediate multiplications use `checked_*` to detect overflow.
/// Returns the new (or unchanged, if elapsed == 0 or rate == 0) index.
///
/// # Overflow guard
/// If the new index would exceed `i128::MAX / INDEX_SCALE` the function
/// panics with `"BorrowIndex: overflow guard triggered"`.
///
/// # Monotonicity guarantee
/// The returned value is always `>= current_index`.
pub fn accrue_index(current_index: i128, elapsed: u64, rate_bps: i128) -> i128 {
    if elapsed == 0 || rate_bps == 0 {
        return current_index;
    }

    // Overflow guard: reject indices already dangerously large.
    let max_safe_index = i128::MAX / INDEX_SCALE;
    if current_index > max_safe_index {
        panic!("BorrowIndex: overflow guard triggered");
    }

    // delta = current_index * rate_bps * elapsed / (SECONDS_PER_YEAR * BPS_DENOM)
    let bps_denom: i128 = 10_000;
    let secs_per_year: i128 = SECONDS_PER_YEAR as i128;

    let step1 = current_index
        .checked_mul(rate_bps)
        .expect("BorrowIndex: overflow in rate multiplication");

    let step2 = step1
        .checked_mul(elapsed as i128)
        .expect("BorrowIndex: overflow in elapsed multiplication");

    let denominator = secs_per_year
        .checked_mul(bps_denom)
        .expect("BorrowIndex: denominator overflow");

    let delta = step2
        .checked_div(denominator)
        .expect("BorrowIndex: division by zero in accrual");

    let new_index = current_index
        .checked_add(delta)
        .expect("BorrowIndex: overflow on add");

    // Enforce monotonicity: never let index decrease.
    new_index.max(current_index)
}

/// Lazily advance the global borrow index to `now` and persist both the new
/// index value and the updated timestamp.
///
/// This is the single "touch" entry-point called by every mutating protocol
/// operation (borrow, repay, liquidate, migrate).
///
/// Returns the updated index value so callers can use it immediately without
/// a second storage round-trip.
pub fn touch_borrow_index(env: &Env, now: u64, rate_bps: i128) -> i128 {
    let current_index = load_borrow_index(env);
    let last_update = load_last_index_update(env);

    let elapsed = now.saturating_sub(last_update);
    let new_index = accrue_index(current_index, elapsed, rate_bps);

    // Only write if the index actually changed (saves a storage write on
    // same-block double-touches).
    if new_index != current_index {
        save_borrow_index(env, new_index);
    }
    save_last_index_update(env, now);
    new_index
}

// ---------------------------------------------------------------------------
// Per-position accrual (O(1) via index ratio)
// ---------------------------------------------------------------------------

/// Compute the current debt for a position using the index ratio:
///
/// ```text
/// current_debt = position.principal × current_index / snapshot_index
/// ```
///
/// Special cases:
/// - If `snapshot_index` is zero (pre-migration record), returns
///   `position.principal` unchanged (no phantom interest).
/// - If `current_index < snapshot_index` (should not happen under normal
///   operation), returns `position.principal` unchanged to avoid reducing
///   debt (Requirement 3.4 / monotonicity safety valve).
///
/// # Panics
/// Panics with a descriptive message if the multiplication overflows `i128`.
pub fn compute_debt(position: &DebtPosition, current_index: i128) -> i128 {
    let snapshot = position.borrow_index_snapshot;

    // Pre-migration record or degenerate state: treat accrued interest as zero.
    if snapshot <= 0 || current_index <= snapshot {
        return position.principal;
    }

    // principal * current_index / snapshot_index
    // Intermediate overflow check: principal * current_index must fit in i128.
    position
        .principal
        .checked_mul(current_index)
        .expect("compute_debt: principal × index overflow")
        .checked_div(snapshot)
        .expect("compute_debt: division by zero (snapshot)")
}

/// Settle a position's accrued interest into its principal and refresh the
/// index snapshot to `current_index`.
///
/// After settlement `position.principal` equals the full debt (including
/// interest), and `position.borrow_index_snapshot == current_index`.
///
/// Returns the settled `DebtPosition`.
pub fn settle_position(
    position: &DebtPosition,
    current_index: i128,
    now: u64,
) -> Result<DebtPosition, DebtError> {
    let new_principal = compute_debt(position, current_index);

    if new_principal < position.principal {
        // This violates the non-negative-interest invariant.
        return Err(DebtError::IndexInvariantViolated);
    }

    Ok(DebtPosition {
        principal: new_principal,
        borrow_index_snapshot: current_index,
        last_update: now,
    })
}

// ---------------------------------------------------------------------------
// Legacy per-position elapsed-time helpers (kept for backward compatibility
// with existing tests and the rounding_strategy module)
// ---------------------------------------------------------------------------

/// Compute elapsed seconds between two timestamps (saturating).
pub fn elapsed_seconds(now: u64, last_update: u64) -> u64 {
    now.saturating_sub(last_update)
}

/// Compute interest on `principal` over `elapsed` seconds at `rate_bps`.
///
/// Retained for backward compatibility with existing tests; new code should
/// use `compute_debt` + `touch_borrow_index` instead.
pub fn accrue_interest(principal: i128, elapsed: u64, rate_bps: i128) -> Result<i128, DebtError> {
    if principal == 0 || elapsed == 0 {
        return Ok(0);
    }

    let result =
        calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Bankers)?;

    if result.interest < 0 {
        return Err(DebtError::Overflow);
    }

    Ok(result.interest)
}

/// Compute gross interest for `principal` over `elapsed` seconds at `rate_bps`,
/// then split it between depositors and the protocol reserve.
///
/// # Formula
///
/// ```text
/// total_interest  = accrue_interest(principal, elapsed, rate_bps)
/// reserve_cut     = floor(total_interest * reserve_factor_bps / 10_000)
/// depositor_yield = total_interest - reserve_cut
/// ```
///
/// The depositor share is the complement, so `depositor_yield + reserve_cut ==
/// total_interest` exactly — no precision is lost to either side.
///
/// # Arguments
///
/// * `principal`           – Current settled principal (≥ 0).
/// * `elapsed`             – Seconds since last accrual.
/// * `rate_bps`            – Annual borrow rate in basis points (e.g. 500 = 5 %).
/// * `reserve_factor_bps`  – Fraction of interest kept by the protocol, in
///   basis points (0 = none, 10 000 = 100 %).
///
/// # Errors
///
/// Returns `DebtError::Overflow` on arithmetic overflow or if the reserve factor
/// exceeds 10 000 bps.
pub fn accrue_interest_split(
    principal: i128,
    elapsed: u64,
    rate_bps: i128,
    reserve_factor_bps: u32,
) -> Result<InterestSplit, DebtError> {
    let total_interest = accrue_interest(principal, elapsed, rate_bps)?;

    // Delegate to the pure math helper which validates reserve_factor_bps.
    let (depositor_yield, reserve_cut) =
        split_interest_by_reserve_factor(total_interest, reserve_factor_bps)
            .map_err(|_| DebtError::Overflow)?;

    Ok(InterestSplit {
        total_interest,
        depositor_yield,
        reserve_cut,
    })
}

/// Settle interest into `principal` using elapsed-time arithmetic.
///
/// Retained for backward compatibility.
pub fn settle_accrual(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError> {
    let elapsed = elapsed_seconds(now, position.last_update);
    let interest = accrue_interest(position.principal, elapsed, rate_bps)?;
    let principal = position
        .principal
        .checked_add(interest)
        .ok_or(DebtError::Overflow)?;

    Ok(DebtPosition {
        principal,
        borrow_index_snapshot: position.borrow_index_snapshot,
        last_update: now,
    })
}

/// Compute effective debt using elapsed-time arithmetic (read-only).
///
/// Retained for backward compatibility with view functions.
pub fn effective_debt(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
) -> Result<i128, DebtError> {
    let elapsed = elapsed_seconds(now, position.last_update);
    let interest = accrue_interest(position.principal, elapsed, rate_bps)?;
    position
        .principal
        .checked_add(interest)
        .ok_or(DebtError::Overflow)
}

// ---------------------------------------------------------------------------
// Mutating debt operations (index-aware)
// ---------------------------------------------------------------------------

/// Record a new borrow against `position`, settling accrued interest first.
///
/// The position's snapshot is refreshed to `current_index` after settlement.
pub fn borrow_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError> {
    if amount <= 0 {
        return Err(DebtError::InvalidAmount);
    }
    // Fall back to elapsed-time accrual for legacy positions with snapshot == 0.
    let mut settled = settle_accrual(&position, now, rate_bps)?;
    settled.principal = settled
        .principal
        .checked_add(amount)
        .ok_or(DebtError::Overflow)?;
    settled.last_update = now;
    Ok(settled)
}

/// Record a repayment against `position`, settling accrued interest first.
///
/// The position's snapshot is refreshed to `current_index` after settlement.
pub fn repay_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError> {
    if amount <= 0 {
        return Err(DebtError::InvalidAmount);
    }
    let mut settled = settle_accrual(&position, now, rate_bps)?;
    settled.principal = if amount >= settled.principal {
        0
    } else {
        settled.principal - amount
    };
    settled.last_update = now;
    Ok(settled)
}

/// Index-aware borrow: settle via index ratio, then add `amount`.
///
/// Preferred over `borrow_amount` once the global index is active.
pub fn borrow_amount_indexed(
    position: &DebtPosition,
    current_index: i128,
    now: u64,
    amount: i128,
) -> Result<DebtPosition, DebtError> {
    if amount <= 0 {
        return Err(DebtError::InvalidAmount);
    }
    let mut settled = settle_position(position, current_index, now)?;
    settled.principal = settled
        .principal
        .checked_add(amount)
        .ok_or(DebtError::Overflow)?;
    Ok(settled)
}

/// Index-aware repay: settle via index ratio, then subtract `amount`.
///
/// Preferred over `repay_amount` once the global index is active.
pub fn repay_amount_indexed(
    position: &DebtPosition,
    current_index: i128,
    now: u64,
    amount: i128,
) -> Result<DebtPosition, DebtError> {
    if amount <= 0 {
        return Err(DebtError::InvalidAmount);
    }
    let mut settled = settle_position(position, current_index, now)?;
    settled.principal = if amount >= settled.principal {
        0
    } else {
        settled.principal - amount
    };
    Ok(settled)
}

/// Loads the aggregate values needed to compute the global borrow rate once.
pub fn load_rate_snapshot(env: &Env) -> RateSnapshot {
    let storage = env.storage();
    let persistent = storage.persistent();
    let instance = storage.instance();

    RateSnapshot {
        total_debt: persistent.get(&DataKey::TotalDebt).unwrap_or(0),
        total_supply: persistent.get(&DataKey::TotalDeposits).unwrap_or(0),
        params: instance.get(&DataKey::RateParams),
    }
}

/// Computes the global borrow rate directly from current aggregate storage.
pub fn uncached_borrow_rate(env: &Env) -> i128 {
    let snapshot = load_rate_snapshot(env);
    compute_borrow_rate_from_snapshot(env, &snapshot).rate_bps
}

/// Computes utilization and borrow rate from a preloaded aggregate snapshot.
///
/// Utilization uses checked arithmetic and falls back to zero when supply is
/// non-positive. Overflow in `debt * 10_000` returns [`DebtError::Overflow`].
pub(crate) fn try_compute_borrow_rate_from_snapshot(
    env: &Env,
    snapshot: &RateSnapshot,
) -> Result<BorrowRateComputation, DebtError> {
    let utilization_bps = if snapshot.total_supply > 0 {
        snapshot
            .total_debt
            .checked_mul(BPS_DENOM)
            .ok_or(DebtError::Overflow)?
            .checked_div(snapshot.total_supply)
            .ok_or(DebtError::Overflow)?
    } else {
        0
    };

    let rate_bps = match &snapshot.params {
        Some(p) => {
            let target_rate = rate_model::compute_borrow_rate(utilization_bps, p)?;
            crate::rate_model::update_and_get_rate(env, target_rate, p)
        }
        None => DEFAULT_APR_BPS,
    };

    Ok(BorrowRateComputation {
        utilization_bps,
        rate_bps,
    })
}

/// Computes utilization and borrow rate from a preloaded aggregate snapshot.
///
/// Panics on arithmetic overflow, matching the existing borrow-rate API shape
/// while keeping the underlying arithmetic checked.
pub(crate) fn compute_borrow_rate_from_snapshot(env: &Env, snapshot: &RateSnapshot) -> BorrowRateComputation {
    try_compute_borrow_rate_from_snapshot(env, snapshot).expect("borrow-rate utilization overflow")
}

fn uncached_borrow_rate_computation(env: &Env) -> BorrowRateComputation {
    let snapshot = load_rate_snapshot(env);
    compute_borrow_rate_from_snapshot(env, &snapshot)
}

/// Returns the global borrow rate, computing it at most once per ledger.
///
/// The temporary-storage key includes `env.ledger().sequence()`, so advancing
/// the ledger naturally misses the previous cache entry and recomputes from a
/// fresh `RateSnapshot`. Each cache miss also writes one utilization sample for
/// the current ledger into the bounded utilization-history ring buffer.
pub fn cached_borrow_rate(env: &Env) -> i128 {
    let ledger_sequence = env.ledger().sequence();
    let key = DataKey::BorrowRateCache(ledger_sequence);

    if let Some(cache) = env
        .storage()
        .temporary()
        .get::<DataKey, BorrowRateCache>(&key)
    {
        if cache.ledger_sequence == ledger_sequence {
            return cache.rate_bps;
        }
    }

    let computation = uncached_borrow_rate_computation(env);
    write_utilization_sample(env, computation.utilization_bps);
    let cache = BorrowRateCache {
        ledger_sequence,
        rate_bps: computation.rate_bps,
    };
    env.storage().temporary().set(&key, &cache);
    computation.rate_bps
}

/// Settle accrued interest and return both the updated `DebtPosition` **and**
/// the `InterestSplit` that describes how the gross interest is divided between
/// depositor yield and protocol reserve.
pub fn settle_accrual_split(
    position: &DebtPosition,
    now: u64,
    rate_bps: i128,
    reserve_factor_bps: u32,
) -> Result<(DebtPosition, InterestSplit), DebtError> {
    let elapsed = elapsed_seconds(now, position.last_update);
    let split = accrue_interest_split(position.principal, elapsed, rate_bps, reserve_factor_bps)?;

    let principal = position
        .principal
        .checked_add(split.total_interest)
        .ok_or(DebtError::Overflow)?;

    let updated = DebtPosition {
        principal,
        borrow_index_snapshot: position.borrow_index_snapshot,
        last_update: now,
    };

    Ok((updated, split))
}

/// Compute the **depositor supply rate** (in basis points) that corresponds to
/// the current borrow rate and utilization after applying the reserve factor.
///
/// This derives the supply-side APR that depositors *effectively* earn, using
/// the same scale constants as the borrow side so the two rates are always
/// consistent.
///
/// # Formula
///
/// ```text
/// supply_rate_bps = borrow_rate_bps
///                   * utilization_bps / 10_000
///                   * (10_000 − reserve_factor_bps) / 10_000
/// ```
///
/// When `reserve_factor_bps == 0` the formula reduces to
/// `borrow_rate * utilization / 10_000`, which is the full utilization-weighted
/// borrow rate — identical to the previous (no-reserve) behaviour.
///
/// # Arguments
///
/// * `borrow_rate_bps`    – Current borrow APR in basis points.
/// * `utilization_bps`    – Current utilization in basis points
///   (total_borrows * 10_000 / total_deposits).
/// * `reserve_factor_bps` – Fraction of interest retained by the protocol, in
///   basis points (0 = none, 10 000 = 100 %).
///
/// # Returns
///
/// The supply APR in basis points.
///
/// Returns `DebtError::Overflow` if any intermediate calculation overflows or
/// an input is out of range.
pub fn effective_supply_rate(
    borrow_rate_bps: i128,
    utilization_bps: i128,
    reserve_factor_bps: u32,
) -> Result<i128, DebtError> {
    use crate::rounding_strategy::BASIS_POINTS_SCALE;

    // Guard inputs so we fail clearly rather than produce silent garbage.
    if borrow_rate_bps < 0 || utilization_bps < 0 {
        return Err(DebtError::Overflow);
    }
    if reserve_factor_bps > BASIS_POINTS_SCALE as u32 {
        return Err(DebtError::Overflow);
    }

    let scale = BASIS_POINTS_SCALE; // 10_000

    // Step 1: utilization-weighted borrow rate
    let rate_util = borrow_rate_bps
        .checked_mul(utilization_bps)
        .ok_or(DebtError::Overflow)?
        .checked_div(scale)
        .ok_or(DebtError::Overflow)?;

    // Step 2: apply (1 − reserve_factor)
    let one_minus_reserve = scale
        .checked_sub(reserve_factor_bps as i128)
        .ok_or(DebtError::Overflow)?;

    let supply_rate = rate_util
        .checked_mul(one_minus_reserve)
        .ok_or(DebtError::Overflow)?
        .checked_div(scale)
        .ok_or(DebtError::Overflow)?;

    Ok(supply_rate.max(0))
}

// ─── Accrual-split event log ──────────────────────────────────────────────────

/// One entry in the persistent accrual-split history.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AccrualSplitEntry {
    pub borrower: Address,
    pub timestamp: u64,
    pub total_interest: i128,
    pub depositor_yield: i128,
    pub reserve_cut: i128,
}

const KEY_ACCRUAL_LOG: &str = "accrual_log";

/// Append a settle_accrual_split result to the persistent log and emit a
/// `settle_accrual_split` contract event for off-chain indexers.
///
/// Call this immediately after `settle_accrual_split` so the split is
/// recorded for both on-chain history (via `get_accrual_split_log`) and
/// off-chain TWAP/revenue attribution consumers.
pub fn record_accrual_split(env: &Env, borrower: &Address, timestamp: u64, split: &InterestSplit) {
    let entry = AccrualSplitEntry {
        borrower: borrower.clone(),
        timestamp,
        total_interest: split.total_interest,
        depositor_yield: split.depositor_yield,
        reserve_cut: split.reserve_cut,
    };

    let mut log: Vec<AccrualSplitEntry> = env
        .storage()
        .persistent()
        .get(&Symbol::new(env, KEY_ACCRUAL_LOG))
        .unwrap_or_else(|| Vec::new(env));
    log.push_back(entry.clone());
    env.storage()
        .persistent()
        .set(&Symbol::new(env, KEY_ACCRUAL_LOG), &log);

    env.events().publish(
        (symbol_short!("accrual"), borrower.clone()),
        (
            split.total_interest,
            split.depositor_yield,
            split.reserve_cut,
        ),
    );
}

/// Return the full history of recorded accrual splits.
pub fn get_accrual_split_log(env: &Env) -> Vec<AccrualSplitEntry> {
    env.storage()
        .persistent()
        .get(&Symbol::new(env, KEY_ACCRUAL_LOG))
        .unwrap_or_else(|| Vec::new(env))
}


