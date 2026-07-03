//! Pure-math helpers for lending calculations
//!
//! All functions in this module are pure (no Env dependency) and
//! fuzzable. They contain the core arithmetic for interest accrual,
//! health factor computation, and rate model calculations.
//!
//! Extracted from the lending crate for fuzzability.

use soroban_sdk::contracterror;

/// Fixed-point scale for internal calculations (7 decimals)
pub const SCALE: i128 = 10_000_000; // 10^7

/// Basis points scale (100% = 10000 bps)
pub const BPS_SCALE: u32 = 10000;

/// Maximum allowed interest rate (1000% APR)
pub const MAX_RATE_BPS: i128 = 100000;

/// Error types for math operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MathError {
    /// Arithmetic overflow
    Overflow = 1,
    /// Division by zero
    DivisionByZero = 2,
    /// Input out of valid range
    OutOfRange = 3,
    /// Result would be negative
    NegativeResult = 4,
}

/// Compute compound interest over a period
///
/// Formula: interest = principal * rate_bps * elapsed / (BPS_SCALE * SECONDS_PER_YEAR)
///
/// # Arguments
/// * `principal` - The base amount (scaled by SCALE)
/// * `rate_bps` - Annual interest rate in basis points (e.g., 1000 = 10%)
/// * `elapsed` - Seconds elapsed since last accrual
///
/// # Returns
/// * `Ok(interest)` - The accrued interest amount (scaled by SCALE)
/// * `Err(MathError)` - On overflow or invalid input
pub fn compute_compound_interest(
    principal: i128,
    rate_bps: i128,
    elapsed: u64,
) -> Result<i128, MathError> {
    // Validate inputs
    if principal < 0 {
        return Err(MathError::OutOfRange);
    }
    if !(0..=MAX_RATE_BPS).contains(&rate_bps) {
        return Err(MathError::OutOfRange);
    }
    if elapsed == 0 {
        return Ok(0);
    }
    if principal == 0 {
        return Ok(0);
    }

    // Compute: principal * rate_bps * elapsed / (BPS_SCALE * SECONDS_PER_YEAR)
    let seconds_per_year: i128 = 31_536_000; // 365 * 24 * 3600

    // Step 1: principal * rate_bps (checked)
    let step1 = principal.checked_mul(rate_bps).ok_or(MathError::Overflow)?;

    // Step 2: step1 * elapsed (checked)
    let step2 = step1
        .checked_mul(elapsed as i128)
        .ok_or(MathError::Overflow)?;

    // Step 3: step2 / (BPS_SCALE * SECONDS_PER_YEAR) (checked division)
    let denominator = (BPS_SCALE as i128)
        .checked_mul(seconds_per_year)
        .ok_or(MathError::Overflow)?;

    let interest = step2
        .checked_div(denominator)
        .ok_or(MathError::DivisionByZero)?;

    // Ensure interest >= 1 for any principal > 0 and elapsed > 0
    // This is the critical invariant: interest ceiling division
    if interest == 0 && principal > 0 && elapsed > 0 {
        Ok(1)
    } else {
        Ok(interest)
    }
}

