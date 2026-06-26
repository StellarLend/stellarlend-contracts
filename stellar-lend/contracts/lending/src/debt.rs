use soroban_sdk::{contracttype, Address, Env};

use crate::math::split_interest_by_reserve_factor;
use crate::rounding_strategy::{calculate_interest_with_rounding, RoundingError, RoundingMode};
use crate::{rate_model, DataKey};
use stellar_lend_common::BPS_DENOM;

/// Default APR when no dynamic rate is available: 5% (500 bps).
pub const DEFAULT_APR_BPS: i128 = 500;

/// Reserve factor used when no explicit value is configured: 0% (protocol takes nothing).
///
/// Keeping the default at zero preserves existing behaviour for any call site
/// that has not been updated to pass an explicit reserve factor.
pub const DEFAULT_RESERVE_FACTOR_BPS: u32 = 0;

// ─── Core position type ───────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebtPosition {
    pub principal: i128,
    pub last_update: u64,
}

#[contracttype]
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

// ─── Error type ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebtError {
    Overflow,
    InvalidAmount,
}

impl From<&'static str> for DebtError {
    fn from(_: &'static str) -> Self {
        DebtError::Overflow
    }
}

impl From<RoundingError> for DebtError {
    fn from(_: RoundingError) -> Self {
        DebtError::Overflow
    }
}

// ─── Interest-split result ────────────────────────────────────────────────────

/// The result of splitting accrued borrow interest between depositors and the
/// protocol reserve.
///
/// # Invariant
///
/// `depositor_yield + reserve_cut == total_interest` always holds.  Neither
/// field is ever negative.  The split is derived from
/// [`split_interest_by_reserve_factor`] in `math.rs`, which floors the
/// reserve cut so any fractional unit falls to the depositor side.
///
/// # Fields
///
/// * `total_interest`  – Gross interest accrued by the borrower.
/// * `depositor_yield` – The portion that belongs to depositors.
///   `= total_interest * (BPS_SCALE - reserve_factor_bps) / BPS_SCALE`
///   (computed as complement to avoid double-rounding).
/// * `reserve_cut`     – The portion retained by the protocol reserve.
///   `= floor(total_interest * reserve_factor_bps / BPS_SCALE)`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterestSplit {
    /// Gross interest accrued during the period.
    pub total_interest: i128,
    /// Share of interest owed to depositors (supply-side yield).
    pub depositor_yield: i128,
    /// Share of interest retained by the protocol reserve.
    pub reserve_cut: i128,
}

// ─── Storage helpers ──────────────────────────────────────────────────────────

pub fn load_debt(env: &Env, user: &Address) -> DebtPosition {
    let key = DataKey::Debt(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(DebtPosition {
            principal: 0,
            last_update: env.ledger().timestamp(),
        })
}

pub fn save_debt(env: &Env, user: &Address, position: &DebtPosition) {
    let key = DataKey::Debt(user.clone());
    env.storage().persistent().set(&key, position);
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

    match snapshot.params {
        Some(p) => {
            let utilization_bps = if snapshot.total_supply > 0 {
                snapshot.total_debt.saturating_mul(BPS_DENOM) / snapshot.total_supply
            } else {
                0
            };
            rate_model::compute_borrow_rate(utilization_bps, &p)
        }
        None => DEFAULT_APR_BPS,
    }
}

/// Returns the global borrow rate, computing it at most once per ledger.
///
/// The temporary-storage key includes `env.ledger().sequence()`, so advancing
/// the ledger naturally misses the previous cache entry and recomputes from a
/// fresh `RateSnapshot`.
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

    let rate_bps = uncached_borrow_rate(env);
    let cache = BorrowRateCache {
        ledger_sequence,
        rate_bps,
    };
    env.storage().temporary().set(&key, &cache);
    rate_bps
}

// ─── Time helpers ─────────────────────────────────────────────────────────────

pub fn elapsed_seconds(now: u64, last_update: u64) -> u64 {
    now.saturating_sub(last_update)
}

// ─── Interest accrual (borrow side) ──────────────────────────────────────────

/// Compute the gross interest accrued on `principal` over `elapsed` seconds at
/// `rate_bps` (annual, in basis points).
///
/// Uses **Bankers rounding** to minimise cumulative drift over many accruals.
/// Returns the interest *delta* only — not `principal + interest`.
///
/// Returns `Ok(0)` when either `principal` or `elapsed` is zero.
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

/// Compute gross interest *and* split it between depositor yield and protocol
/// reserve in one pass.
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

// ─── Settle helpers ───────────────────────────────────────────────────────────

/// Settle accrued interest into the principal using the **borrow rate only**.
///
/// This is the original single-value settlement path, unchanged.  The returned
/// `DebtPosition` has `principal = old_principal + total_interest` and
/// `last_update = now`.
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
        last_update: now,
    })
}

/// Settle accrued interest and return both the updated `DebtPosition` **and**
/// the `InterestSplit` that describes how the gross interest is divided between
/// depositor yield and protocol reserve.
///
/// This is the primary entry point for any code path that needs to credit
/// depositors and fund the reserve simultaneously with debt settlement.
///
/// # Invariant
///
/// `split.depositor_yield + split.reserve_cut == split.total_interest`.
///
/// The `DebtPosition` in the return value has the same `principal` as
/// `settle_accrual` would produce — the split is purely an accounting
/// decomposition of the gross interest.
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
        last_update: now,
    };

    Ok((updated, split))
}

// ─── View helpers ─────────────────────────────────────────────────────────────

/// Read-only equivalent of `settle_accrual`.
///
/// Returns what the total debt (principal + accrued interest) would be right
/// now without writing any state.  Used by view queries such as
/// `get_position` and `get_health_factor`.
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
/// Equivalently:
///
/// ```text
/// supply_rate_bps = compute_supply_rate(borrow_rate_bps, utilization_bps, reserve_factor_bps)
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
/// The supply APR in basis points, clamped to
/// `[0, crate::math::MAX_RATE_BPS]`.
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
    //   rate_util = borrow_rate_bps * utilization_bps / 10_000
    let rate_util = borrow_rate_bps
        .checked_mul(utilization_bps)
        .ok_or(DebtError::Overflow)?
        .checked_div(scale)
        .ok_or(DebtError::Overflow)?;

    // Step 2: apply (1 − reserve_factor)
    //   supply_rate = rate_util * (10_000 − reserve_factor_bps) / 10_000
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

// ─── Borrow / repay mutations ─────────────────────────────────────────────────

pub fn borrow_amount(
    position: DebtPosition,
    now: u64,
    amount: i128,
    rate_bps: i128,
) -> Result<DebtPosition, DebtError> {
    if amount <= 0 {
        return Err(DebtError::InvalidAmount);
    }

    let mut settled = settle_accrual(&position, now, rate_bps)?;
    settled.principal = settled
        .principal
        .checked_add(amount)
        .ok_or(DebtError::Overflow)?;
    settled.last_update = now;
    Ok(settled)
}

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
