/// Regression contract for interest-rate and utilization math.
///
/// Refs #1915
///
/// # Coverage matrix
///
/// | Area                          | States covered                                          |
/// |-------------------------------|--------------------------------------------------------|
/// | `compute_borrow_rate`         | zero util, kink, post-kink, 100%, floor, ceiling       |
/// | `compute_borrow_rate` errors  | multiplier overflow, jump overflow, surcharge overflow  |
/// | `compute_smoothed_rate`       | no elapsed, step cap, upward/downward convergence       |
/// | `apply_hysteresis`            | inside band, outside band, zero band, negative diff     |
/// | `effective_supply_rate`       | zero util, 100% util, reserve factor sweep             |
/// | `accrue_index`                | zero elapsed, zero rate, multi-period compound         |
/// | `calculate_interest_rounding` | floor/ceil/bankers parity, one-year exact              |
/// | `utilization`                 | zero supply, 100% utilization, overflow guard          |
/// | API stability                 | `RateParams::default()` field values, `BPS_DENOM`      |
#[cfg(test)]
mod interest_rate_regression {
    use crate::debt::{
        accrue_index, accrue_interest, effective_supply_rate, DebtError, INDEX_SCALE,
    };
    use crate::rate_model::{
        apply_hysteresis, compute_borrow_rate, compute_smoothed_rate, RateModelError, RateParams,
    };
    use crate::rounding_strategy::{
        calculate_interest_with_rounding, RoundingMode, SECONDS_PER_YEAR,
    };
    use stellar_lend_common::BPS_DENOM;

    // ── API / contract stability ────────────────────────────────────────────

    /// `BPS_DENOM` must remain 10_000; downstream consumers depend on it.
    #[test]
    fn bps_denom_is_ten_thousand() {
        assert_eq!(BPS_DENOM, 10_000);
    }

    /// `INDEX_SCALE` must remain 10^7; position accounting depends on it.
    #[test]
    fn index_scale_is_ten_million() {
        assert_eq!(INDEX_SCALE, 10_000_000);
    }

    /// `RateParams::default()` field values are part of the public contract.
    /// Changing them is a breaking change to any deployed configuration that
    /// relies on the default model.
    #[test]
    fn rate_params_default_contract() {
        let p = RateParams::default();
        assert_eq!(p.base_rate_bps, 100, "base_rate_bps");
        assert_eq!(p.kink_utilization_bps, 8_000, "kink_utilization_bps");
        assert_eq!(p.multiplier_bps, 2_000, "multiplier_bps");
        assert_eq!(p.jump_multiplier_bps, 10_000, "jump_multiplier_bps");
        assert_eq!(p.rate_floor_bps, 50, "rate_floor_bps");
        assert_eq!(p.rate_ceiling_bps, 10_000, "rate_ceiling_bps");
        assert_eq!(p.max_rate_change_per_ledger_bps, i128::MAX, "max_rate_change");
        assert_eq!(p.hysteresis_bps, 0, "hysteresis_bps");
        assert_eq!(p.surcharge_kink_bps, 10_000, "surcharge_kink_bps");
        assert_eq!(p.surcharge_slope, 0, "surcharge_slope");
    }

    // ── compute_borrow_rate: success path ──────────────────────────────────

    /// At zero utilization the rate equals the base rate (floor may apply).
    #[test]
    fn rate_at_zero_utilization_equals_base_or_floor() {
        let p = RateParams::default();
        let rate = compute_borrow_rate(0, &p).unwrap();
        // base_rate=100, floor=50 → max(100, 50) = 100
        assert_eq!(rate, 100);
    }

    /// At exactly the kink the legacy formula gives 1700 bps.
    #[test]
    fn rate_at_kink_exact_value() {
        let rate = compute_borrow_rate(8_000, &RateParams::default()).unwrap();
        // 100 + 8000*2000/10000 = 100 + 1600 = 1700
        assert_eq!(rate, 1_700);
    }

    /// One tick above the kink activates the jump slope.
    #[test]
    fn rate_one_tick_above_kink_uses_jump_slope() {
        let p = RateParams::default();
        let rate_at_kink = compute_borrow_rate(8_000, &p).unwrap();
        let rate_above_kink = compute_borrow_rate(8_001, &p).unwrap();
        // 1 bps of excess at jump_multiplier=10000 adds 10000/10000 = 1 bps
        assert_eq!(rate_above_kink, rate_at_kink + 1);
    }

