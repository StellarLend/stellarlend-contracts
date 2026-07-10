#[cfg(test)]
mod bps_roundtrip_tests {
    use stellar_lend_common::{scale_bps, unscale_bps, BPS_DENOM};

    /// Round-trip: unscale_bps(scale_bps(v, r), r) should recover v within ±1 unit.
    fn assert_roundtrip_within_one(v: i128, r: i128) {
        let scaled = scale_bps(v, r).expect("scale_bps should not overflow");
        let recovered = unscale_bps(scaled, r).expect("unscale_bps should not overflow");
        let diff = (recovered - v).abs();
        assert!(
            diff <= 1,
            "roundtrip loss = {} for v={}, r={}; expected <= 1",
            diff, v, r
        );
    }

    #[test]
    fn roundtrip_positive_values() {
        for v in [1, 10, 100, 1_000, 10_000, 1_000_000, 123_456_789] {
            for r in [1, 50, 500, 1_000, 2_500, 5_000, 7_500, 9_999, 10_000] {
                assert_roundtrip_within_one(v, r);
            }
        }
    }

    #[test]
    fn roundtrip_negative_values() {
        for v in [-1, -10, -100, -1_000, -10_000, -1_000_000] {
            for r in [1, 50, 500, 1_000, 2_500, 5_000, 10_000] {
                assert_roundtrip_within_one(v, r);
            }
        }
    }

    #[test]
    fn roundtrip_zero_value() {
        for r in [1, 50, 500, 1_000, 10_000] {
            assert_eq!(scale_bps(0, r), Some(0));
            assert_eq!(unscale_bps(0, r), Some(0));
        }
    }

    #[test]
    fn roundtrip_full_rate() {
        // At 100% rate, BPS_DENOM, scale/unscale are identity.
        for v in [1, 100, 50_000, 1_000_000] {
            assert_eq!(scale_bps(v, BPS_DENOM), Some(v));
            assert_eq!(unscale_bps(v, BPS_DENOM), Some(v));
        }
    }

    #[test]
    fn scale_bps_overflow_max_i128() {
        // i128::MAX with any rate > 0 that makes product exceed i128::MAX
        assert_eq!(scale_bps(i128::MAX, 2), None);
        // i128::MIN with positive rate should also overflow
        assert_eq!(scale_bps(i128::MIN, 2), None);
    }

    #[test]
    fn unscale_bps_overflow_max_i128() {
        // i128::MAX * BPS_DENOM overflows
        assert_eq!(unscale_bps(i128::MAX, 1), None);
    }

    #[test]
    fn unscale_bps_zero_rate_returns_none() {
        assert_eq!(unscale_bps(1_000, 0), None);
        assert_eq!(unscale_bps(0, 0), None);
    }

    #[test]
    fn one_bps_precision() {
        // 1 BPS of 1_000_000 = 100; reverse recovers exactly
        assert_eq!(scale_bps(1_000_000, 1), Some(100));
        assert_eq!(unscale_bps(100, 1), Some(1_000_000));
    }

    #[test]
    fn large_safe_value_roundtrip() {
        let v = i128::MAX / 10_001; // safe from overflow even at 100% rate
        for r in [1, 100, 500, 1_000, 5_000, 10_000] {
            assert_roundtrip_within_one(v, r);
        }
    }
}
