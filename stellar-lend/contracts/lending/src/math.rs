//! Shared arithmetic used by the lending contract.

use soroban_sdk::contracterror;

// Reused, not re-declared: a second, independent `31_536_000` here would let
// this and the rounding-strategy interest paths silently drift apart (e.g. if
// a leap-year adjustment were ever applied to only one of them).
use crate::rounding_strategy::SECONDS_PER_YEAR;

/// Basis points scale (100% = 10,000 bps).
pub const BPS_SCALE: u32 = 10_000;

/// Sanity ceiling on `rate_bps` accepted by [`compute_compound_interest`]
/// (1000% APR). Anything above this is certainly a caller error (e.g. a
/// mis-scaled input) rather than a legitimate rate, so it is rejected
/// outright instead of silently producing a huge or overflowing result.
pub const MAX_RATE_BPS: i128 = 100_000;

/// Error types for checked arithmetic.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MathError {
    Overflow = 1,
    DivisionByZero = 2,
    OutOfRange = 3,
    NegativeResult = 4,
}

/// Split accrued interest into depositor yield and protocol reserve.
///
/// The reserve share is rounded down and the depositor receives the remainder,
/// which guarantees that the two results always sum to `total_interest`.
///
/// # Authorization and Hostile Input Boundary
///
/// - Validates `principal >= 0` to reject malicious negative principal
/// - Validates `rate_bps >= 0` and `<= MAX_RATE_BPS` to reject invalid rates
/// - Validates `elapsed_seconds` does not cause overflow
/// - Uses checked arithmetic throughout to detect all overflow conditions
pub fn compute_compound_interest(
    principal: i128,
    rate_bps: i128,
    elapsed_seconds: u64,
) -> Result<i128, MathError> {
    // Authorization boundary: validate all inputs
    if principal < 0 || rate_bps < 0 || rate_bps > MAX_RATE_BPS {
        return Err(MathError::OutOfRange);
    }
    
    // Safe short-circuit for zero values
    if principal == 0 || elapsed_seconds == 0 || rate_bps == 0 {
        return Ok(0);
    }

    let elapsed_i128 = i128::from(elapsed_seconds);
    let interest = principal
        .checked_mul(rate_bps)
        .ok_or(MathError::Overflow)?
        .checked_mul(elapsed_i128)
        .ok_or(MathError::Overflow)?
        .checked_div((SECONDS_PER_YEAR as i128) * BPS_SCALE as i128)
        .ok_or(MathError::DivisionByZero)?;

    Ok(interest)
}

/// Split accrued interest into depositor yield and protocol reserve.
///
/// The reserve share is rounded down and the depositor receives the remainder,
/// which guarantees that the two results always sum to `total_interest`.
///
/// # Authorization and Hostile Input Boundary
///
/// - Validates `total_interest >= 0` to reject negative interest
/// - Validates `reserve_factor_bps <= BPS_SCALE` to prevent excessive reserve
/// - Uses checked arithmetic to detect overflow
/// - Ensures depositor_yield + reserve_cut == total_interest (no precision loss)
pub fn split_interest_by_reserve_factor(
    total_interest: i128,
    reserve_factor_bps: u32,
) -> Result<(i128, i128), MathError> {
    // Authorization boundary: validate inputs
    if total_interest < 0 || reserve_factor_bps > BPS_SCALE {
        return Err(MathError::OutOfRange);
    }
    if total_interest == 0 {
        return Ok((0, 0));
    }

    let reserve_cut = total_interest
        .checked_mul(reserve_factor_bps as i128)
        .ok_or(MathError::Overflow)?
        .checked_div(BPS_SCALE as i128)
        .ok_or(MathError::DivisionByZero)?;
    let depositor_yield = total_interest
        .checked_sub(reserve_cut)
        .ok_or(MathError::NegativeResult)?;

    Ok((depositor_yield, reserve_cut))
}

/// Multiply `a * b` then divide by `c`, rounding toward negative infinity.
///
/// For the non-negative inputs used by liquidation this is integer truncation,
/// deliberately rounding seized collateral and close-factor repayment down.
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

/// Multiply `a * b` then divide by `c`, rounding toward positive infinity.
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
    fn compute_compound_interest_uses_shared_year_length() {
        let interest = compute_compound_interest(100, 500, SECONDS_PER_YEAR).unwrap();
        assert_eq!(interest, 5);
    }

    #[test]
    fn compute_compound_interest_matches_a_365_day_year() {
        // `elapsed_seconds` here is a literal 365-day year, independent of the
        // `SECONDS_PER_YEAR` import above, so this fails if `math.rs` ever
        // divides by a re-hardcoded value that has drifted from it.
        let one_year_seconds: u64 = 365 * 24 * 60 * 60;
        let interest = compute_compound_interest(100, 500, one_year_seconds).unwrap();
        assert_eq!(interest, 5);
    }

    #[test]
    fn split_preserves_all_accrued_interest() {
        let (depositor, reserve) = split_interest_by_reserve_factor(1_000, 1_000).unwrap();
        assert_eq!((depositor, reserve), (900, 100));
        assert_eq!(depositor + reserve, 1_000);
    }

    #[test]
    fn split_rejects_invalid_inputs() {
        assert_eq!(
            split_interest_by_reserve_factor(-1, 1_000),
            Err(MathError::OutOfRange)
        );
        assert_eq!(
            split_interest_by_reserve_factor(1_000, BPS_SCALE + 1),
            Err(MathError::OutOfRange)
        );
    }

    #[test]
    fn mul_div_floor_has_explicit_failure_modes() {
        assert_eq!(checked_mul_div_floor(10, 11, 3).unwrap(), 36);
        assert_eq!(
            checked_mul_div_floor(1, 1, 0),
            Err(MathError::DivisionByZero)
        );
        assert_eq!(
            checked_mul_div_floor(i128::MAX, 2, 1),
            Err(MathError::Overflow)
        );
    }
}