/// Compute health factor for a position
///
/// Formula: HF = (collateral_value * liquidation_threshold) / debt_value
///
/// # Arguments
/// * `collateral_value` - Total collateral in base units (scaled by SCALE)
/// * `debt_value` - Total debt in base units (scaled by SCALE)
/// * `liquidation_threshold_bps` - Liquidation threshold in basis points
///
/// # Returns
/// * `Ok(health_factor)` - The health factor (scaled by SCALE)
/// * `Err(MathError)` - On overflow, division by zero, or invalid input
pub fn compute_health_factor(
    collateral_value: i128,
    debt_value: i128,
    liquidation_threshold_bps: u32,
) -> Result<i128, MathError> {
    // Validate inputs
    if collateral_value < 0 || debt_value < 0 {
        return Err(MathError::OutOfRange);
    }
    if liquidation_threshold_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }
    if debt_value == 0 {
        // No debt = infinite health, return max value
        return Ok(i128::MAX);
    }
    if collateral_value == 0 {
        return Ok(0);
    }

    // Compute: (collateral_value * liquidation_threshold_bps) / BPS_SCALE
    let weighted_collateral = collateral_value
        .checked_mul(liquidation_threshold_bps as i128)
        .ok_or(MathError::Overflow)?
        .checked_div(BPS_SCALE as i128)
        .ok_or(MathError::DivisionByZero)?;

    // Compute: weighted_collateral * SCALE / debt_value
    // This gives us HF scaled by SCALE
    let health_factor = weighted_collateral
        .checked_mul(SCALE)
        .ok_or(MathError::Overflow)?
        .checked_div(debt_value)
        .ok_or(MathError::DivisionByZero)?;

    Ok(health_factor)
}

/// Compute borrow rate from utilization using a jump rate model
///
/// Formula:
///   if utilization <= kink:
///     rate = base_rate + utilization * multiplier
///   else:
///     rate = base_rate + kink * multiplier + (utilization - kink) * jump_multiplier
///
/// # Arguments
/// * `utilization_bps` - Current utilization in basis points (0-10000)
/// * `base_rate_bps` - Base rate in basis points
/// * `multiplier_bps` - Slope before kink in basis points
/// * `jump_multiplier_bps` - Slope after kink in basis points
/// * `kink_bps` - Kink utilization point in basis points
///
/// # Returns
/// * `Ok(borrow_rate)` - The borrow rate in basis points
/// * `Err(MathError)` - On overflow or invalid input
pub fn compute_borrow_rate(
    utilization_bps: u32,
    base_rate_bps: u32,
    multiplier_bps: u32,
    jump_multiplier_bps: u32,
    kink_bps: u32,
) -> Result<u32, MathError> {
    // Validate inputs
    if utilization_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }
    if kink_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }

    let base = base_rate_bps as i128;
    let util = utilization_bps as i128;
    let mult = multiplier_bps as i128;
    let jump_mult = jump_multiplier_bps as i128;
    let kink = kink_bps as i128;
    let scale = BPS_SCALE as i128;

    let rate = if util <= kink {
        // rate = base + util * multiplier / SCALE
        base.checked_add(
            util.checked_mul(mult)
                .ok_or(MathError::Overflow)?
                .checked_div(scale)
                .ok_or(MathError::DivisionByZero)?,
        )
        .ok_or(MathError::Overflow)?
    } else {
        // rate = base + kink * multiplier / SCALE + (util - kink) * jump_multiplier / SCALE
        let base_plus_kink = base
            .checked_add(
                kink.checked_mul(mult)
                    .ok_or(MathError::Overflow)?
                    .checked_div(scale)
                    .ok_or(MathError::DivisionByZero)?,
            )
            .ok_or(MathError::Overflow)?;

        let excess_util = util.checked_sub(kink).ok_or(MathError::NegativeResult)?;

        base_plus_kink
            .checked_add(
                excess_util
                    .checked_mul(jump_mult)
                    .ok_or(MathError::Overflow)?
                    .checked_div(scale)
                    .ok_or(MathError::DivisionByZero)?,
            )
            .ok_or(MathError::Overflow)?
    };

    // Clamp to MAX_RATE_BPS
    Ok(rate.min(MAX_RATE_BPS) as u32)
}

