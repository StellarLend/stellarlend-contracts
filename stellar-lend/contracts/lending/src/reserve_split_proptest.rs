//! Property-based tests for [`split_interest_by_reserve_factor`].
//!
//! These proptests assert the three core conservation invariants over the full
//! valid input space:
//!
//! 1. **No-leakage**: `depositor_yield + reserve_cut == total_interest` for every
//!    valid input pair.
//! 2. **Non-negativity**: both output parts are always `≥ 0`.
//! 3. **Rounding consistency**: any fractional unit falls to the depositor side —
//!    i.e. `reserve_cut == floor(total_interest * reserve_factor_bps / BPS_SCALE)`.
//! 4. **No panic / typed error on overflow**: the only way the function returns
//!    `Err` for valid inputs is `MathError::Overflow` on the intermediate
//!    multiplication, which only occurs when
//!    `total_interest > i128::MAX / BPS_SCALE`.
//!
//! Run with:
//! ```sh
//! cargo test -p stellarlend-lending reserve_split_proptest
//! ```

#![cfg(test)]

use super::math::{split_interest_by_reserve_factor, MathError, BPS_SCALE};
use proptest::prelude::*;

// ── Input strategies ──────────────────────────────────────────────────────────

/// Largest `total_interest` that cannot overflow the intermediate multiplication
/// `total_interest * reserve_factor_bps` inside [`split_interest_by_reserve_factor`].
///
/// The function multiplies `total_interest` by at most `BPS_SCALE` (10 000),
/// so the safe ceiling is `i128::MAX / BPS_SCALE`.
const SAFE_MAX_INTEREST: i128 = i128::MAX / BPS_SCALE as i128;

/// Strategy yielding non-negative interest values that stay within the
/// overflow-free range of the reserve-split multiplication.
fn safe_interest_strategy() -> impl Strategy<Value = i128> {
    0i128..=SAFE_MAX_INTEREST
}

/// Strategy yielding interest values that are **guaranteed to overflow** the
/// intermediate `total_interest * reserve_factor_bps` multiplication when
/// `reserve_factor_bps ≥ 1`.
fn overflow_interest_strategy() -> impl Strategy<Value = i128> {
    (SAFE_MAX_INTEREST + 1)..=i128::MAX
}

/// Strategy yielding a valid reserve factor in `[0, BPS_SCALE]` (inclusive).
fn reserve_factor_strategy() -> impl Strategy<Value = u32> {
    0u32..=BPS_SCALE
}

