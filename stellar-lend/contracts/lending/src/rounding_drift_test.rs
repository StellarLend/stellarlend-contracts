#[cfg(test)]
mod rounding_drift_tests {
    use crate::debt::accrue_interest;
    use crate::rounding_strategy::{
        calculate_interest_with_rounding, InterestCalcResult, RoundingMode, BASIS_POINTS_SCALE,
        INTEREST_PRECISION, SECONDS_PER_YEAR,
    };

    fn denominator() -> i128 {
        (SECONDS_PER_YEAR as i128) * BASIS_POINTS_SCALE
    }

    fn rounded_micro_units(result: &InterestCalcResult) -> i128 {
        result.interest * INTEREST_PRECISION + result.remainder
    }

    fn exact_micro_numerator(principal: i128, elapsed_seconds: u64, rate_bps: i128) -> i128 {
        principal * (elapsed_seconds as i128) * rate_bps * INTEREST_PRECISION
    }

    fn bankers_round_div(numerator: i128, divisor: i128) -> i128 {
        let quotient = numerator / divisor;
        let remainder = numerator % divisor;
        let half_divisor = divisor / 2;

        if remainder < half_divisor {
            quotient
        } else if remainder > half_divisor {
            quotient + 1
        } else if quotient % 2 == 0 {
            quotient
        } else {
            quotient + 1
        }
    }

    /// Pins the public direction of every rounding mode on fractional accruals.
    #[test]
    fn test_rounding_modes_pin_direction_with_non_zero_remainder() {
        let below_half = (1i128, 1u64, 1i128);
        let almost_one_unit = (1i128, SECONDS_PER_YEAR - 1, BASIS_POINTS_SCALE);

        let below_truncate = calculate_interest_with_rounding(
            below_half.0,
            below_half.1,
            below_half.2,
            RoundingMode::Truncate,
        )
        .unwrap();
        let below_floor = calculate_interest_with_rounding(
            below_half.0,
            below_half.1,
            below_half.2,
            RoundingMode::Floor,
        )
        .unwrap();
        let below_bankers = calculate_interest_with_rounding(
            below_half.0,
            below_half.1,
            below_half.2,
            RoundingMode::Bankers,
        )
        .unwrap();
        let below_ceil = calculate_interest_with_rounding(
            below_half.0,
            below_half.1,
            below_half.2,
            RoundingMode::Ceil,
        )
        .unwrap();

        assert_eq!(rounded_micro_units(&below_truncate), 0);
        assert_eq!(rounded_micro_units(&below_floor), 0);
        assert_eq!(rounded_micro_units(&below_bankers), 0);
        assert_eq!(rounded_micro_units(&below_ceil), 1);

        let high_truncate = calculate_interest_with_rounding(
            almost_one_unit.0,
            almost_one_unit.1,
            almost_one_unit.2,
            RoundingMode::Truncate,
        )
        .unwrap();
        let high_floor = calculate_interest_with_rounding(
            almost_one_unit.0,
            almost_one_unit.1,
            almost_one_unit.2,
            RoundingMode::Floor,
        )
        .unwrap();
        let high_bankers = calculate_interest_with_rounding(
            almost_one_unit.0,
            almost_one_unit.1,
            almost_one_unit.2,
            RoundingMode::Bankers,
        )
        .unwrap();
        let high_ceil = calculate_interest_with_rounding(
            almost_one_unit.0,
            almost_one_unit.1,
            almost_one_unit.2,
            RoundingMode::Ceil,
        )
        .unwrap();

        assert_eq!(rounded_micro_units(&high_truncate), INTEREST_PRECISION - 1);
        assert_eq!(rounded_micro_units(&high_floor), INTEREST_PRECISION - 1);
        assert_eq!(rounded_micro_units(&high_bankers), INTEREST_PRECISION);
        assert_eq!(rounded_micro_units(&high_ceil), INTEREST_PRECISION);
    }

    /// Covers both exact-half Bankers tie branches through the public result.
    #[test]
    fn test_bankers_exact_half_ties_round_to_even_micro_unit() {
        let even_tie =
            calculate_interest_with_rounding(73, 3_600, 3, RoundingMode::Bankers).unwrap();
        let odd_tie =
            calculate_interest_with_rounding(219, 3_600, 3, RoundingMode::Bankers).unwrap();

        assert_eq!(
            exact_micro_numerator(73, 3_600, 3) % denominator(),
            denominator() / 2
        );
        assert_eq!(
            exact_micro_numerator(219, 3_600, 3) % denominator(),
            denominator() / 2
        );

        assert_eq!(rounded_micro_units(&even_tie), 2);
        assert_eq!(rounded_micro_units(&odd_tie), 8);
    }

    /// Fails if the production debt accrual path stops using Bankers rounding.
    #[test]
    fn test_debt_accrual_uses_bankers_rounding_for_exact_half_case() {
        let principal = 9_275_289_480i128;
        let elapsed_seconds = 1u64;
        let rate_bps = 34i128;

        let truncate = calculate_interest_with_rounding(
            principal,
            elapsed_seconds,
            rate_bps,
            RoundingMode::Truncate,
        )
        .unwrap();

        assert_eq!(truncate.interest, 0);
        assert_eq!(
            exact_micro_numerator(principal, elapsed_seconds, rate_bps) % denominator(),
            denominator() / 2
        );
        assert_eq!(accrue_interest(principal, elapsed_seconds, rate_bps), Ok(1));
    }

    /// Compares cumulative Bankers rounding against an aggregate exact reference.
    #[test]
    fn test_bankers_long_horizon_drift_matches_high_precision_reference() {
        let mut principal = 123_456_789i128;
        let elapsed_seconds = 86_400u64;
        let rate_bps = 537i128;
        let steps = 730i128;

        let mut total_rounded_micro_units = 0i128;
        let mut total_exact_numerator = 0i128;

        for _ in 0..steps {
            total_exact_numerator += exact_micro_numerator(principal, elapsed_seconds, rate_bps);

            let result = calculate_interest_with_rounding(
                principal,
                elapsed_seconds,
                rate_bps,
                RoundingMode::Bankers,
            )
            .unwrap();

            total_rounded_micro_units += rounded_micro_units(&result);
            principal += result.interest;
        }

        let reference_micro_units = bankers_round_div(total_exact_numerator, denominator());
        let drift = (total_rounded_micro_units - reference_micro_units).abs();

        assert!(
            drift <= steps,
            "Bankers drift exceeded one micro-unit per accrual: drift={}, steps={}",
            drift,
            steps
        );
    }
}