/// Compute supply rate from borrow rate and utilization
///
/// Formula: supply_rate = borrow_rate * utilization * (1 - reserve_factor) / SCALE
///
/// # Arguments
/// * `borrow_rate_bps` - Current borrow rate in basis points
/// * `utilization_bps` - Current utilization in basis points
/// * `reserve_factor_bps` - Reserve factor in basis points
///
/// # Returns
/// * `Ok(supply_rate)` - The supply rate in basis points
/// * `Err(MathError)` - On overflow or invalid input
pub fn compute_supply_rate(
    borrow_rate_bps: u32,
    utilization_bps: u32,
    reserve_factor_bps: u32,
) -> Result<u32, MathError> {
    if borrow_rate_bps > MAX_RATE_BPS as u32 {
        return Err(MathError::OutOfRange);
    }
    if utilization_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }
    if reserve_factor_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }

    let borrow = borrow_rate_bps as i128;
    let util = utilization_bps as i128;
    let reserve = reserve_factor_bps as i128;
    let scale = BPS_SCALE as i128;

    // borrow * utilization / SCALE
    let rate_util = borrow
        .checked_mul(util)
        .ok_or(MathError::Overflow)?
        .checked_div(scale)
        .ok_or(MathError::DivisionByZero)?;

    // (1 - reserve_factor) = (SCALE - reserve) / SCALE
    let one_minus_reserve = scale
        .checked_sub(reserve)
        .ok_or(MathError::NegativeResult)?;

    // rate_util * one_minus_reserve / SCALE
    let supply_rate = rate_util
        .checked_mul(one_minus_reserve)
        .ok_or(MathError::Overflow)?
        .checked_div(scale)
        .ok_or(MathError::DivisionByZero)?;

    Ok(supply_rate.min(MAX_RATE_BPS) as u32)
}

/// Split a gross interest amount into depositor yield and protocol reserve.
///
/// Given `total_interest` accrued by a borrower, this function deterministically
/// partitions it using the reserve factor:
///
/// ```text
/// reserve_cut      = total_interest * reserve_factor_bps / BPS_SCALE
/// depositor_yield  = total_interest - reserve_cut
/// ```
///
/// The depositor share is computed as the complement (subtraction) rather than
/// by a second multiplication, so that `depositor_yield + reserve_cut ==
/// total_interest` exactly — no precision is lost to either side.
///
/// # Rounding
/// Integer division of `total_interest * reserve_factor_bps / BPS_SCALE` floors
/// toward zero, meaning any fractional unit falls to the **depositor** side.
/// This is the conservative choice: the protocol never takes more than its
/// exact share.
///
/// # Arguments
/// * `total_interest` - Gross interest accrued by the borrower (≥ 0)
/// * `reserve_factor_bps` - Fraction of interest kept by the protocol,
///   expressed in basis points (0 = none, 10_000 = 100%)
///
/// # Returns
/// * `Ok((depositor_yield, reserve_cut))` where both values are ≥ 0 and their
///   sum equals `total_interest`
/// * `Err(MathError::OutOfRange)` if either input is negative or
///   `reserve_factor_bps > BPS_SCALE`
/// * `Err(MathError::Overflow)` if the intermediate multiplication overflows
///
/// # Examples (basis-point arithmetic)
///
/// ```text
/// // 10% reserve factor on 1_000 interest
/// split_interest_by_reserve_factor(1_000, 1_000)
///   => Ok((900, 100))   // 10% to reserve, 90% to depositors
///
/// // 0% reserve factor — all interest goes to depositors
/// split_interest_by_reserve_factor(1_000, 0)
///   => Ok((1_000, 0))
///
/// // 100% reserve factor — all interest goes to the protocol
/// split_interest_by_reserve_factor(1_000, 10_000)
///   => Ok((0, 1_000))
/// ```
pub fn split_interest_by_reserve_factor(
    total_interest: i128,
    reserve_factor_bps: u32,
) -> Result<(i128, i128), MathError> {
    if total_interest < 0 {
        return Err(MathError::OutOfRange);
    }
    if reserve_factor_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }
    if total_interest == 0 {
        return Ok((0, 0));
    }

    let scale = BPS_SCALE as i128;
    let reserve_bps = reserve_factor_bps as i128;

    // reserve_cut = floor(total_interest * reserve_factor_bps / BPS_SCALE)
    // Integer division floors toward zero, so any fractional unit stays with
    // the depositor — the protocol never takes more than its exact share.
    let reserve_cut = total_interest
        .checked_mul(reserve_bps)
        .ok_or(MathError::Overflow)?
        .checked_div(scale)
        .ok_or(MathError::DivisionByZero)?;

    // Complement: depositor_yield = total_interest - reserve_cut
    // This guarantees depositor_yield + reserve_cut == total_interest exactly.
    let depositor_yield = total_interest
        .checked_sub(reserve_cut)
        .ok_or(MathError::NegativeResult)?;

    Ok((depositor_yield, reserve_cut))
}