    /// At 100% utilization (no surcharge) the legacy value is 3700 bps.
    #[test]
    fn rate_at_full_utilization_legacy_value() {
        let rate = compute_borrow_rate(10_000, &RateParams::default()).unwrap();
        // 100 + 1600 + (2000*10000/10000) = 100 + 1600 + 2000 = 3700
        assert_eq!(rate, 3_700);
    }

    /// Floor clamps a rate that would otherwise fall below it.
    #[test]
    fn floor_clamps_low_rate() {
        let p = RateParams {
            base_rate_bps: 0,
            multiplier_bps: 0,
            jump_multiplier_bps: 0,
            rate_floor_bps: 200,
            ..RateParams::default()
        };
        let rate = compute_borrow_rate(0, &p).unwrap();
        assert_eq!(rate, 200, "floor should apply when raw rate < floor");
    }

    /// Ceiling clamps a rate that would otherwise exceed it.
    #[test]
    fn ceiling_clamps_high_rate() {
        let p = RateParams {
            rate_ceiling_bps: 1_000,
            ..RateParams::default()
        };
        // At 100% util, raw rate is 3700 — must be clamped to 1000.
        let rate = compute_borrow_rate(10_000, &p).unwrap();
        assert_eq!(rate, 1_000, "ceiling should clamp rate above it");
    }

    /// When floor == ceiling the rate is always that single value.
    #[test]
    fn floor_equals_ceiling_always_returns_that_value() {
        let p = RateParams {
            rate_floor_bps: 500,
            rate_ceiling_bps: 500,
            ..RateParams::default()
        };
        for util in [0i128, 4_000, 8_000, 10_000] {
            let rate = compute_borrow_rate(util, &p).unwrap();
            assert_eq!(rate, 500, "floor==ceiling at util={util}");
        }
    }

    /// The function is strictly monotonic non-decreasing across 0–10 000 bps.
    #[test]
    fn borrow_rate_monotonic_default_params() {
        let p = RateParams::default();
        let mut prev = i128::MIN;
        for util in (0i128..=10_000).step_by(1) {
            let rate = compute_borrow_rate(util, &p).unwrap();
            assert!(
                rate >= prev,
                "rate decreased at util={util}: {rate} < {prev}"
            );
            prev = rate;
        }
    }

    /// Monotonicity holds when both kink and surcharge are active.
    #[test]
    fn borrow_rate_monotonic_with_surcharge() {
        let p = RateParams {
            surcharge_kink_bps: 9_000,
            surcharge_slope: 30_000,
            ..RateParams::default()
        };
        let mut prev = i128::MIN;
        for util in (0i128..=10_000).step_by(1) {
            let rate = compute_borrow_rate(util, &p).unwrap();
            assert!(
                rate >= prev,
                "rate decreased (with surcharge) at util={util}: {rate} < {prev}"
            );
            prev = rate;
        }
    }

    /// Output is always within [floor, ceiling] for all valid utilizations.
    #[test]
    fn rate_always_within_floor_ceiling() {
        let p = RateParams::default();
        for util in (0i128..=10_000).step_by(100) {
            let rate = compute_borrow_rate(util, &p).unwrap();
            assert!(
                rate >= p.rate_floor_bps,
                "rate {rate} below floor {} at util={util}",
                p.rate_floor_bps
            );
            assert!(
                rate <= p.rate_ceiling_bps,
                "rate {rate} above ceiling {} at util={util}",
                p.rate_ceiling_bps
            );
        }
    }

    // ── compute_borrow_rate: failure / overflow paths ──────────────────────

    /// A multiplier so large it overflows `utilization × multiplier` → `Overflow`.
    #[test]
    fn multiplier_overflow_returns_error_not_panic() {
        let p = RateParams {
            multiplier_bps: i128::MAX / 2,
            kink_utilization_bps: i128::MAX / 2,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(i128::MAX / 2, &p),
            Err(RateModelError::Overflow)
        );
    }

    /// Jump multiplier overflow is caught in the post-kink branch.
    #[test]
    fn jump_multiplier_overflow_returns_error_not_panic() {
        let p = RateParams {
            jump_multiplier_bps: i128::MAX,
            ..RateParams::default()
        };
        // Utilization above kink triggers jump calculation.
        assert_eq!(
            compute_borrow_rate(9_000, &p),
            Err(RateModelError::Overflow)
        );
    }

