use crate::rate_model::{compute_borrow_rate, RateParams};

/// Surcharge disabled when both fields are zero; behavior matches the base curve.
#[test]
fn test_surcharge_disabled_preserves_base_curve() {
    let params = RateParams::default();

    assert_eq!(compute_borrow_rate(0, &params), 100);
    assert_eq!(compute_borrow_rate(8_000, &params), 1_700);
    assert_eq!(compute_borrow_rate(9_000, &params), 2_700);
    assert_eq!(compute_borrow_rate(10_000, &params), 3_700);
}

/// No surcharge is applied when utilization is below the surcharge kink.
#[test]
fn test_surcharge_not_applied_below_kink() {
    let params = RateParams {
        surcharge_kink_bps: 9_000,
        surcharge_slope: 5_000,
        ..Default::default()
    };

    let below_kink = compute_borrow_rate(8_999, &params);
    let without_surcharge = compute_borrow_rate(8_999, &RateParams::default());

    assert_eq!(below_kink, without_surcharge);
}

/// No surcharge is applied when utilization is exactly at the surcharge kink.
#[test]
fn test_surcharge_not_applied_at_kink() {
    let params = RateParams {
        surcharge_kink_bps: 8_000,
        surcharge_slope: 5_000,
        ..Default::default()
    };

    assert_eq!(compute_borrow_rate(8_000, &params), 1_700);
}

/// Surcharge applies once utilization exceeds the surcharge kink.
#[test]
fn test_surcharge_applies_above_kink() {
    let params = RateParams {
        surcharge_kink_bps: 8_000,
        surcharge_slope: 5_000,
        ..Default::default()
    };

    // base curve at 90%: 2_700; surcharge: (1_000 * 5_000) / 10_000 = 500
    assert_eq!(compute_borrow_rate(9_000, &params), 3_200);
}

/// Near-full utilization keeps increasing the rate up to the ceiling.
#[test]
fn test_surcharge_near_full_utilization() {
    let params = RateParams {
        surcharge_kink_bps: 9_000,
        surcharge_slope: 1_000,
        ..Default::default()
    };

    let at_99 = compute_borrow_rate(9_900, &params);
    let at_100 = compute_borrow_rate(10_000, &params);

    assert!(at_99 <= at_100);
    assert!(at_100 <= params.rate_ceiling_bps);
}

/// The rate ceiling still clamps the final rate after the surcharge is applied.
#[test]
fn test_surcharge_respects_ceiling() {
    let params = RateParams {
        surcharge_kink_bps: 9_000,
        surcharge_slope: 100_000,
        rate_ceiling_bps: 4_500,
        ..Default::default()
    };

    assert_eq!(compute_borrow_rate(20_000, &params), 4_500);
}

/// Borrow rate is non-decreasing as utilization increases when surcharge is enabled.
#[test]
fn test_surcharge_monotonic_in_utilization() {
    let params = RateParams {
        surcharge_kink_bps: 8_000,
        surcharge_slope: 2_000,
        ..Default::default()
    };

    let mut prev = compute_borrow_rate(0, &params);
    for util in [1_000, 4_000, 7_999, 8_000, 8_001, 9_000, 9_900, 10_000] {
        let rate = compute_borrow_rate(util, &params);
        assert!(prev <= rate, "rate decreased at util {util}: {prev} -> {rate}");
        prev = rate;
    }
}