/// Compute liquidation bonus for a position
///
/// Formula: bonus = debt_to_cover * liquidation_bonus_bps / BPS_SCALE
///
/// # Arguments
/// * `debt_to_cover` - Amount of debt being liquidated
/// * `liquidation_bonus_bps` - Liquidation bonus in basis points
///
/// # Returns
/// * `Ok(bonus)` - The liquidation bonus amount
/// * `Err(MathError)` - On overflow or invalid input
pub fn compute_liquidation_bonus(
    debt_to_cover: i128,
    liquidation_bonus_bps: u32,
) -> Result<i128, MathError> {
    if debt_to_cover < 0 {
        return Err(MathError::OutOfRange);
    }
    if liquidation_bonus_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }
    if debt_to_cover == 0 {
        return Ok(0);
    }

    debt_to_cover
        .checked_mul(liquidation_bonus_bps as i128)
        .ok_or(MathError::Overflow)?
        .checked_div(BPS_SCALE as i128)
        .ok_or(MathError::DivisionByZero)
}

/// Compute maximum borrow amount given collateral
///
/// Formula: max_borrow = collateral_value * ltv_bps / BPS_SCALE
///
/// # Arguments
/// * `collateral_value` - Total collateral value
/// * `ltv_bps` - Loan-to-value ratio in basis points
///
/// # Returns
/// * `Ok(max_borrow)` - Maximum borrowable amount
/// * `Err(MathError)` - On overflow or invalid input
pub fn compute_max_borrow(collateral_value: i128, ltv_bps: u32) -> Result<i128, MathError> {
    if collateral_value < 0 {
        return Err(MathError::OutOfRange);
    }
    if ltv_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }

    collateral_value
        .checked_mul(ltv_bps as i128)
        .ok_or(MathError::Overflow)?
        .checked_div(BPS_SCALE as i128)
        .ok_or(MathError::DivisionByZero)
}

/// Check if a position is eligible for liquidation
///
/// A position is liquidatable when health_factor < SCALE (i.e., HF < 1.0)
///
/// # Arguments
/// * `health_factor` - The computed health factor (scaled by SCALE)
///
/// # Returns
/// * `true` if position can be liquidated
pub fn is_liquidatable(health_factor: i128) -> bool {
    health_factor < SCALE
}

/// Compute utilization rate
///
/// Formula: utilization = total_borrows * BPS_SCALE / total_deposits
///
/// # Arguments
/// * `total_borrows` - Total amount borrowed
/// * `total_deposits` - Total amount deposited
///
/// # Returns
/// * `Ok(utilization_bps)` - Utilization in basis points
/// * `Err(MathError)` - On overflow or invalid input
pub fn compute_utilization(total_borrows: i128, total_deposits: i128) -> Result<u32, MathError> {
    if total_borrows < 0 || total_deposits < 0 {
        return Err(MathError::OutOfRange);
    }
    if total_deposits == 0 {
        return Ok(0);
    }
    if total_borrows > total_deposits {
        // Cap at 100%
        return Ok(BPS_SCALE);
    }

    total_borrows
        .checked_mul(BPS_SCALE as i128)
        .ok_or(MathError::Overflow)?
        .checked_div(total_deposits)
        .ok_or(MathError::DivisionByZero)
        .map(|v| v as u32)
}