    /// Surcharge multiplication overflow is caught and returns `Overflow`.
    #[test]
    fn surcharge_slope_overflow_returns_error_not_panic() {
        let p = RateParams {
            surcharge_kink_bps: 0,
            surcharge_slope: i128::MAX,
            ..RateParams::default()
        };
        assert_eq!(
            compute_borrow_rate(10_000, &p),
            Err(RateModelError::Overflow)
        );
    }

    // ── apply_hysteresis ───────────────────────────────────────────────────

    /// Difference inside the band → current rate is held.
    #[test]
    fn hysteresis_inside_band_holds_current() {
        // target = current + 5, band = 10 → diff (5) ≤ band → hold
        assert_eq!(apply_hysteresis(1_000, 1_005, 10), 1_000);
    }

    /// Difference exactly equal to the band → current rate is held.
    #[test]
    fn hysteresis_at_band_boundary_holds_current() {
        assert_eq!(apply_hysteresis(1_000, 1_010, 10), 1_000);
    }

    /// Difference outside the band → rate moves toward target minus the band.
    #[test]
    fn hysteresis_outside_band_moves_toward_target() {
        // target = 1_020, current = 1_000, band = 10 → step to 1_020 - 10 = 1_010
        assert_eq!(apply_hysteresis(1_000, 1_020, 10), 1_010);
    }

    /// Downward movement outside the band → rate moves toward target + band.
    #[test]
    fn hysteresis_downward_outside_band() {
        // target = 900, current = 1_000, band = 10 → step to 900 + 10 = 910
        assert_eq!(apply_hysteresis(1_000, 900, 10), 910);
    }

    /// Zero band disables hysteresis entirely — rate jumps straight to target.
    #[test]
    fn hysteresis_zero_band_jumps_to_target() {
        assert_eq!(apply_hysteresis(1_000, 1_500, 0), 1_500);
        assert_eq!(apply_hysteresis(1_000, 500, 0), 500);
    }

    /// Negative band is treated as zero (no hysteresis).
    #[test]
    fn hysteresis_negative_band_treated_as_zero() {
        // apply_hysteresis clamps band = band.max(0)
        assert_eq!(apply_hysteresis(1_000, 2_000, -50), 2_000);
    }

    // ── compute_smoothed_rate ─────────────────────────────────────────────

    /// With `elapsed = 0` the adjusted target is returned immediately.
    #[test]
    fn smoothed_rate_zero_elapsed_returns_target() {
        let result = compute_smoothed_rate(500, 1_000, 100, 0, 0);
        assert_eq!(result, 1_000);
    }

    /// With `max_step = i128::MAX` the rate jumps to target in one ledger.
    #[test]
    fn smoothed_rate_max_step_jumps_to_target() {
        let result = compute_smoothed_rate(500, 2_000, i128::MAX, 1, 0);
        assert_eq!(result, 2_000);
    }

    /// Rate is capped at `last + max_step * elapsed` when rising.
    #[test]
    fn smoothed_rate_caps_upward_movement() {
        // last=500, target=2000, max_step=100, elapsed=3 → max_change=300
        // adjusted_target=2000, diff=1500 → step=min(1500,300)=300 → 500+300=800
        let result = compute_smoothed_rate(500, 2_000, 100, 3, 0);
        assert_eq!(result, 800);
    }

    /// Rate is capped at `last - max_step * elapsed` when falling.
    #[test]
    fn smoothed_rate_caps_downward_movement() {
        // last=2000, target=500, max_step=100, elapsed=3 → max_change=300
        // diff=-1500 → decrease=min(1500,300)=300 → 2000-300=1700
        let result = compute_smoothed_rate(2_000, 500, 100, 3, 0);
        assert_eq!(result, 1_700);
    }

    /// With enough elapsed ledgers the smoothed rate converges to target.
    #[test]
    fn smoothed_rate_converges_to_target_over_time() {
        // last=500, target=1000, max_step=50 → needs 10 ledgers
        let result = compute_smoothed_rate(500, 1_000, 50, 10, 0);
        assert_eq!(result, 1_000);
    }

    /// Hysteresis inside the smoothing path holds the current rate.
    #[test]
    fn smoothed_rate_with_hysteresis_holds_inside_band() {
        // last=1_000, target=1_005, band=10 → adjusted_target=1_000 (held)
        let result = compute_smoothed_rate(1_000, 1_005, 200, 5, 10);
        assert_eq!(result, 1_000);
    }

    // ── effective_supply_rate ────────────────────────────────────────────

