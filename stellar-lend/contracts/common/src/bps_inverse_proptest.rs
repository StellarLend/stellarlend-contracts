//! Property-based invariants for [`scale_bps`] / [`unscale_bps`].
//!
//! These tests complement the example-based unit tests in `lib.rs` by
//! randomising `value` and `rate_bps` across the full `i128` range and
//! asserting the helpers' structural guarantees on every generated case.
//!
//! # Invariants proven
//!
//! **I-1 Round-trip error bound** — When both directions succeed, the
//! round-trip `unscale_bps(scale_bps(v, r), r)` differs from `v` by at most
//! `BPS_DENOM / |r| + 1`. Two truncating integer divisions occur (one per
//! direction); each contributes at most one unit of its own quotient, and the
//! `scale` remainder (`< BPS_DENOM`) is magnified by `1 / |r|` on the way back.
//! When `|r| >= BPS_DENOM` (rate ≥ 100 %) the bound collapses to the documented
//! **one-unit** rounding error.
//!
//! **I-2 Totality (never panics, `None` on overflow)** — Both helpers return a
//! value for every `(value, rate_bps)` pair and yield `None` exactly when the
//! underlying checked arithmetic would overflow (verified against a reference
//! oracle built from the same checked primitives).
//!
//! **I-3 Zero divisor** — `unscale_bps(_, 0)` is always `None`.
//!
//! **I-4 Sign consistency** — Negating `value` negates the result, so the
//! helpers behave symmetrically across zero.

use crate::{scale_bps, unscale_bps, BPS_DENOM};
use proptest::prelude::*;

/// Reference oracle for [`scale_bps`] using the same checked primitives.
///
/// Used to prove `scale_bps` returns `None` exactly on overflow and never panics.
fn scale_ref(value: i128, rate_bps: i128) -> Option<i128> {
    value
        .checked_mul(rate_bps)
        .and_then(|product| product.checked_div(BPS_DENOM))
}

/// Reference oracle for [`unscale_bps`] (zero divisor and overflow → `None`).
fn unscale_ref(value: i128, rate_bps: i128) -> Option<i128> {
    if rate_bps == 0 {
        return None;
    }
    value
        .checked_mul(BPS_DENOM)
        .and_then(|product| product.checked_div(rate_bps))
}

proptest! {
    /// **I-1** — Round-trip error is within `BPS_DENOM / |rate_bps| + 1`.
    #[test]
    fn prop_round_trip_within_bound(value in any::<i128>(), rate_bps in any::<i128>()) {
        prop_assume!(rate_bps != 0);
        if let Some(scaled) = scale_bps(value, rate_bps) {
            if let Some(round_trip) = unscale_bps(scaled, rate_bps) {
                let diff = round_trip
                    .checked_sub(value)
                    .expect("round-trip stays within one quotient of the original");
                let bound = (BPS_DENOM as u128) / rate_bps.unsigned_abs() + 1;
                prop_assert!(
                    diff.unsigned_abs() <= bound,
                    "round-trip error {} exceeds bound {} (value={}, rate_bps={}, round_trip={})",
                    diff.unsigned_abs(), bound, value, rate_bps, round_trip
                );
            }
        }
    }

    /// **I-2** — `scale_bps` never panics and is `None` exactly on overflow.
    #[test]
    fn prop_scale_matches_reference(value in any::<i128>(), rate_bps in any::<i128>()) {
        prop_assert_eq!(scale_bps(value, rate_bps), scale_ref(value, rate_bps));
    }

    /// **I-2** — `unscale_bps` never panics and is `None` on overflow / zero divisor.
    #[test]
    fn prop_unscale_matches_reference(value in any::<i128>(), rate_bps in any::<i128>()) {
        prop_assert_eq!(unscale_bps(value, rate_bps), unscale_ref(value, rate_bps));
    }

    /// **I-3** — A zero divisor always yields `None`, independent of `value`.
    #[test]
    fn prop_unscale_zero_divisor_is_none(value in any::<i128>()) {
        prop_assert_eq!(unscale_bps(value, 0), None);
    }

    /// **I-4** — Negating the value negates the scaled result.
    #[test]
    fn prop_sign_consistency(value in any::<i128>(), rate_bps in any::<i128>()) {
        // `i128::MIN` has no positive counterpart; skip that single degenerate input.
        let neg_value = match value.checked_neg() {
            Some(v) => v,
            None => return Ok(()),
        };
        if let (Some(positive), Some(negative)) =
            (scale_bps(value, rate_bps), scale_bps(neg_value, rate_bps))
        {
            prop_assert_eq!(Some(negative), positive.checked_neg());
        }
    }

    /// One-bps edge: at the smallest non-zero rate, `scale_bps` is `v / BPS_DENOM`.
    #[test]
    fn prop_one_bps_edge(value in any::<i128>()) {
        prop_assert_eq!(scale_bps(value, 1), value.checked_div(BPS_DENOM));
    }
}