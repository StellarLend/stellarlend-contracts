/// Tests for the emergency rate surcharge band in [`compute_borrow_rate`].
///
/// The surcharge band is an optional linear penalty activated only when
/// utilization exceeds `surcharge_kink_bps`.  All tests use checked arithmetic
/// and verify monotonic non-decreasing behaviour.
///
/// See [`RATE_SURCHARGE.md`](../RATE_SURCHARGE.md) for the full specification.
use crate::rate_model::{compute_borrow_rate, RateModelError, RateParams};
use stellar_lend_common::BPS_DENOM;

/// Default params produce the legacy two-slope curve (surcharge disabled).
#[test]
fn test_surcharge_disabled_default_params() {
    let params = RateParams::default();
    // Default surcharge_kink_bps = 10_000, surcharge_slope = 0 → no surcharge.
    assert_eq!(params.surcharge_slope, 0);

    // Same expected values as the legacy model:
    // 0% util → base_rate = 100 bps
    assert_eq!(compute_borrow_rate(0, &params).unwrap(), 100);
    // 80% util (kink) → 100 + 8000 * 2000 / 10000 = 1700
    assert_eq!(compute_borrow_rate(8_000, &params).unwrap(), 1_700);
    // 100% util → 1700 + 2000 * 10000 / 10000 = 3700
    assert_eq!(compute_borrow_rate(10_000, &params).unwrap(), 3_700);
}

/// Surcharge is a no-op when slope is zero, regardless of kink position.
#[test]
fn test_surcharge_zero_slope_noop() {
    let params = RateParams {
        surcharge_slope: 0,
        surcharge_kink_bps: 5_000, // kink at 50% — but slope=0 so no effect
        ..RateParams::default()
    };
    // 100% util → still 3700 (no surcharge added)
    assert_eq!(compute_borrow_rate(10_000, &params).unwrap(), 3_700);
}

/// When utilization is below the surcharge kink, no surcharge is added.
#[test]
fn test_below_surcharge_kink() {
    let params = RateParams {
        surcharge_kink_bps: 9_000, // 90%
        surcharge_slope: 50_000,
        ..RateParams::default()
    };
    // At 89% util → below surcharge kink, same as without surcharge
    let rate = compute_borrow_rate(8_900, &params).unwrap();
    let expected_no_surcharge = compute_borrow_rate(8_900, &RateParams::default()).unwrap();
    assert_eq!(rate, expected_no_surcharge);
}

/// At the surcharge kink, no surcharge is added (exactly at boundary).
#[test]
fn test_at_surcharge_kink() {
    let params = RateParams {
        surcharge_kink_bps: 9_000,
        surcharge_slope: 50_000,
        ..RateParams::default()
    };
    let rate_at_kink = compute_borrow_rate(9_000, &params).unwrap();
    let expected_no_surcharge = compute_borrow_rate(9_000, &RateParams::default()).unwrap();
    assert_eq!(rate_at_kink, expected_no_surcharge);
}

/// Above the surcharge kink, the surcharge is added linearly.
#[test]
fn test_above_surcharge_kink() {
    // 95% surcharge kink, slope = 80_000 bps (800% — steep)
    let params = RateParams {
        surcharge_kink_bps: 9_500,
        surcharge_slope: 80_000,
        ..RateParams::default()
    };
    // At 95% (kink): no surcharge → raw rate:
    //   pre-kink = 100 + 8000*2000/10000 = 1700
    //   jump = (9500-8000)*10000/10000 = 1500
    //   raw = 1700 + 1500 = 3200
    let rate_at_kink = compute_borrow_rate(9_500, &params).unwrap();
    assert_eq!(rate_at_kink, 3_200);

    // At 100%: surcharge = (10000-9500)*80000/10000 = 4000
    //   raw = 3200 (at kink) + 2000*500/10000 (extra jump from 95→100%) + 4000 = ?
    //       Wait, let me recalculate:
    //   At 10000 util:
    //     pre-kink = 100 + 8000*2000/10000 = 1700
    //     jump = (10000-8000)*10000/10000 = 2000
    //     raw = 1700 + 2000 = 3700
    //     surcharge = (10000-9500)*80000/10000 = 4000
    //     total = 3700 + 4000 = 7700
    let rate_at_100 = compute_borrow_rate(10_000, &params).unwrap();
    assert_eq!(
        rate_at_100,
        3_700 + 4_000, // 7700
        "expected raw rate + surcharge at 100% utilization"
    );
}