// ── Conservation invariants ───────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// **No-leakage invariant**: the two output parts always sum exactly to
    /// `total_interest`.  No unit of interest is created or destroyed by the
    /// split, regardless of how the BPS division rounds.
    #[test]
    fn reserve_split_sum_equals_total(
        total_interest in safe_interest_strategy(),
        reserve_factor_bps in reserve_factor_strategy(),
    ) {
        let (depositor, reserve) =
            split_interest_by_reserve_factor(total_interest, reserve_factor_bps)
                .expect("safe inputs must not overflow");

        prop_assert_eq!(
            depositor + reserve,
            total_interest,
            "no-leakage violated: total={total_interest} rf={reserve_factor_bps} \
             => depositor={depositor} reserve={reserve}"
        );
    }

    /// **Non-negativity invariant**: neither output part is ever negative.
    ///
    /// Negative splits would imply one party subsidises the other — that must
    /// never happen.
    #[test]
    fn reserve_split_both_parts_non_negative(
        total_interest in safe_interest_strategy(),
        reserve_factor_bps in reserve_factor_strategy(),
    ) {
        let (depositor, reserve) =
            split_interest_by_reserve_factor(total_interest, reserve_factor_bps)
                .expect("safe inputs must not overflow");

        prop_assert!(
            depositor >= 0,
            "depositor_yield is negative: total={total_interest} rf={reserve_factor_bps} \
             => depositor={depositor}"
        );
        prop_assert!(
            reserve >= 0,
            "reserve_cut is negative: total={total_interest} rf={reserve_factor_bps} \
             => reserve={reserve}"
        );
    }

    /// **Rounding-direction invariant**: any fractional BPS unit falls to the
    /// *depositor* side.  Concretely:
    ///
    /// ```text
    /// reserve_cut == floor(total_interest * reserve_factor_bps / BPS_SCALE)
    /// ```
    ///
    /// Equivalently, `reserve_cut * BPS_SCALE ≤ total_interest * reserve_factor_bps`.
    /// This ensures the protocol never captures more than its exact arithmetic share.
    #[test]
    fn reserve_split_rounding_favours_depositor(
        total_interest in safe_interest_strategy(),
        reserve_factor_bps in reserve_factor_strategy(),
    ) {
        let (depositor, reserve) =
            split_interest_by_reserve_factor(total_interest, reserve_factor_bps)
                .expect("safe inputs must not overflow");

        // Check floor condition: reserve_cut * BPS_SCALE ≤ total_interest * reserve_factor_bps
        // Both sides fit in i128 because total_interest ≤ SAFE_MAX_INTEREST.
        let lhs = reserve * BPS_SCALE as i128;
        let rhs = total_interest * reserve_factor_bps as i128;
        prop_assert!(
            lhs <= rhs,
            "rounding-direction violated: reserve_cut={reserve} * BPS_SCALE={lhs} \
             > total_interest={total_interest} * rf={reserve_factor_bps} ({rhs})"
        );

        // Also confirm depositor is not shortchanged beyond 1 unit
        // (the maximum rounding error from floor division).
        let max_depositor_exact = total_interest
            .saturating_sub(total_interest * reserve_factor_bps as i128 / BPS_SCALE as i128);
        prop_assert!(
            depositor >= max_depositor_exact,
            "depositor received less than the floor-rounded share: \
             depositor={depositor} expected≥{max_depositor_exact}"
        );
    }

    /// **Monotonicity in reserve factor**: a higher reserve factor never gives
    /// *more* to depositors.  As the protocol's cut grows, the depositor share
    /// weakly decreases.
    #[test]
    fn reserve_split_depositor_monotone_decreasing_in_rf(
        total_interest in safe_interest_strategy(),
        rf_lo in 0u32..BPS_SCALE,
        rf_hi in 0u32..=BPS_SCALE,
    ) {
        // Ensure rf_lo ≤ rf_hi (swap if needed).
        let (rf_lo, rf_hi) = if rf_lo <= rf_hi { (rf_lo, rf_hi) } else { (rf_hi, rf_lo) };

        let (depositor_lo, _) =
            split_interest_by_reserve_factor(total_interest, rf_lo)
                .expect("safe inputs must not overflow");
        let (depositor_hi, _) =
            split_interest_by_reserve_factor(total_interest, rf_hi)
                .expect("safe inputs must not overflow");

        prop_assert!(
            depositor_hi <= depositor_lo,
            "monotonicity violated: rf_lo={rf_lo} depositor={depositor_lo}, \
             rf_hi={rf_hi} depositor={depositor_hi}; \
             total_interest={total_interest}"
        );
    }

    /// **Monotonicity in total interest**: holding the reserve factor fixed, a
    /// larger total accrual yields a weakly larger reserve cut (and depositor
    /// share).
    #[test]
    fn reserve_split_both_parts_monotone_in_total_interest(
        interest_lo in 0i128..=SAFE_MAX_INTEREST / 2,
        interest_hi in 0i128..=SAFE_MAX_INTEREST / 2,
        reserve_factor_bps in reserve_factor_strategy(),
    ) {
        let (interest_lo, interest_hi) = if interest_lo <= interest_hi {
            (interest_lo, interest_hi)
        } else {
            (interest_hi, interest_lo)
        };

        let (dep_lo, res_lo) =
            split_interest_by_reserve_factor(interest_lo, reserve_factor_bps)
                .expect("safe inputs must not overflow");
        let (dep_hi, res_hi) =
            split_interest_by_reserve_factor(interest_hi, reserve_factor_bps)
                .expect("safe inputs must not overflow");

        prop_assert!(
            dep_hi >= dep_lo,
            "depositor monotonicity violated: \
             interest_lo={interest_lo} dep={dep_lo}, \
             interest_hi={interest_hi} dep={dep_hi}"
        );
        prop_assert!(
            res_hi >= res_lo,
            "reserve monotonicity violated: \
             interest_lo={interest_lo} res={res_lo}, \
             interest_hi={interest_hi} res={res_hi}"
        );
    }
}

