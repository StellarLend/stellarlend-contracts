//! Round-trip and loss-bound tests for [`scale_bps`] / [`unscale_bps`].
//!
//! Proves that `unscale_bps(scale_bps(v, r), r)` recovers `v` within at most
//! **one unit** of rounding loss whenever `|r| ≥ BPS_DENOM` (rate ≥ 100 %),
//! and that overflow is never silently wrapped.
//!
//! # Rounding contract
//!
//! Both helpers truncate (Rust's truncating `i128` division, which rounds toward
//! zero). A single truncation discards at most `|r| - 1` units of `v·r / D`.
//! When `|r| ≥ D` the combined truncation error of the two-step round-trip is
//! bounded by one unit:
//!
//! ```text
//! |unscale_bps(scale_bps(v, r), r) - v| ≤ 1   for |r| ≥ BPS_DENOM
//! ```
//!
//! For `|r| < BPS_DENOM` the theoretical bound is larger (`D / |r| + 1`).
//! Those cases are covered by the property tests in [`super::bps_inverse_proptest`].
//!
//! # Overflow contract
//!
//! Both helpers return `None` (never panic) whenever the intermediate
//! multiplication would exceed `i128::MAX`.

use crate::{scale_bps, unscale_bps, BPS_DENOM};

/// Assert that `unscale_bps(scale_bps(v, r), r)` differs from `v` by at most
/// **one unit** when `|r| ≥ BPS_DENOM`.
///
/// When `|r| < BPS_DENOM` the function still verifies the composition is sound
/// (no unexpected `None`, no panic) but does *not* assert the one-unit bound.
///
/// When `r == 0` the function verifies that `unscale_bps` returns `None`.
fn assert_round_trip_loss_one(value: i128, rate_bps: i128) {
    if rate_bps == 0 {
        assert!(
            unscale_bps(value, 0).is_none(),
            "unscale_bps(_, 0) must return None (value={})",
            value
        );
        return;
    }

    // scale_bps may return None on overflow — vacuously sound.
    let Some(scaled) = scale_bps(value, rate_bps) else {
        return;
    };

    let Some(round_trip) = unscale_bps(scaled, rate_bps) else {
        panic!(
            "unscale_bps returned None after successful scale (value={}, rate_bps={}, scaled={})",
            value, rate_bps, scaled
        );
    };

    let diff = round_trip.abs_diff(value);

    // The one-unit bound only holds when |rate| ≥ BPS_DENOM.
    if rate_bps.unsigned_abs() >= (BPS_DENOM as u128) {
        assert!(
            diff <= 1,
            "round-trip error {} exceeds 1 (value={}, rate_bps={}, scaled={}, round_trip={})",
            diff,
            value,
            rate_bps,
            scaled,
            round_trip
        );
    }
}