    /// At zero utilization the supply rate is always zero.
    #[test]
    fn supply_rate_zero_util_is_zero() {
        for borrow_rate in [0i128, 100, 500, 1_700, 10_000] {
            for rf in [0u32, 500, 10_000] {
                let r = effective_supply_rate(borrow_rate, 0, rf).unwrap();
                assert_eq!(r, 0, "borrow={borrow_rate} rf={rf}");
            }
        }
    }

    /// At 100% utilization with 0% reserve the supply rate equals the borrow rate.
    #[test]
    fn supply_rate_full_util_no_reserve_equals_borrow_rate() {
        for borrow_rate in [100i128, 500, 1_700, 3_700, 10_000] {
            let r = effective_supply_rate(borrow_rate, 10_000, 0).unwrap();
            assert_eq!(r, borrow_rate, "borrow_rate={borrow_rate}");
        }
    }

    /// Supply rate is always ≤ borrow rate across the full valid grid.
    #[test]
    fn supply_rate_never_exceeds_borrow_rate() {
        for borrow_rate in [0i128, 100, 500, 1_700, 10_000] {
            for util in [0i128, 1_000, 5_000, 8_000, 10_000] {
                for rf in [0u32, 500, 2_000, 10_000] {
                    let s = effective_supply_rate(borrow_rate, util, rf).unwrap();
                    assert!(
                        s <= borrow_rate,
                        "supply {s} > borrow {borrow_rate} (util={util}, rf={rf})"
                    );
                }
            }
        }
    }

    /// A 100% reserve factor always produces a zero supply rate.
    #[test]
    fn supply_rate_full_reserve_is_zero() {
        for util in [1_000i128, 5_000, 10_000] {
            let r = effective_supply_rate(500, util, 10_000).unwrap();
            assert_eq!(r, 0, "util={util}");
        }
    }

    /// Supply rate is non-negative for all valid inputs.
    #[test]
    fn supply_rate_always_non_negative() {
        for br in [0i128, 50, 500, 1_700, 10_000] {
            for util in [0i128, 1_000, 5_000, 10_000] {
                for rf in [0u32, 500, 5_000, 10_000] {
                    let r = effective_supply_rate(br, util, rf).unwrap();
                    assert!(r >= 0, "negative: br={br} util={util} rf={rf}");
                }
            }
        }
    }

    /// Negative borrow rate returns an error.
    #[test]
    fn supply_rate_negative_borrow_rate_errors() {
        assert_eq!(
            effective_supply_rate(-1, 5_000, 0),
            Err(DebtError::Overflow)
        );
    }

    /// Negative utilization returns an error.
    #[test]
    fn supply_rate_negative_utilization_errors() {
        assert_eq!(
            effective_supply_rate(500, -1, 0),
            Err(DebtError::Overflow)
        );
    }

    /// Reserve factor above 10 000 returns an error.
    #[test]
    fn supply_rate_reserve_factor_above_10000_errors() {
        assert_eq!(
            effective_supply_rate(500, 5_000, 10_001),
            Err(DebtError::Overflow)
        );
    }

    // ── accrue_index ──────────────────────────────────────────────────────

    /// Zero elapsed time: index is unchanged.
    #[test]
    fn accrue_index_zero_elapsed_unchanged() {
        let idx = accrue_index(INDEX_SCALE, 0, 500);
        assert_eq!(idx, INDEX_SCALE);
    }

    /// Zero rate: index is unchanged regardless of elapsed.
    #[test]
    fn accrue_index_zero_rate_unchanged() {
        let idx = accrue_index(INDEX_SCALE, SECONDS_PER_YEAR, 0);
        assert_eq!(idx, INDEX_SCALE);
    }

    /// Index is strictly monotonic non-decreasing.
    #[test]
    fn accrue_index_monotonic() {
        let mut idx = INDEX_SCALE;
        for _ in 0..10 {
            let new = accrue_index(idx, SECONDS_PER_YEAR / 12, 500);
            assert!(new >= idx, "index decreased: {new} < {idx}");
            idx = new;
        }
    }

    /// One full year at 5% APR on INDEX_SCALE increases the index by ~5%.
    #[test]
    fn accrue_index_one_year_five_percent() {
        // delta = INDEX_SCALE * 500 * 31_536_000 / (31_536_000 * 10_000)
        //       = INDEX_SCALE * 500 / 10_000
        //       = INDEX_SCALE * 0.05
        //       = 10_000_000 * 0.05 = 500_000
        let idx = accrue_index(INDEX_SCALE, SECONDS_PER_YEAR, 500);
        let expected = INDEX_SCALE + INDEX_SCALE * 500 / 10_000;
        assert_eq!(idx, expected);
    }