/// Surcharge near 100% utilization is computed correctly.
#[test]
fn test_surcharge_near_full_utilization() {
    let params = RateParams {
        surcharge_kink_bps: 9_900, // 99%
        surcharge_slope: 100_000,  // 10× jump per % above 99%
        ..RateParams::default()
    };
    // At 99.5% util:
    //   surcharge_excess = 9950 - 9900 = 50 bps
    //   surcharge = 50 * 100_000 / 10_000 = 500 bps
    //   raw at 99.5%:
    //     pre-kink = 100 + 8000*2000/10000 = 1700
    //     jump = (9950-8000)*10000/10000 = 1950
    //     raw = 3650
    //     total = 3650 + 500 = 4150
    let rate = compute_borrow_rate(9_950, &params).unwrap();
    assert_eq!(rate, 4_150);

    // At 100% util:
    //   surcharge_excess = 100
    //   surcharge = 100 * 100_000 / 10_000 = 1000
    //   raw = 3700
    //   total = 4700
    let rate = compute_borrow_rate(10_000, &params).unwrap();
    assert_eq!(rate, 4_700);
}

/// The ceiling clamp still applies after the surcharge is added.
#[test]
fn test_ceiling_clamps_surcharge() {
    let params = RateParams {
        surcharge_kink_bps: 9_000,
        surcharge_slope: 2_000_000, // extreme slope
        rate_ceiling_bps: 5_000,    // 50% ceiling
        ..RateParams::default()
    };
    // At 91%: surcharge_excess = 100, surcharge = 100 * 2_000_000 / 10000 = 20_000
    //   raw ≈ 2000ish, raw + surcharge ≈ 22_000 → clamped to 5_000
    let rate = compute_borrow_rate(9_100, &params).unwrap();
    assert_eq!(rate, 5_000, "surcharged rate should be clamped by ceiling");

    // Even at 100%, ceiling still applies
    let rate = compute_borrow_rate(10_000, &params).unwrap();
    assert_eq!(rate, 5_000);
}

/// The floor clamp still applies (unchanged from legacy behaviour).
#[test]
fn test_floor_still_applies() {
    let params = RateParams {
        rate_floor_bps: 500,
        rate_ceiling_bps: 10_000,
        ..RateParams::default()
    };
    // At 0% util → base_rate = 100, but floor = 500 → 500
    let rate = compute_borrow_rate(0, &params).unwrap();
    assert_eq!(rate, 500);
}

/// The function is monotonic non-decreasing across the full utilization range.
#[test]
fn test_monotonic_non_decreasing() {
    let params = RateParams {
        surcharge_kink_bps: 8_500,
        surcharge_slope: 50_000,
        ..RateParams::default()
    };
    let mut prev_rate = i128::MIN;
    for util in (0..=10_000).step_by(100) {
        let rate = compute_borrow_rate(util, &params).unwrap();
        assert!(
            rate >= prev_rate,
            "rate decreased at utilization={}: {} < {}",
            util,
            rate,
            prev_rate
        );
        prev_rate = rate;
    }
}

/// Monotonicity holds with an aggressive surcharge slope.
#[test]
fn test_monotonic_aggressive_surcharge() {
    let params = RateParams {
        surcharge_kink_bps: 9_500,
        surcharge_slope: 500_000,
        ..RateParams::default()
    };
    let mut prev_rate = i128::MIN;
    for util in (0..=10_000).step_by(50) {
        let rate = compute_borrow_rate(util, &params).unwrap();
        assert!(
            rate >= prev_rate,
            "rate decreased at utilization={}: {} < {}",
            util,
            rate,
            prev_rate
        );
        prev_rate = rate;
    }
}