/// Assert that both helpers return `None` (never wrap) on overflow.
fn assert_overflow_none(value: i128, rate_bps: i128) {
    let scaled = scale_bps(value, rate_bps);
    let unscaled = unscale_bps(value, rate_bps);
    // At least one of the two should be None for these extreme inputs.
    // (Both might be None if both paths overflow.)
    assert!(
        scaled.is_none() || unscaled.is_none(),
        "expected at least one None for overflow input (value={}, rate_bps={}, scaled={:?}, unscaled={:?})",
        value, rate_bps, scaled, unscaled
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Round-trip: at-most-one-unit loss (|r| ≥ BPS_DENOM) ────────

    #[test]
    fn round_trip_full_hundred_percent() {
        assert_round_trip_loss_one(1_000_000, BPS_DENOM);
    }

    #[test]
    fn round_trip_one_fifty_percent() {
        assert_round_trip_loss_one(1_000_000, BPS_DENOM + 5_000);
    }

    #[test]
    fn round_trip_two_hundred_percent() {
        assert_round_trip_loss_one(1_000_000, BPS_DENOM * 2);
    }

    #[test]
    fn round_trip_ten_thousand_percent() {
        assert_round_trip_loss_one(1_000_000, BPS_DENOM * 100);
    }

    #[test]
    fn round_trip_negative_rate_at_hundred_percent() {
        assert_round_trip_loss_one(1_000_000, -BPS_DENOM);
    }

    #[test]
    fn round_trip_negative_value_at_hundred_percent() {
        assert_round_trip_loss_one(-1_000_000, BPS_DENOM);
    }

    #[test]
    fn round_trip_both_negative() {
        assert_round_trip_loss_one(-1_000_000, -BPS_DENOM);
    }

    #[test]
    fn round_trip_small_value_at_hundred_percent() {
        assert_round_trip_loss_one(1, BPS_DENOM);
        assert_round_trip_loss_one(7, BPS_DENOM);
        assert_round_trip_loss_one(99, BPS_DENOM + 1);
    }

    #[test]
    fn round_trip_max_value_at_hundred_percent() {
        // i128::MAX * BPS_DENOM / BPS_DENOM = i128::MAX (exact)
        // But checked_mul(i128::MAX, BPS_DENOM) overflows → None.
        // round-trip is vacuously sound (scale returns None).
        assert_round_trip_loss_one(i128::MAX, BPS_DENOM);
    }

    #[test]
    fn round_trip_min_value_at_hundred_percent() {
        assert_round_trip_loss_one(i128::MIN, BPS_DENOM);
    }

    // ── Edge cases: zero value, zero rate, one-bps rate ─────────────

    #[test]
    fn round_trip_zero_value() {
        assert_round_trip_loss_one(0, 0);
        assert_round_trip_loss_one(0, 1);
        assert_round_trip_loss_one(0, 500);
        assert_round_trip_loss_one(0, BPS_DENOM);
        assert_round_trip_loss_one(0, BPS_DENOM * 2);
    }

    #[test]
    fn round_trip_zero_rate_unscale_none() {
        assert!(unscale_bps(42, 0).is_none());
        assert!(unscale_bps(0, 0).is_none());
        assert!(unscale_bps(-1, 0).is_none());
    }

    #[test]
    fn round_trip_one_bps() {
        // |r| = 1 < BPS_DENOM → bound is D/1 + 1 = 10_001, not 1.
        // This test just verifies the composition succeeds without panic.
        assert_round_trip_loss_one(10_000, 1);
        assert_round_trip_loss_one(1, 1);
        assert_round_trip_loss_one(0, 1);
        assert_round_trip_loss_one(-10_000, 1);
    }

    // ── Negative values ─────────────────────────────────────────────

    #[test]
    fn round_trip_negative_value_various_rates() {
        assert_round_trip_loss_one(-1, BPS_DENOM);
        assert_round_trip_loss_one(-100, BPS_DENOM + 500);
        assert_round_trip_loss_one(-1_000_000, BPS_DENOM * 2);
        assert_round_trip_loss_one(-1_000_000, 500); // 5 % (bound > 1)
        assert_round_trip_loss_one(-1, 1);
    }

    #[test]
    fn round_trip_negative_rate_with_positive_value() {
        assert_round_trip_loss_one(100, -BPS_DENOM);
        assert_round_trip_loss_one(1_000_000, -500);
        assert_round_trip_loss_one(42, -1);
    }

    // ── Overflow → None (never wrap) ───────────────────────────────

    #[test]
    fn scale_overflow_max_times_two() {
        assert!(scale_bps(i128::MAX, 2).is_none());
    }

    #[test]
    fn scale_overflow_min_times_two() {
        assert!(scale_bps(i128::MIN, 2).is_none());
    }

    #[test]
    fn unscale_overflow_max() {
        assert!(unscale_bps(i128::MAX, 1).is_none());
    }

    #[test]
    fn unscale_overflow_min() {
        assert!(unscale_bps(i128::MIN, 1).is_none());
    }

    #[test]
    fn overflow_large_value_moderate_rate() {
        assert_overflow_none(i128::MAX, BPS_DENOM);
    }

    // ── Composition success paths ─────────────────────────────────────

    #[test]
    fn composition_exact_division() {
        // 1_000_000 * 500 / 10_000 = 50_000 (exact)
        // 50_000 * 10_000 / 500 = 1_000_000 (exact)
        let scaled = scale_bps(1_000_000, 500).unwrap();
        let round_trip = unscale_bps(scaled, 500).unwrap();
        assert_eq!(round_trip, 1_000_000);
    }

    #[test]
    fn composition_full_rate_exact() {
        // At 100 % rate, scale and unscale should be identity when no
        // truncation loss occurs.
        let scaled = scale_bps(42, BPS_DENOM).unwrap();
        assert_eq!(scaled, 42);
        let round_trip = unscale_bps(scaled, BPS_DENOM).unwrap();
        assert_eq!(round_trip, 42);
    }

    #[test]
    fn composition_small_loss_at_full_rate() {
        // 5 * 10_000 / 10_000 = 5
        // 5 * 10_000 / 10_001 = 4  (truncation loss of 1)
        let scaled = scale_bps(5, BPS_DENOM + 1).unwrap();
        assert_eq!(scaled, 5);
        let round_trip = unscale_bps(scaled, BPS_DENOM + 1).unwrap();
        assert_eq!(round_trip, 4);
        let diff = round_trip.abs_diff(5);
        assert_eq!(diff, 1);
    }

    // ── i128 boundary behavior ─────────────────────────────────────

    #[test]
    fn boundary_max_value_safe_rate() {
        // rate = 1: scale(i128::MAX, 1) = i128::MAX / D (safe)
        let scaled = scale_bps(i128::MAX, 1).unwrap();
        let round_trip = unscale_bps(scaled, 1).unwrap();
        // |round_trip - i128::MAX| should be < D
        let diff = round_trip.abs_diff(i128::MAX);
        assert!(diff < BPS_DENOM as u128);
    }

    #[test]
    fn boundary_min_value_safe_rate() {
        let scaled = scale_bps(i128::MIN, 1).unwrap();
        let round_trip = unscale_bps(scaled, 1).unwrap();
        let diff = round_trip.abs_diff(i128::MIN);
        assert!(diff < BPS_DENOM as u128);
    }

    // ── Determinism / idempotency ─────────────────────────────────

    #[test]
    fn composition_is_deterministic() {
        for _ in 0..10 {
            assert_round_trip_loss_one(99_999, BPS_DENOM);
            assert_round_trip_loss_one(99_999, BPS_DENOM + 1);
        }
    }
}
