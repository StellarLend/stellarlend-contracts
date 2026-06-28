use crate::rate_model::{compute_borrow_rate, RateParams};

#[test]
fn test_surcharge_disabled_by_default_is_no_op() {
    let params = RateParams::default();

    assert_eq!(compute_borrow_rate(0, &params), 100);
    assert_eq!(compute_borrow_rate(8_000, &params), 1_700);
    assert_eq!(compute_borrow_rate(9_000, &params), 2_700);
}

#[test]
fn test_surcharge_applies_only_above_kink() {
    let params = RateParams {
        surcharge_kink_bps: 8_000,
        surcharge_slope: 5_000,
        ..Default::default()
    };

    assert_eq!(compute_borrow_rate(8_000, &params), 1_700);
    assert_eq!(compute_borrow_rate(9_000, &params), 3_200);
}

#[test]
fn test_surcharge_respects_ceiling() {
    let params = RateParams {
        surcharge_kink_bps: 9_000,
        surcharge_slope: 100_000,
        rate_ceiling_bps: 4_500,
        ..Default::default()
    };

    let rate = compute_borrow_rate(20_000, &params);
    assert_eq!(rate, 4_500);
}

#[test]
fn test_surcharge_near_full_utilization_remains_monotonic() {
    let params = RateParams {
        surcharge_kink_bps: 9_000,
        surcharge_slope: 1_000,
        ..Default::default()
    };

    let before = compute_borrow_rate(9_900, &params);
    let at_full = compute_borrow_rate(10_000, &params);

    assert!(before <= at_full);
    assert!(at_full <= params.rate_ceiling_bps);
}

#[test]
fn test_surcharge_monotonic_non_decreasing_in_utilization() {
    let params = RateParams {
        surcharge_kink_bps: 8_000,
        surcharge_slope: 2_000,
        ..Default::default()
    };

    let at_kink = compute_borrow_rate(8_000, &params);
    let above_kink = compute_borrow_rate(8_001, &params);
    let far_above = compute_borrow_rate(10_000, &params);

    assert!(at_kink <= above_kink);
    assert!(above_kink <= far_above);
}