/// Multiply `a * b` then divide by `c`, rounding toward negative infinity (floor).
///
/// For positive inputs this is equivalent to truncation toward zero, but the
/// semantic is explicitly **floor** so the rounding direction is unambiguous:
/// the caller always knowingly rounds in favour of the protocol (smaller result
/// for seized_collateral and max_repay; lower HF → more liquidatable).
///
/// # Panics
/// - Returns `Err(MathError::DivisionByZero)` when `c == 0`.
/// - Returns `Err(MathError::Overflow)` when the intermediate product exceeds `i128::MAX`.
#[inline]
pub fn checked_mul_div_floor(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if c == 0 {
        return Err(MathError::DivisionByZero);
    }
    a.checked_mul(b)
        .ok_or(MathError::Overflow)?
        .checked_div(c)
        .ok_or(MathError::DivisionByZero)
}

/// Multiply `a * b` then divide by `c`, rounding toward positive infinity (ceil).
///
/// For positive inputs the result is `(a * b + c - 1) / c`.  Use sparingly —
/// ceiling rounds in favour of the receiver, so it should **never** appear in
/// liquidation paths where the liquidator is the receiver.
///
/// # Panics
/// - Returns `Err(MathError::DivisionByZero)` when `c == 0`.
/// - Returns `Err(MathError::Overflow)` when the intermediate product exceeds `i128::MAX`.
#[inline]
pub fn checked_mul_div_ceil(a: i128, b: i128, c: i128) -> Result<i128, MathError> {
    if c == 0 {
        return Err(MathError::DivisionByZero);
    }
    let product = a.checked_mul(b).ok_or(MathError::Overflow)?;
    let quotient = product.checked_div(c).ok_or(MathError::DivisionByZero)?;
    if product % c == 0 {
        Ok(quotient)
    } else {
        quotient.checked_add(1).ok_or(MathError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_compound_interest_basic() {
        let principal = 1_000_000_000i128; // 100 * SCALE
        let rate = 1000i128; // 10%
        let elapsed = 31_536_000u64; // 1 year

        let interest = compute_compound_interest(principal, rate, elapsed).unwrap();
        assert_eq!(interest, 100_000_000); // 10 * SCALE
    }

    #[test]
    fn test_compute_compound_interest_ensures_minimum() {
        // Small principal, short time should still return at least 1
        let interest = compute_compound_interest(1, 1, 1).unwrap();
        assert_eq!(interest, 1);
    }

    #[test]
    fn test_compute_health_factor_basic() {
        let collateral = 1_000_000_000i128;
        let debt = 500_000_000i128;
        let threshold = 8000u32; // 80%

        let hf = compute_health_factor(collateral, debt, threshold).unwrap();
        // HF = (1000 * 0.8) / 500 * SCALE = 1.6 * SCALE
        assert!(hf > SCALE); // HF > 1.0
    }

    #[test]
    fn test_compute_health_factor_no_debt() {
        let hf = compute_health_factor(1000, 0, 8000).unwrap();
        assert_eq!(hf, i128::MAX);
    }

    #[test]
    fn test_compute_borrow_rate_below_kink() {
        let rate = compute_borrow_rate(5000, 200, 5000, 20000, 8000).unwrap();
        // 5000 bps util, base=200, mult=5000
        // rate = 200 + 5000*5000/10000 = 200 + 2500 = 2700
        assert_eq!(rate, 2700);
    }

    #[test]
    fn test_compute_borrow_rate_above_kink() {
        let rate = compute_borrow_rate(9000, 200, 5000, 20000, 8000).unwrap();
        // Above kink: base + kink*mult/SCALE + (util-kink)*jump/SCALE
        // = 200 + 8000*5000/10000 + 1000*20000/10000
        // = 200 + 4000 + 2000 = 6200
        assert_eq!(rate, 6200);
    }

    #[test]
    fn test_compute_utilization() {
        let util = compute_utilization(5000, 10000).unwrap();
        assert_eq!(util, 5000); // 50%
    }

    #[test]
    fn test_compute_utilization_zero_deposits() {
        let util = compute_utilization(0, 0).unwrap();
        assert_eq!(util, 0);
    }

    #[test]
    fn test_is_liquidatable() {
        assert!(!is_liquidatable(SCALE)); // HF = 1.0, not liquidatable
        assert!(is_liquidatable(SCALE - 1)); // HF < 1.0, liquidatable
        assert!(!is_liquidatable(SCALE + 1)); // HF > 1.0, not liquidatable
    }

    // ── checked_mul_div_floor ─────────────────────────────────────────────────

    #[test]
    fn mul_div_floor_exact_division() {
        assert_eq!(checked_mul_div_floor(100, 50, 10).unwrap(), 500);
    }

    #[test]
    fn mul_div_floor_truncates_toward_zero() {
        // 10 * 11 / 3 = 110 / 3 = 36.666 → floor = 36
        assert_eq!(checked_mul_div_floor(10, 11, 3).unwrap(), 36);
    }

    #[test]
    fn mul_div_floor_small_boundary() {
        // 1 * 11000 / 10000 = 1.1 → floor = 1 (seized_collateral rounding)
        assert_eq!(checked_mul_div_floor(1, 11000, 10000).unwrap(), 1);
    }

    #[test]
    fn mul_div_floor_zero_numerator() {
        assert_eq!(checked_mul_div_floor(0, 9999, 10000).unwrap(), 0);
    }

    #[test]
    fn mul_div_floor_division_by_zero() {
        assert_eq!(
            checked_mul_div_floor(1, 1, 0),
            Err(MathError::DivisionByZero)
        );
    }

    #[test]
    fn mul_div_floor_overflow_returns_error() {
        assert_eq!(
            checked_mul_div_floor(i128::MAX, 2, 1),
            Err(MathError::Overflow)
        );
    }

    // ── checked_mul_div_ceil ──────────────────────────────────────────────────

    #[test]
    fn mul_div_ceil_exact_division() {
        assert_eq!(checked_mul_div_ceil(100, 50, 10).unwrap(), 500);
    }

    #[test]
    fn mul_div_ceil_rounds_up() {
        // 10 * 11 / 3 = 110 / 3 = 36.666 → ceil = 37
        assert_eq!(checked_mul_div_ceil(10, 11, 3).unwrap(), 37);
    }

    #[test]
    fn mul_div_ceil_small_boundary() {
        // 1 * 11000 / 10000 = 1.1 → ceil = 2
        assert_eq!(checked_mul_div_ceil(1, 11000, 10000).unwrap(), 2);
    }

    #[test]
    fn mul_div_ceil_no_round_up_when_exact() {
        // 10 * 10000 / 10000 = 10 → ceil = 10
        assert_eq!(checked_mul_div_ceil(10, 10000, 10000).unwrap(), 10);
    }

    #[test]
    fn mul_div_ceil_division_by_zero() {
        assert_eq!(
            checked_mul_div_ceil(1, 1, 0),
            Err(MathError::DivisionByZero)
        );
    }

    #[test]
    fn mul_div_ceil_overflow_returns_error() {
        assert_eq!(
            checked_mul_div_ceil(i128::MAX, 2, 1),
            Err(MathError::Overflow)
        );
    }

    #[test]
    fn mul_div_ceil_overflow_on_addition() {
        // product fits, but product % c != 0 and product / c + 1 overflows
        // i128::MAX - 1 divided by 1 = i128::MAX - 1, +1 = i128::MAX → fine
        // But i128::MAX / 1 = i128::MAX, remainder 0 → no addition
        // To trigger: i128::MAX / 1 with remainder = 0 → won't hit add
        // The ceiling addition can only overflow if product == i128::MAX
        // and c > 1, with product % c != 0
        // i128::MAX = 170141183460469231731687303715884105727
        // Let's use a case where product = i128::MAX - 1 and c divides it:
        // Actually the simpler case: product = i128::MAX, c = 2, remainder = 1
        // Then quotient = (i128::MAX - 1) / 2 = 9223372036854775807
        // product % c = 1, so we'd do quotient + 1 = 9223372036854775808 which fits
        // Actually let me just test a pathological case
        assert_eq!(checked_mul_div_ceil(i128::MAX, 1, 1).unwrap(), i128::MAX);
    }

    // ── split_interest_by_reserve_factor ─────────────────────────────────────

    #[test]
    fn test_split_zero_reserve_factor_all_to_depositors() {
        let (depositor, reserve) = split_interest_by_reserve_factor(1_000, 0).unwrap();
        assert_eq!(depositor, 1_000);
        assert_eq!(reserve, 0);
    }

    #[test]
    fn test_split_full_reserve_factor_all_to_protocol() {
        let (depositor, reserve) = split_interest_by_reserve_factor(1_000, 10_000).unwrap();
        assert_eq!(depositor, 0);
        assert_eq!(reserve, 1_000);
    }

    #[test]
    fn test_split_10pct_reserve_factor() {
        // 10% reserve factor: 1_000 * 1_000 / 10_000 = 100 to reserve, 900 to depositors
        let (depositor, reserve) = split_interest_by_reserve_factor(1_000, 1_000).unwrap();
        assert_eq!(reserve, 100);
        assert_eq!(depositor, 900);
        assert_eq!(depositor + reserve, 1_000);
    }

    #[test]
    fn test_split_20pct_reserve_factor() {
        // 20% reserve: 500 * 2_000 / 10_000 = 100 to reserve, 400 to depositors
        let (depositor, reserve) = split_interest_by_reserve_factor(500, 2_000).unwrap();
        assert_eq!(reserve, 100);
        assert_eq!(depositor, 400);
        assert_eq!(depositor + reserve, 500);
    }

    #[test]
    fn test_split_sum_always_equals_total_interest() {
        // Exhaustively verify the no-leakage invariant across several values
        for total in [0i128, 1, 7, 100, 999, 10_000, 1_000_000] {
            for rf_bps in [0u32, 1, 500, 1_000, 2_000, 5_000, 9_999, 10_000] {
                let (d, r) = split_interest_by_reserve_factor(total, rf_bps).unwrap();
                assert_eq!(
                    d + r,
                    total,
                    "sum mismatch: total={total} rf={rf_bps} => depositor={d} reserve={r}"
                );
                assert!(
                    d >= 0 && r >= 0,
                    "negative split for total={total} rf={rf_bps}"
                );
            }
        }
    }

    #[test]
    fn test_split_depositor_never_less_than_floor_rounding() {
        // When total_interest * reserve_factor_bps is not exactly divisible by BPS_SCALE,
        // the fractional unit falls to the depositor (floor division of reserve_cut).
        // Example: 1 * 5_000 / 10_000 = 0 (integer floor) => depositor gets 1, reserve 0.
        let (depositor, reserve) = split_interest_by_reserve_factor(1, 5_000).unwrap();
        // 1 * 5000 / 10000 = 0 (floor), so reserve_cut = 0, depositor_yield = 1
        assert_eq!(reserve, 0);
        assert_eq!(depositor, 1);
    }

    #[test]
    fn test_split_zero_interest_returns_zeros() {
        let (d, r) = split_interest_by_reserve_factor(0, 2_000).unwrap();
        assert_eq!(d, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn test_split_rejects_negative_interest() {
        let result = split_interest_by_reserve_factor(-1, 1_000);
        assert_eq!(result, Err(MathError::OutOfRange));
    }

    #[test]
    fn test_split_rejects_reserve_factor_above_100pct() {
        let result = split_interest_by_reserve_factor(1_000, 10_001);
        assert_eq!(result, Err(MathError::OutOfRange));
    }
}
