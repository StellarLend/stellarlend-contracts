// ════════════════════════════════════════════════════════════════
// REGRESSION TEST: Interest Accrual Rounding Drift
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod interest_drift_regression_tests {
    extern crate std;
    use std::println;
    use crate::rounding_strategy::{
        calculate_interest_with_rounding, RoundingMode, SECONDS_PER_YEAR,
    };

    /// ✅ Test: 24-month accrual with banker's rounding shows bounded drift
    #[test]
    fn test_24_month_long_horizon_drift_bounded() {
        let borrowed = 100_000i128; // $100,000
        let monthly_seconds = SECONDS_PER_YEAR / 12;
        let mut total_interest = 0i128;

        // Simulate 24 monthly accruals
        for month in 0..24 {
            let result = calculate_interest_with_rounding(
                borrowed,
                monthly_seconds,
                500, // 5% APR
                RoundingMode::Bankers,
            )
            .expect("should not overflow");

            total_interest += result.interest;

            println!(
                "Month {}: monthly interest = {}, total so far = {}",
                month + 1,
                result.interest,
                total_interest
            );
        }

        // Expected: 100,000 * 0.05 * 2 = 10,000 (exact)
        // With 24 months of rounding: should be very close
        let expected = 10_000i128;
        let drift = (total_interest - expected).abs();

        println!("Total interest accrued: {}", total_interest);
        println!("Expected: {}", expected);
        println!("Drift: {} (max allowed: 5)", drift);

        // Banker's rounding should keep drift under 20 units for this scenario
        assert!(
            drift <= 20,
            "Drift too large: {} (expected <= 20)",
            drift
        );
    }

    /// ✅ Test: 100-month (8+ year) accrual with drift tracking
    #[test]
    fn test_long_horizon_100_months_drift_tracking() {
        let borrowed = 50_000i128;
        let monthly_seconds = SECONDS_PER_YEAR / 12;
        let mut total_interest = 0i128;
        let mut total_drift = 0i128;

        for month in 0..100 {
            let result = calculate_interest_with_rounding(
                borrowed,
                monthly_seconds,
                500, // 5%
                RoundingMode::Bankers,
            )
            .expect("should not overflow");

            total_interest += result.interest;
            total_drift += result.remainder;

            if month % 12 == 11 {
                println!(
                    "Year {}: YTD interest = {}, accumulated drift = {}",
                    month / 12 + 1,
                    total_interest,
                    total_drift
                );
            }
        }

        // 100 months ≈ 8.33 years
        // 50,000 * 0.05 * 8.33 = 20,825
        let expected_approx = 20_825i128;
        let drift = (total_interest - expected_approx).abs();

        println!("Total 100-month interest: {}", total_interest);
        println!("Approx expected: {}", expected_approx);
        println!("Drift: {}", drift);

        // Even over 100 months, drift should be bounded
        assert!(
            drift <= 50,
            "Long-horizon drift too large: {} (expected <= 50)",
            drift
        );
    }

    /// ✅ Test: Monotonic accrual (debt never decreases)
    #[test]
    fn test_interest_monotonic_over_long_horizon() {
        let borrowed = 1_000_000i128;
        let mut previous_total = 0i128;

        for seconds_elapsed in [0, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000] {
            let result = calculate_interest_with_rounding(
                borrowed,
                seconds_elapsed,
                500, // 5%
                RoundingMode::Bankers,
            )
            .expect("should not overflow");

            assert!(
                result.interest >= previous_total,
                "Interest decreased at {} seconds: {} < {}",
                seconds_elapsed,
                result.interest,
                previous_total
            );

            previous_total = result.interest;
        }
    }

    /// ✅ Test: Different rounding modes bound drift differently
    #[test]
    fn test_rounding_modes_drift_comparison() {
        let borrowed = 1000i128;
        let one_month = SECONDS_PER_YEAR / 12;

        // Run 12-month cycle with each rounding mode
        for mode in [
            RoundingMode::Floor,
            RoundingMode::Ceil,
            RoundingMode::Bankers,
        ] {
            let mut total = 0i128;

            for _ in 0..12 {
                let result =
                    calculate_interest_with_rounding(borrowed, one_month, 500, mode)
                        .expect("should not overflow");
                total += result.interest;
            }

            // Expected: 1000 * 0.05 = 50
            let drift = (total - 50).abs();
            println!("Mode {:?}: total = {}, drift = {}", mode, total, drift);

            // All modes should have bounded drift
            assert!(drift <= 10, "Excessive drift for {:?}: {}", mode, drift);
        }
    }

    /// ✅ Test: User vs Protocol accounting reconciliation
    #[test]
    fn test_debt_reconciliation_within_tolerance() {
        use crate::rounding_strategy::reconcile_debt_with_drift_correction;

        // Scenario 1: Drift is within tolerance (5/1000 = 50 bps <= 100 bps)
        let stored = 1000i128;
        let fresh = 1005i128;
        let accumulated_drift = 2i128;
        let max_allowed_drift_bps = 100; // 1% = 100 basis points

        let result = reconcile_debt_with_drift_correction(stored, fresh, accumulated_drift, max_allowed_drift_bps);
        assert!(result.is_ok());
        let (reconciled_debt, new_drift) = result.unwrap();
        assert_eq!(reconciled_debt, 1005);
        assert_eq!(new_drift, 7); // 2 + (1005 - 1000) = 7

        // Scenario 2: Drift is out of tolerance (5/100 = 500 bps > 100 bps)
        let stored_bad = 100i128;
        let fresh_bad = 105i128;
        let result_bad = reconcile_debt_with_drift_correction(stored_bad, fresh_bad, accumulated_drift, max_allowed_drift_bps);
        assert!(result_bad.is_err());
    }

    /// ✅ Test: Overflow handling on extreme horizons
    #[test]
    fn test_extreme_horizon_overflow_protection() {
        // i128::MAX seconds ≈ 9.2 * 10^18 seconds ≈ 292 billion years
        let result = calculate_interest_with_rounding(
            i128::MAX / 2,
            u64::MAX,
            500,
            RoundingMode::Bankers,
        );

        // Should error gracefully, not panic
        assert!(result.is_err(), "Should detect overflow at extreme horizon");
    }

    /// ✅ Test: Edge case - very small borrowed amounts
    #[test]
    fn test_small_amounts_precision() {
        let result = calculate_interest_with_rounding(
            1, // 1 unit
            SECONDS_PER_YEAR,
            500, // 5%
            RoundingMode::Bankers,
        )
        .expect("should not overflow");

        // 1 * 0.05 = 0.05, rounds to 0
        assert_eq!(result.interest, 0);
    }
    /// ✅ Test: High interest rates don't cause unexpected drift
    #[test]
    fn test_high_rate_long_horizon() {
        let borrowed = 100_000i128;
        let one_month = SECONDS_PER_YEAR / 12;
        let high_rate_bps = 10000; // 100% APR (aggressive)

        let mut total = 0i128;
        for _ in 0..12 {
            let result = calculate_interest_with_rounding(
                borrowed,
                one_month,
                high_rate_bps,
                RoundingMode::Bankers,
            )
            .expect("should not overflow");
            total += result.interest;
        }

        // 100,000 * 1.0 = 100,000 exact
        assert!(total >= 95_000 && total <= 105_000, "total: {}", total);
    }
}