// ── Edge-case / boundary proptests ───────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// **Zero reserve factor**: the entire interest amount must flow to the
    /// depositor and nothing goes to the reserve.
    #[test]
    fn reserve_split_zero_reserve_factor_all_to_depositor(
        total_interest in safe_interest_strategy(),
    ) {
        let (depositor, reserve) =
            split_interest_by_reserve_factor(total_interest, 0)
                .expect("zero rf must never overflow");

        prop_assert_eq!(depositor, total_interest);
        prop_assert_eq!(reserve, 0i128);
    }

    /// **Full reserve factor (100%)**: the entire interest amount must go to
    /// the protocol reserve; depositors receive nothing.
    #[test]
    fn reserve_split_full_reserve_factor_all_to_protocol(
        total_interest in safe_interest_strategy(),
    ) {
        let (depositor, reserve) =
            split_interest_by_reserve_factor(total_interest, BPS_SCALE)
                .expect("full rf must never overflow");

        prop_assert_eq!(reserve, total_interest);
        prop_assert_eq!(depositor, 0i128);
    }

    /// **Overflow returns a typed error**: when `total_interest` is large enough
    /// that `total_interest * reserve_factor_bps` overflows `i128`, the function
    /// must return `Err(MathError::Overflow)` rather than panicking.
    ///
    /// We exclude `reserve_factor_bps == 0` because `0 * anything == 0` and
    /// never overflows.
    #[test]
    fn reserve_split_overflow_returns_typed_error(
        total_interest in overflow_interest_strategy(),
        reserve_factor_bps in 1u32..=BPS_SCALE,
    ) {
        let result = split_interest_by_reserve_factor(total_interest, reserve_factor_bps);
        prop_assert_eq!(
            result,
            Err(MathError::Overflow),
            "expected Overflow for total_interest={total_interest} rf={reserve_factor_bps}"
        );
    }
}

// ── Deterministic unit tests (complement the proptests) ──────────────────────

#[cfg(test)]
mod unit {
    use super::super::math::{split_interest_by_reserve_factor, MathError, BPS_SCALE};

    /// Verify the canonical worked example from the doc comment.
    #[test]
    fn canonical_10pct_reserve() {
        let (dep, res) = split_interest_by_reserve_factor(1_000, 1_000).unwrap();
        assert_eq!(res, 100, "10% of 1000 = 100 to protocol");
        assert_eq!(dep, 900, "remainder 900 to depositors");
        assert_eq!(dep + res, 1_000, "no-leakage");
    }

    /// Smallest indivisible unit: 1 interest with 50% reserve factor.
    /// The floor of 0.5 is 0, so the whole unit stays with the depositor.
    #[test]
    fn single_unit_50pct_reserve_stays_with_depositor() {
        let (dep, res) = split_interest_by_reserve_factor(1, 5_000).unwrap();
        assert_eq!(res, 0, "floor(1 * 5000 / 10000) = 0");
        assert_eq!(dep, 1, "1 unit stays with depositor");
    }

    /// `total_interest == 0` returns `(0, 0)` regardless of reserve factor.
    #[test]
    fn zero_interest_always_zero_split() {
        for rf in [0u32, 1, 5_000, 9_999, BPS_SCALE] {
            let (dep, res) = split_interest_by_reserve_factor(0, rf).unwrap();
            assert_eq!((dep, res), (0, 0), "zero split for rf={rf}");
        }
    }

    /// Negative `total_interest` must return `OutOfRange`.
    #[test]
    fn negative_interest_rejected() {
        assert_eq!(
            split_interest_by_reserve_factor(-1, 0),
            Err(MathError::OutOfRange)
        );
    }

    /// `reserve_factor_bps > BPS_SCALE` must return `OutOfRange`.
    #[test]
    fn reserve_factor_above_100pct_rejected() {
        assert_eq!(
            split_interest_by_reserve_factor(1_000, BPS_SCALE + 1),
            Err(MathError::OutOfRange)
        );
    }

    /// Verify conservation at several representative BPS values.
    #[test]
    fn conservation_spot_checks() {
        let cases: &[(i128, u32)] = &[
            (7, 1),
            (1_000, 500),
            (99_999, 3_333),
            (1_000_000, 9_999),
            (10_000_000_000, 2_000),
        ];
        for &(total, rf) in cases {
            let (dep, res) = split_interest_by_reserve_factor(total, rf).unwrap();
            assert_eq!(
                dep + res,
                total,
                "conservation failed for total={total} rf={rf}"
            );
            assert!(dep >= 0 && res >= 0, "negative part for total={total} rf={rf}");
        }
    }
}
