//! Property-based tests for `math::compute_compound_interest`.
//!
//! Uses proptest to verify that the function maintains critical invariants
//! across a wide range of randomly generated inputs. These tests are
//! compiled only under `#[cfg(test)]` and require the `proptest` dev-dep.

#![cfg(test)]

use crate::math::{compute_compound_interest, MathError, MAX_RATE_BPS, SCALE};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Arbitrary-strategy helpers
// ---------------------------------------------------------------------------

/// Valid non-negative principal values (including zero).
fn principal() -> impl Strategy<Value = i128> {
    prop_oneof![
        0i128..=1_000_000_000_000i128,                   // typical range
        (1_000_000_000_001i128..=i128::MAX / 1_000_000), // large but safe
        prop::num::i128::POSITIVE,                       // any positive
    ]
    .prop_map(|v| v.abs()) // ensure non-negative
}

/// Valid rate bps values: 0 to MAX_RATE_BPS inclusive.
fn rate_bps() -> impl Strategy<Value = i128> {
    0i128..=MAX_RATE_BPS
}

/// Valid elapsed seconds: 0 to ~100 years.
fn elapsed_seconds() -> impl Strategy<Value = u64> {
    0u64..=3_153_600_000u64 // 0…100 years in seconds
}

// ---------------------------------------------------------------------------
// Invariant: interest is never negative
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn interest_is_always_non_negative(
        p in principal(),
        r in rate_bps(),
        t in elapsed_seconds(),
    ) {
        match compute_compound_interest(p, r, t) {
            Ok(interest) => prop_assert!(interest >= 0, "interest must be >= 0"),
            Err(e) => {
                // The only acceptable errors are Overflow or OutOfRange.
                // An error is tolerable only when inputs push the
                // multiplication past i128::MAX.
                prop_assert!(
                    matches!(e, MathError::Overflow | MathError::OutOfRange),
                    "unexpected error {:?} for p={}, r={}, t={}",
                    e, p, r, t,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant: zero principal → zero interest
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn zero_principal_yields_zero_interest(
        r in rate_bps(),
        t in elapsed_seconds(),
    ) {
        let interest = compute_compound_interest(0, r, t).unwrap();
        prop_assert_eq!(interest, 0, "zero principal must produce zero interest");
    }
}

// ---------------------------------------------------------------------------
// Invariant: zero elapsed time → zero interest
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn zero_elapsed_yields_zero_interest(
        p in 1i128..=1_000_000_000_000i128,
        r in rate_bps(),
    ) {
        let interest = compute_compound_interest(p, r, 0).unwrap();
        prop_assert_eq!(interest, 0, "zero elapsed time must produce zero interest");
    }
}

// ---------------------------------------------------------------------------
// Invariant: zero rate → zero interest
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn zero_rate_yields_zero_interest(
        p in 1i128..=1_000_000_000_000i128,
        t in 1u64..=31_536_000u64, // up to 1 year
    ) {
        let interest = compute_compound_interest(p, 0, t).unwrap();
        prop_assert_eq!(interest, 0, "zero rate must produce zero interest");
    }
}

// ---------------------------------------------------------------------------
// Invariant: interest scales linearly with principal (for small values
// where overflow is not a concern).
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn interest_scales_linearly_with_principal(
        base in 1i128..=1_000_000i128,
        scale_factor in 1i128..=100i128,
        r in 1i128..=1000i128,          // ≤ 10 %
        t in 3600u64..=31_536_000u64,   // 1 hour … 1 year
    ) {
        let p1 = base;
        let p2 = base.checked_mul(scale_factor).unwrap_or(i128::MAX / 2);
        if p2 < 0 { return Ok(()); } // skip on overflow to negative

        let i1 = compute_compound_interest(p1, r, t).unwrap();
        let i2 = compute_compound_interest(p2, r, t).unwrap();

        // i2 should be (approximately) scale_factor × i1
        // Allow a small rounding difference of 1 unit due to integer division.
        let expected_min = i1.saturating_mul(scale_factor).saturating_sub(scale_factor);
        let expected_max = i1.saturating_mul(scale_factor).saturating_add(scale_factor);

        prop_assert!(
            i2 >= expected_min && i2 <= expected_max,
            "linearity violated: p1={}, p2={} (×{}), i1={}, i2={}, expect in [{}, {}]",
            p1, p2, scale_factor, i1, i2, expected_min, expected_max,
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant: minimum interest floor of 1 for any positive principal and
// elapsed time, when the computed result would otherwise round to 0.
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn minimum_interest_floor_applied(
        p in 1i128..=1_000i128,
        r in 1i128..=100i128,
        t in 1u64..=3600u64,
    ) {
        let interest = compute_compound_interest(p, r, t).unwrap();
        // The interest *should* be at least 1 because the function applies
        // a floor of 1 for any positive principal and elapsed time.
        prop_assert!(
            interest >= 1,
            "interest floor of 1 violated (got {}) for p={}, r={}, t={}",
            interest, p, r, t,
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant: interest is monotonically non-decreasing with elapsed time
// (for a fixed principal and rate).
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn interest_monotonic_with_time(
        p in 1i128..=10_000_000i128,
        r in rate_bps(),
        t1 in 0u64..=15_768_000u64,    // 0 … 6 months
        t2 in 15_768_001u64..=31_536_000u64, // 6 months+1 … 1 year
    ) {
        let i1 = compute_compound_interest(p, r, t1).unwrap_or(0);
        let i2 = compute_compound_interest(p, r, t2).unwrap_or(0);
        prop_assert!(
            i2 >= i1,
            "interest must be non-decreasing with time: t1={} → i1={}, t2={} → i2={}",
            t1, i1, t2, i2,
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant: interest is monotonically non-decreasing with rate
// (for a fixed principal and elapsed time).
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn interest_monotonic_with_rate(
        p in 1i128..=10_000_000i128,
        t in 3600u64..=31_536_000u64,
        r1 in 0i128..=MAX_RATE_BPS / 2,
        r2 in (MAX_RATE_BPS / 2 + 1)..=MAX_RATE_BPS,
    ) {
        let i1 = compute_compound_interest(p, r1, t).unwrap_or(0);
        let i2 = compute_compound_interest(p, r2, t).unwrap_or(0);
        prop_assert!(
            i2 >= i1,
            "interest must be non-decreasing with rate: r1={} → i1={}, r2={} → i2={}",
            r1, i1, r2, i2,
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant: Extreme but valid inputs should not panic (return Err instead).
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn extreme_values_dont_panic(
        p in (i128::MAX / 1000)..=i128::MAX,
        r in rate_bps(),
        t in 31_536_000u64..=3_153_600_000u64, // 1 … 100 years
    ) {
        // Should never panic — should either produce an interest value or
        // return an Err (Overflow or OutOfRange).
        let result = compute_compound_interest(p, r, t);
        match result {
            Ok(interest) => {
                prop_assert!(interest >= 0, "interest must be >= 0");
            }
            Err(e) => {
                prop_assert!(
                    matches!(e, MathError::Overflow | MathError::OutOfRange),
                    "unexpected error {:?}",
                    e,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant: Known reference values match exactly.
// ---------------------------------------------------------------------------

/// Known-answer tests for `compute_compound_interest`.
/// These serve as regression anchors and documentation.
#[test]
fn known_reference_values() {
    // (principal, rate_bps, elapsed, expected_interest)
    let cases: &[(i128, i128, u64, i128)] = &[
        (1_000_000_000, 1000, 31_536_000, 100_000_000), // 100 * 10% per year
        (10_000, 500, 31_536_000, 500),                 // 10K * 5% per year
        (100_000, 500, 15_768_000, 250),                // 100K * 5% for 6 months
        (50_000, 500, 7_884_000, 125),                  // 50K * 5% for 3 months
        (1, 1, 1, 1),                                   // minimum floor
        (0, 5000, 31_536_000, 0),                       // zero principal
        (5_000, 0, 31_536_000, 0),                      // zero rate
        (5_000, 500, 0, 0),                             // zero time
    ];

    for &(p, r, t, expected) in cases {
        let actual = compute_compound_interest(p, r, t)
            .unwrap_or_else(|e| panic!("unexpected error {:?} for p={}, r={}, t={}", e, p, r, t));
        assert_eq!(
            actual, expected,
            "compute_compound_interest({}, {}, {}) = {}, expected {}",
            p, r, t, actual, expected,
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant: overflow returns Err(MathError::Overflow), not a panic.
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn overflow_returns_err_not_panic(
        // Use extreme values that are guaranteed to overflow:
        p in (i128::MAX / 10)..=i128::MAX,
        r in (MAX_RATE_BPS - 1000)..=MAX_RATE_BPS,
        t in 31_536_000u64..=3_153_600_000u64,
    ) {
        let result = compute_compound_interest(p, r, t);
        match result {
            Err(MathError::Overflow) => {} // expected
            Err(MathError::OutOfRange) if r < 0 || r > MAX_RATE_BPS => {} // expected
            Ok(_) => {} // might not overflow for some combinations — fine
            Err(e) => {
                panic!("unexpected error {:?} for p={}, r={}, t={}", e, p, r, t);
            }
        }
    }
}

use crate::math::compute_compound_interest;
use proptest::prelude::*;

proptest! {

    #[test]
    fn interest_never_below_principal(
        principal in 1i128..1_000_000_000,
        rate in 0i128..5_000,
        elapsed in 0u64..10_000,
    ) {
        let result = compute_compound_interest(principal, rate, elapsed).unwrap();
        prop_assert!(result >= principal);
    }

    #[test]
    fn monotonic_in_elapsed(
        principal in 1i128..1_000_000,
        rate in 0i128..3_000,
        t1 in 0u64..500,
        t2 in 501u64..1000,
    ) {
        let r1 = compute_compound_interest(principal, rate, t1).unwrap();
        let r2 = compute_compound_interest(principal, rate, t2).unwrap();

        prop_assert!(r2 >= r1);
    }

    #[test]
    fn monotonic_in_rate(
        principal in 1i128..1_000_000,
        rate1 in 0i128..1000,
        rate2 in 1001i128..3000,
        elapsed in 0u64..1000,
    ) {
        let r1 = compute_compound_interest(principal, rate1, elapsed).unwrap();
        let r2 = compute_compound_interest(principal, rate2, elapsed).unwrap();

        prop_assert!(r2 >= r1);
    }
}