/// Overflow in the surcharge computation returns an error (not a panic).
#[test]
fn test_surcharge_overflow_returns_error() {
    let params = RateParams {
        surcharge_kink_bps: 0,
        surcharge_slope: i128::MAX,
        ..RateParams::default()
    };
    // With surcharge_kink = 0 and surcharge_slope = i128::MAX,
    // a large enough utilization will cause overflow:
    // surcharge_excess * surcharge_slope = 10000 * i128::MAX > i128::MAX
    let result = compute_borrow_rate(10_000, &params);
    assert_eq!(
        result,
        Err(RateModelError::Overflow),
        "expected Overflow error from surcharge multiplication, got {:?}",
        result
    );
}

/// Surcharge with kink at 0% applies surcharge at any positive utilization.
#[test]
fn test_surcharge_kink_at_zero() {
    let params = RateParams {
        surcharge_kink_bps: 0,
        surcharge_slope: 5_000,
        ..RateParams::default()
    };
    // At 1% util: surcharge = 100 * 5000 / 10000 = 50
    //   raw = 100 + 100*2000/10000 = 120
    //   total = 120 + 50 = 170
    let rate = compute_borrow_rate(100, &params).unwrap();
    assert_eq!(rate, 170);

    // At 10% util: surcharge = 1000 * 5000 / 10000 = 500
    //   raw = 100 + 1000*2000/10000 = 300
    //   total = 800
    let rate = compute_borrow_rate(1_000, &params).unwrap();
    assert_eq!(rate, 800);

    // Monotonic check for this extreme configuration
    let mut prev_rate = i128::MIN;
    for util in (0..=10_000).step_by(200) {
        let rate = compute_borrow_rate(util, &params).unwrap();
        assert!(
            rate >= prev_rate,
            "rate decreased at utilization={}: {} < {}",
            util,
            rate,
            prev_rate
        );
        prev_rate = rate;
    }
}

/// Surcharge with kink above BPS_DENOM never activates (disabled).
#[test]
fn test_surcharge_kink_above_max_util() {
    let params = RateParams {
        surcharge_kink_bps: BPS_DENOM + 1, // above max possible utilization
        surcharge_slope: 100_000,
        ..RateParams::default()
    };
    // Even at 100% util, surcharge doesn't activate (kink is above 10000)
    let rate = compute_borrow_rate(10_000, &params).unwrap();
    let expected = compute_borrow_rate(10_000, &RateParams::default()).unwrap();
    assert_eq!(rate, expected);
}

/// Verify exact surcharge computation at multiple points.
#[test]
fn test_surcharge_exact_values() {
    // Configure: kink at 80%, surcharge kink at 95%, slope 20000
    let params = RateParams {
        kink_utilization_bps: 8_000,
        surcharge_kink_bps: 9_500,
        surcharge_slope: 20_000,
        ..RateParams::default()
    };

    // 94% util → below surcharge kink, no surcharge
    //   pre-kink = 100 + 8000*2000/10000 = 1700
    //   jump = (9400-8000)*10000/10000 = 1400
    //   raw = 3100
    //   surcharge = 0
    //   total = 3100
    assert_eq!(compute_borrow_rate(9_400, &params).unwrap(), 3_100);

    // 95% util → at surcharge kink, no surcharge
    //   raw = 100 + 1700 + 1500 = 3200
    assert_eq!(compute_borrow_rate(9_500, &params).unwrap(), 3_200);

    // 96% util → surcharge = (9600-9500)*20000/10000 = 200
    //   raw = 100 + 1700 + 1600 = 3300
    //   total = 3500
    assert_eq!(compute_borrow_rate(9_600, &params).unwrap(), 3_500);

    // 98% util → surcharge = (9800-9500)*20000/10000 = 600
    //   raw = 100 + 1600 + 1800 = 3500
    //   total = 4100
    assert_eq!(compute_borrow_rate(9_800, &params).unwrap(), 4_100);

    // 100% util → surcharge = (10000-9500)*20000/10000 = 1000
    //   raw = 3700
    //   total = 4700
    assert_eq!(compute_borrow_rate(10_000, &params).unwrap(), 4_700);
}
