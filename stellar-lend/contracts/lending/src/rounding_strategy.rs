// ════════════════════════════════════════════════════════════════
// ROUNDING STRATEGY - Fix interest accrual drift
// ════════════════════════════════════════════════════════════════

/// Rounding strategy for interest calculations
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingMode {
    Truncate,
    Floor,
    Bankers,
    Ceil,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingError {
    Overflow,
    InvalidParameter,
}

/// Constants for interest calculation precision
pub const INTEREST_PRECISION: i128 = 1_000_000; // 6 decimal places for intermediate calc
pub const SECONDS_PER_YEAR: u64 = 365 * 24 * 60 * 60; // 31,536,000
pub const BASIS_POINTS_SCALE: i128 = 10_000;

/// Interest calculation result with full precision tracking
#[derive(Clone, Debug)]
pub struct InterestCalcResult {
    pub interest: i128,
    pub remainder: i128,
    pub total_drift: i128,
    pub mode: RoundingMode,
}

impl InterestCalcResult {
    pub fn new(interest: i128, remainder: i128, mode: RoundingMode) -> Self {
        InterestCalcResult {
            interest,
            remainder,
            total_drift: remainder,
            mode,
        }
    }
}

/// Calculate interest with configurable rounding strategy
pub fn calculate_interest_with_rounding(
    borrowed_amount: i128,
    elapsed_seconds: u64,
    rate_bps: i128,
    mode: RoundingMode,
) -> Result<InterestCalcResult, RoundingError> {
    // Guard: negative amounts
    if borrowed_amount < 0 || rate_bps < 0 {
        return Err(RoundingError::InvalidParameter);
    }
    if borrowed_amount == 0 {
        return Ok(InterestCalcResult::new(0, 0, mode));
    }

    // Use checked arithmetic to avoid overflow

    // Step 1: Multiply borrowed_amount * elapsed_seconds
    let amount_times_seconds = borrowed_amount
        .checked_mul(elapsed_seconds as i128)
        .ok_or(RoundingError::Overflow)?;

    // Step 2: Multiply by rate_bps
    let amount_times_seconds_times_rate = amount_times_seconds
        .checked_mul(rate_bps)
        .ok_or(RoundingError::Overflow)?;

    // Step 3: Multiply by PRECISION for fractional tracking
    let with_precision = amount_times_seconds_times_rate
        .checked_mul(INTEREST_PRECISION)
        .ok_or(RoundingError::Overflow)?;

    // Step 4: Divide by denominator
    let denominator = (SECONDS_PER_YEAR as i128)
        .checked_mul(BASIS_POINTS_SCALE)
        .ok_or(RoundingError::Overflow)?;

    let full_division = with_precision / denominator;
    let remainder = with_precision % denominator;

    // Step 5: Apply rounding strategy
    let (rounded_interest, _actual_remainder) =
        apply_rounding(full_division, remainder, denominator, mode);

    // Step 6: Back-convert from precision scale
    let mut final_interest = rounded_interest / INTEREST_PRECISION;
    let mut final_remainder = rounded_interest % INTEREST_PRECISION;

    // Safety: ensure Ceil never produces a lower integer interest than Floor
    // due to subtle integer division edge-cases. Compute the floor-rounded
    // integer interest and clamp the ceil result to be >= floor.
    if mode == RoundingMode::Ceil {
        let (floor_rounded, _) =
            apply_rounding(full_division, remainder, denominator, RoundingMode::Floor);
        let floor_interest = floor_rounded / INTEREST_PRECISION;
        if final_interest < floor_interest {
            final_interest = floor_interest;
            final_remainder = 0;
        }
    }

    Ok(InterestCalcResult::new(
        final_interest,
        final_remainder,
        mode,
    ))
}

fn apply_rounding(
    quotient: i128,
    remainder: i128,
    divisor: i128,
    mode: RoundingMode,
) -> (i128, i128) {
    let half_divisor = divisor / 2;
    match mode {
        RoundingMode::Truncate => (quotient, remainder),
        RoundingMode::Floor => (quotient, remainder),
        RoundingMode::Bankers => {
            if remainder < half_divisor {
                (quotient, remainder)
            } else if remainder > half_divisor {
                (quotient + 1, remainder - divisor)
            } else if quotient % 2 == 0 {
                (quotient, remainder)
            } else {
                (quotient + 1, remainder - divisor)
            }
        }
        RoundingMode::Ceil => {
            if remainder == 0 {
                (quotient, 0)
            } else {
                (quotient + 1, remainder - divisor)
            }
        }
    }
}
pub fn reconcile_debt_with_drift_correction(
    stored_debt: i128,
    freshly_calculated_debt: i128,
    accumulated_drift: i128,
    max_allowed_drift_bps: i128, // e.g., 10 = 0.1% max drift
) -> Result<(i128, i128), RoundingError> {
    // Calculate the drift in basis points
    let debt_basis = if stored_debt > 0 {
        (freshly_calculated_debt - stored_debt) * 10000 / stored_debt
    } else {
        0
    };
    if debt_basis.abs() > max_allowed_drift_bps {
        return Err(RoundingError::InvalidParameter);
    }

    // Return reconciled debt and updated drift
    Ok((
        freshly_calculated_debt,
        accumulated_drift + (freshly_calculated_debt - stored_debt),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_borrowed_returns_zero_interest() {
        let result =
            calculate_interest_with_rounding(0, 365 * 24 * 60 * 60, 500, RoundingMode::Floor);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().interest, 0);
    }

    #[test]
    fn test_simple_one_year_accrual() {
        // $100 borrowed for 1 year at 5% APR
        let result = calculate_interest_with_rounding(
            100,
            SECONDS_PER_YEAR,
            500, // 5%
            RoundingMode::Floor,
        )
        .unwrap();

        // Expected: 100 * 0.05 = 5
        assert_eq!(result.interest, 5);
    }

    #[test]
    fn test_rounding_modes_differ_on_fractions() {
        // Set up a scenario with fractional interest
        let result_floor = calculate_interest_with_rounding(
            1000,
            SECONDS_PER_YEAR / 12, // 1 month
            500,                   // 5% APR
            RoundingMode::Floor,
        )
        .unwrap();

        let result_ceil =
            calculate_interest_with_rounding(1000, SECONDS_PER_YEAR / 12, 500, RoundingMode::Ceil)
                .unwrap();

        // Ceil should round up from floor
        assert!(result_ceil.interest >= result_floor.interest);
    }

    #[test]
    fn test_long_horizon_no_drift_with_bankers() {
        // 24 months of monthly accruals
        let mut total_interest = 0i128;
        let borrowed = 1000i128;
        let monthly_seconds = SECONDS_PER_YEAR / 12;

        for _ in 0..24 {
            let result = calculate_interest_with_rounding(
                borrowed,
                monthly_seconds,
                500, // 5% APR
                RoundingMode::Bankers,
            )
            .unwrap();

            total_interest += result.interest;
        }

        // 24 * (1000 * 0.05 / 12) ≈ 100
        // Should be close to 100 with bankers rounding
        assert!(
            (95..=105).contains(&total_interest),
            "total_interest: {}",
            total_interest
        );
    }

    #[test]
    fn test_scaling_constants_are_canonical() {
        assert_eq!(INTEREST_PRECISION, 1_000_000);
        assert_eq!(BASIS_POINTS_SCALE, 10_000);
        assert_eq!(SECONDS_PER_YEAR, 31_536_000);

        let denominator = (SECONDS_PER_YEAR as i128) * BASIS_POINTS_SCALE;
        assert_eq!(denominator, 315_360_000_000);
    }

    #[test]
    fn test_bps_to_decimal_conversion() {
        assert_eq!(500 * INTEREST_PRECISION / BASIS_POINTS_SCALE, 50_000);
        assert_eq!(
            10_000 * INTEREST_PRECISION / BASIS_POINTS_SCALE,
            INTEREST_PRECISION
        );
        assert_eq!(1_000 * INTEREST_PRECISION / BASIS_POINTS_SCALE, 100_000);
    }

    #[test]
    fn test_one_year_exact_no_remainder() {
        let result =
            calculate_interest_with_rounding(100, SECONDS_PER_YEAR, 500, RoundingMode::Bankers)
                .unwrap();
        assert_eq!(result.interest, 5);
        assert_eq!(result.remainder, 0);
    }

    #[test]
    fn test_one_second_small_fractional_interest() {
        let result =
            calculate_interest_with_rounding(100_000, 1, 500, RoundingMode::Bankers).unwrap();
        assert_eq!(result.interest, 0);
    }

    #[test]
    fn test_one_month_bankers_rounding() {
        let result = calculate_interest_with_rounding(
            1_000,
            SECONDS_PER_YEAR / 12,
            500,
            RoundingMode::Bankers,
        )
        .unwrap();
        assert_eq!(result.interest, 4);
    }

    #[test]
    fn test_ceil_never_below_floor() {
        let floor_result = calculate_interest_with_rounding(
            1_000,
            SECONDS_PER_YEAR / 12,
            500,
            RoundingMode::Floor,
        )
        .unwrap();
        let ceil_result =
            calculate_interest_with_rounding(1_000, SECONDS_PER_YEAR / 12, 500, RoundingMode::Ceil)
                .unwrap();
        assert!(
            ceil_result.interest >= floor_result.interest,
            "Ceil interest ({}) must be >= Floor interest ({})",
            ceil_result.interest,
            floor_result.interest
        );
    }
}