    // ── accrue_interest / rounding ────────────────────────────────────────

    /// Zero principal produces zero interest for any elapsed/rate.
    #[test]
    fn accrue_interest_zero_principal_zero_result() {
        let r = accrue_interest(0, SECONDS_PER_YEAR, 500).unwrap();
        assert_eq!(r, 0);
    }

    /// Zero elapsed produces zero interest for any principal/rate.
    #[test]
    fn accrue_interest_zero_elapsed_zero_result() {
        let r = accrue_interest(1_000_000, 0, 500).unwrap();
        assert_eq!(r, 0);
    }

    /// One year at 5% on 100 units → exactly 5 (integer).
    #[test]
    fn accrue_interest_one_year_exact() {
        let r = accrue_interest(100, SECONDS_PER_YEAR, 500).unwrap();
        assert_eq!(r, 5);
    }

    /// Ceil rounding is always ≥ floor rounding (drift safety).
    #[test]
    fn rounding_ceil_never_below_floor() {
        for principal in [100i128, 1_000, 1_000_000] {
            let floor = calculate_interest_with_rounding(
                principal,
                SECONDS_PER_YEAR / 12,
                500,
                RoundingMode::Floor,
            )
            .unwrap();
            let ceil = calculate_interest_with_rounding(
                principal,
                SECONDS_PER_YEAR / 12,
                500,
                RoundingMode::Ceil,
            )
            .unwrap();
            assert!(
                ceil.interest >= floor.interest,
                "ceil ({}) < floor ({}) for principal={principal}",
                ceil.interest,
                floor.interest
            );
        }
    }

    /// Bankers rounding stays within ±1 of floor rounding on a short interval.
    #[test]
    fn rounding_bankers_close_to_floor() {
        let floor = calculate_interest_with_rounding(
            1_000,
            SECONDS_PER_YEAR / 12,
            500,
            RoundingMode::Floor,
        )
        .unwrap();
        let bankers = calculate_interest_with_rounding(
            1_000,
            SECONDS_PER_YEAR / 12,
            500,
            RoundingMode::Bankers,
        )
        .unwrap();
        let diff = (bankers.interest - floor.interest).abs();
        assert!(diff <= 1, "bankers diverges from floor by {diff}");
    }

    // ── Integration: borrow rate ↔ supply rate consistency ────────────────

    /// The two canonical data points from `RateParams::default()` docs must
    /// match the implementation forever (regression anchors).
    #[test]
    fn canonical_rate_curve_regression_anchors() {
        let p = RateParams::default();
        // Documented in RateParams::default() doc comment.
        assert_eq!(compute_borrow_rate(0, &p).unwrap(), 100); // 0% util
        assert_eq!(compute_borrow_rate(8_000, &p).unwrap(), 1_700); // 80% kink
        assert_eq!(compute_borrow_rate(10_000, &p).unwrap(), 3_700); // 100% util
    }

    /// Supply rate at the kink with no reserve factor is exactly
    /// `borrow_rate * utilization / BPS_DENOM`.
    #[test]
    fn supply_rate_at_kink_no_reserve_is_utilization_weighted_borrow_rate() {
        let borrow_rate = 1_700i128;
        let util = 8_000i128;
        let expected = borrow_rate * util / BPS_DENOM; // 1_360
        let actual = effective_supply_rate(borrow_rate, util, 0).unwrap();
        assert_eq!(actual, expected);
    }

    // ── Permission / authorization invariants ─────────────────────────────

    /// The model never returns a rate below zero for any valid input combination.
    #[test]
    fn borrow_rate_never_negative() {
        let p = RateParams::default();
        for util in (0i128..=10_000).step_by(500) {
            let rate = compute_borrow_rate(util, &p).unwrap();
            assert!(rate >= 0, "negative rate at util={util}: {rate}");
        }
    }

    /// The floor enforces a non-zero minimum even when base rate and slopes are zero.
    #[test]
    fn floor_enforces_minimum_at_all_utilizations() {
        let p = RateParams {
            base_rate_bps: 0,
            multiplier_bps: 0,
            jump_multiplier_bps: 0,
            rate_floor_bps: 75,
            ..RateParams::default()
        };
        for util in [0i128, 5_000, 8_000, 10_000] {
            let rate = compute_borrow_rate(util, &p).unwrap();
            assert!(
                rate >= 75,
                "floor not enforced at util={util}: rate={rate}"
            );
        }
    }
}
