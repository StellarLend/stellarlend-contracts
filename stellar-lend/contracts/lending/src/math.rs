//! Pure-math helpers for lending calculations
//!
//! All functions in this module are pure (no Env dependency) and
//! fuzzable. They contain the core arithmetic for interest accrual,
//! health factor computation, and rate model calculations.
//!
//! Extracted from the lending crate for fuzzability.

use soroban_sdk::contracterror;

/// Fixed-point scale for internal calculations (7 decimals)
pub const SCALE: i128 = 1_000_000_0; // 10^7

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
    if rate_bps < 0 || rate_bps > MAX_RATE_BPS {
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
    let step1 = principal
        .checked_mul(rate_bps)
        .ok_or(MathError::Overflow)?;

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
    let weighted_collateral = (collateral_value as i128)
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
                .ok_or(MathError::DivisionByZero)?
        ).ok_or(MathError::Overflow)?
    } else {
        // rate = base + kink * multiplier / SCALE + (util - kink) * jump_multiplier / SCALE
        let base_plus_kink = base.checked_add(
            kink.checked_mul(mult)
                .ok_or(MathError::Overflow)?
                .checked_div(scale)
                .ok_or(MathError::DivisionByZero)?
        ).ok_or(MathError::Overflow)?;

        let excess_util = util.checked_sub(kink)
            .ok_or(MathError::NegativeResult)?;

        base_plus_kink.checked_add(
            excess_util.checked_mul(jump_mult)
                .ok_or(MathError::Overflow)?
                .checked_div(scale)
                .ok_or(MathError::DivisionByZero)?
        ).ok_or(MathError::Overflow)?
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
    let one_minus_reserve = scale.checked_sub(reserve)
        .ok_or(MathError::NegativeResult)?;

    // rate_util * one_minus_reserve / SCALE
    let supply_rate = rate_util
        .checked_mul(one_minus_reserve)
        .ok_or(MathError::Overflow)?
        .checked_div(scale)
        .ok_or(MathError::DivisionByZero)?;

    Ok(supply_rate.min(MAX_RATE_BPS) as u32)
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
pub fn compute_max_borrow(
    collateral_value: i128,
    ltv_bps: u32,
) -> Result<i128, MathError> {
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
pub fn compute_utilization(
    total_borrows: i128,
    total_deposits: i128,
) -> Result<u32, MathError> {
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
}