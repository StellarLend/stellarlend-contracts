// ════════════════════════════════════════════════════════════════
// RUNNABLE EXAMPLE: Interest Scaling Constants and BPS Conversions
//
// This file demonstrates the canonical scaling constants used by
// the StellarLend interest accrual system. Run with:
//
//   cargo test --example scaling_demo
//
// See: stellar-lend/docs/INTEREST_NUMERIC_ASSUMPTIONS.md
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod scaling_demo {
    use stellarlend_lending::rounding_strategy::{
        calculate_interest_with_rounding, RoundingMode, BASIS_POINTS_SCALE, INTEREST_PRECISION,
        SECONDS_PER_YEAR,
    };

    #[test]
    fn demo_canonical_constants() {
        println!("=== StellarLend Interest Scaling Constants ===\n");

        println!("INTEREST_PRECISION = {} (10^6)", INTEREST_PRECISION);
        println!(
            "BASIS_POINTS_SCALE = {} (100% = 10,000 bps)",
            BASIS_POINTS_SCALE
        );
        println!("SECONDS_PER_YEAR   = {} (365 days)\n", SECONDS_PER_YEAR);

        let denominator = (SECONDS_PER_YEAR as i128) * BASIS_POINTS_SCALE;
        println!("Combined denominator = {}", denominator);
        println!("  = SECONDS_PER_YEAR * BASIS_POINTS_SCALE");
        println!("  = {} * {}", SECONDS_PER_YEAR, BASIS_POINTS_SCALE);
        println!("  = {}\n", denominator);

        assert_eq!(INTEREST_PRECISION, 1_000_000);
        assert_eq!(BASIS_POINTS_SCALE, 10_000);
        assert_eq!(SECONDS_PER_YEAR, 31_536_000);
        assert_eq!(denominator, 315_360_000_000);
    }

    #[test]
    fn demo_bps_conversions() {
        println!("=== Basis Points Conversions ===\n");

        let conversions = [
            (10_000, "100%"),
            (5_000, "50%"),
            (1_000, "10%"),
            (500, "5%"),
            (100, "1%"),
        ];

        for (bps, label) in conversions {
            let decimal = bps * INTEREST_PRECISION / BASIS_POINTS_SCALE;
            println!("{} bps = {} = {} (in precision scale)", bps, label, decimal);
        }
        println!();

        assert_eq!(500 * INTEREST_PRECISION / BASIS_POINTS_SCALE, 50_000);
        assert_eq!(
            10_000 * INTEREST_PRECISION / BASIS_POINTS_SCALE,
            INTEREST_PRECISION
        );
    }

    #[test]
    fn demo_interest_calculation_1_year() {
        println!("=== Interest Calculation: $100 at 5% APR for 1 Year ===\n");

        let principal = 100i128;
        let rate_bps = 500i128;
        let elapsed = SECONDS_PER_YEAR;

        let result =
            calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Bankers)
                .unwrap();

        println!("Principal:       {}", principal);
        println!("Rate:            {} bps (5%)", rate_bps);
        println!("Elapsed:         {} seconds (1 year)", elapsed);
        println!("Interest:        {}", result.interest);
        println!("Remainder:       {}", result.remainder);
        println!("Rounding mode:   Bankers\n");

        assert_eq!(result.interest, 5);
        assert_eq!(result.remainder, 0);
    }

    #[test]
    fn demo_interest_calculation_1_second() {
        println!("=== Interest Calculation: $100,000 at 5% APR for 1 Second ===\n");

        let principal = 100_000i128;
        let rate_bps = 500i128;
        let elapsed = 1u64;

        let result =
            calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Bankers)
                .unwrap();

        println!("Principal:       {}", principal);
        println!("Rate:            {} bps (5%)", rate_bps);
        println!("Elapsed:         {} second", elapsed);
        println!(
            "Interest:        {} (rounds to 0 at token-unit level)",
            result.interest
        );
        println!(
            "Remainder:       {} (tracked for drift analysis)",
            result.remainder
        );
        println!("Rounding mode:   Bankers\n");

        assert_eq!(result.interest, 0);
    }

    #[test]
    fn demo_interest_calculation_1_month() {
        println!("=== Interest Calculation: $1,000 at 5% APR for 1 Month ===\n");

        let principal = 1_000i128;
        let rate_bps = 500i128;
        let elapsed = SECONDS_PER_YEAR / 12;

        let result =
            calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Bankers)
                .unwrap();

        println!("Principal:       {}", principal);
        println!("Rate:            {} bps (5%)", rate_bps);
        println!("Elapsed:         {} seconds (~1 month)", elapsed);
        println!(
            "Interest:        {} (exact: $4.167, truncated to {})",
            result.interest, result.interest
        );
        println!("Remainder:       {}", result.remainder);
        println!("Rounding mode:   Bankers\n");

        assert_eq!(result.interest, 4);
    }

    #[test]
    fn demo_rounding_modes_comparison() {
        println!("=== Rounding Modes Comparison: $1,000 at 5% for 1 Month ===\n");

        let principal = 1_000i128;
        let rate_bps = 500i128;
        let elapsed = SECONDS_PER_YEAR / 12;

        for mode in [
            RoundingMode::Floor,
            RoundingMode::Ceil,
            RoundingMode::Bankers,
        ] {
            let result =
                calculate_interest_with_rounding(principal, elapsed, rate_bps, mode).unwrap();

            println!(
                "{:<12} interest={}, remainder={}",
                format!("{:?}", mode),
                result.interest,
                result.remainder
            );
        }
        println!();
    }
}
