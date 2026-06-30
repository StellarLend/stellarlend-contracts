#[cfg(test)]
mod effective_supply_rate_tests {
    use crate::debt::{effective_supply_rate, DebtError};

    #[test]
    fn zero_utilization_yields_zero_supply_rate() {
        let cases = [
            (0i128, 0u32),
            (100, 0),
            (500, 0),
            (500, 1_000),
            (500, 5_000),
            (500, 10_000),
            (1_700, 0),
            (10_000, 9_999),
        ];
        for (borrow_rate, reserve_factor) in cases {
            let rate = effective_supply_rate(borrow_rate, 0, reserve_factor)
                .expect("should not error on zero utilization");
            assert_eq!(
                rate, 0,
                "supply rate must be 0 when utilization=0 \
                 (borrow_rate={borrow_rate}, rf={reserve_factor})"
            );
        }
    }

    #[test]
    fn supply_rate_never_exceeds_borrow_rate() {
        let borrow_rates = [0i128, 1, 100, 500, 1_700, 3_700, 10_000];
        let utilizations = [0i128, 1, 1_000, 5_000, 8_000, 9_999, 10_000];
        let reserve_factors = [0u32, 1, 500, 1_000, 2_000, 5_000, 9_999, 10_000];

        for &borrow_rate in &borrow_rates {
            for &util in &utilizations {
                for &rf in &reserve_factors {
                    let supply = effective_supply_rate(borrow_rate, util, rf)
                        .expect("valid inputs should not error");
                    assert!(
                        supply <= borrow_rate,
                        "supply {supply} > borrow {borrow_rate} \
                         (util={util}, rf={rf})"
                    );
                }
            }
        }
    }

    #[test]
    fn zero_reserve_factor_yields_at_least_as_much_as_nonzero() {
        let borrow_rate = 1_000i128; // 10% APR
        let utilization = 7_000i128; // 70%

        let rate_zero_rf =
            effective_supply_rate(borrow_rate, utilization, 0).expect("zero rf should not error");

        for nonzero_rf in [1u32, 100, 500, 1_000, 2_000, 5_000, 9_999, 10_000] {
            let rate_nonzero = effective_supply_rate(borrow_rate, utilization, nonzero_rf)
                .expect("nonzero rf should not error");
            assert!(
                rate_zero_rf >= rate_nonzero,
                "zero reserve ({rate_zero_rf}) should yield >= \
                 nonzero reserve rf={nonzero_rf} ({rate_nonzero})"
            );
        }
    }
    #[test]
    fn supply_rate_decreases_as_reserve_factor_increases() {
        let borrow_rate = 900i128;
        let utilization = 6_000i128;

        let ordered_rfs = [0u32, 500, 1_000, 2_000, 5_000, 8_000, 9_999, 10_000];
        let mut prev = i128::MAX;

        for &rf in &ordered_rfs {
            let rate =
                effective_supply_rate(borrow_rate, utilization, rf).expect("should not error");
            assert!(
                rate <= prev,
                "supply rate increased as rf grew: rf={rf} gave {rate} > prev {prev}"
            );
            prev = rate;
        }
    }

    #[test]
    fn zero_reserve_full_utilization_equals_borrow_rate() {
        let borrow_rate = 500i128;
        let rate = effective_supply_rate(borrow_rate, 10_000, 0).expect("should not error");
        assert_eq!(
            rate, borrow_rate,
            "at 100% util with 0% reserve, supply rate must equal borrow rate"
        );
    }

    #[test]
    fn full_reserve_factor_supply_rate_is_zero() {
        for util in [1_000i128, 5_000, 8_000, 10_000] {
            let rate = effective_supply_rate(500, util, 10_000).expect("should not error");
            assert_eq!(
                rate, 0,
                "supply rate must be 0 when reserve=100% (util={util})"
            );
        }
    }

    #[test]
    fn worked_example_half_util_half_reserve() {
        let rate = effective_supply_rate(400, 5_000, 5_000).expect("should not error");
        assert_eq!(rate, 100);
    }

    #[test]
    fn no_panic_on_max_borrow_rate_and_utilization() {
        let result = effective_supply_rate(i128::MAX, i128::MAX, 0);
        match result {
            Ok(v) => assert!(v >= 0, "result must be non-negative"),
            Err(DebtError::Overflow) => { /* expected – overflow detected safely */ }
            Err(e) => panic!("unexpected error variant: {e:?}"),
        }
    }

    #[test]
    fn negative_borrow_rate_returns_error() {
        let result = effective_supply_rate(-1, 5_000, 0);
        assert_eq!(
            result,
            Err(DebtError::Overflow),
            "negative borrow rate must return Overflow error"
        );
    }

    #[test]
    fn negative_utilization_returns_error() {
        let result = effective_supply_rate(500, -1, 0);
        assert_eq!(
            result,
            Err(DebtError::Overflow),
            "negative utilization must return Overflow error"
        );
    }

    #[test]
    fn reserve_factor_above_10000_returns_error() {
        let result = effective_supply_rate(500, 5_000, 10_001);
        assert_eq!(
            result,
            Err(DebtError::Overflow),
            "reserve factor > 10_000 must return Overflow error"
        );
    }

    #[test]
    fn large_valid_inputs_no_panic() {
        let result = effective_supply_rate(10_000, 10_000, 1);
        assert!(result.is_ok(), "large valid inputs should not error");
        assert!(result.unwrap() >= 0);
    }

    // ── Zero borrow rate edge case ────────────────────────────────────────────

    /// A zero borrow rate must always produce a zero supply rate regardless of
    /// utilization or reserve factor.
    #[test]
    fn zero_borrow_rate_always_yields_zero() {
        for util in [0i128, 1_000, 5_000, 10_000] {
            for rf in [0u32, 1_000, 5_000, 10_000] {
                let rate = effective_supply_rate(0, util, rf).expect("should not error");
                assert_eq!(
                    rate, 0,
                    "supply rate must be 0 when borrow_rate=0 (util={util}, rf={rf})"
                );
            }
        }
    }

    // ── Supply rate is non-negative for all valid inputs ──────────────────────

    /// Exhaustive grid: supply rate is always ≥ 0 for all valid input ranges.
    #[test]
    fn supply_rate_always_non_negative() {
        for borrow_rate in [0i128, 50, 500, 1_700, 10_000] {
            for util in [0i128, 1_000, 5_000, 10_000] {
                for rf in [0u32, 500, 1_000, 5_000, 10_000] {
                    let rate = effective_supply_rate(borrow_rate, util, rf)
                        .expect("valid inputs must not error");
                    assert!(
                        rate >= 0,
                        "negative supply rate: borrow={borrow_rate} util={util} rf={rf} => {rate}"
                    );
                }
            }
        }
    }
}
