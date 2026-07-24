extern crate std;

use super::math::{compute_max_borrow, MathError, BPS_SCALE};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Number of generated cases per max-borrow property.
const MAX_BORROW_PROPTEST_CASES: u32 = 512;

/// Fixed proptest seed so CI failures can be replayed deterministically.
const MAX_BORROW_PROPTEST_SEED: u64 = 0x5EED_4D41_5842_4F52;

/// Largest collateral value whose `collateral * BPS_SCALE` product cannot
/// overflow an `i128`.
const SAFE_COLLATERAL_MAX: i128 = i128::MAX / BPS_SCALE as i128;

/// Returns the bounded, seeded proptest configuration for this invariant suite.
fn seeded_config() -> ProptestConfig {
    ProptestConfig {
        cases: MAX_BORROW_PROPTEST_CASES,
        rng_seed: RngSeed::Fixed(MAX_BORROW_PROPTEST_SEED),
        ..ProptestConfig::default()
    }
}

/// Generates collateral values that keep every valid LTV multiply in range.
fn safe_collateral_strategy() -> impl Strategy<Value = i128> {
    0i128..=SAFE_COLLATERAL_MAX
}

/// Generates collateral values that must overflow when multiplied by 100% LTV.
fn overflow_collateral_strategy() -> impl Strategy<Value = i128> {
    (SAFE_COLLATERAL_MAX + 1)..=i128::MAX
}

proptest! {
    #![proptest_config(seeded_config())]

    #[test]
    fn max_borrow_matches_documented_floor_formula_for_safe_inputs(
        collateral_value in safe_collateral_strategy(),
        ltv_bps in 0u32..=BPS_SCALE,
    ) {
        let expected = collateral_value
            .checked_mul(ltv_bps as i128)
            .expect("safe strategy keeps LTV multiply in range")
            / BPS_SCALE as i128;

        prop_assert_eq!(
            compute_max_borrow(collateral_value, ltv_bps),
            Ok(expected)
        );
    }

    #[test]
    fn max_borrow_never_exceeds_collateral_for_valid_ltv(
        collateral_value in safe_collateral_strategy(),
        ltv_bps in 0u32..=BPS_SCALE,
    ) {
        let max_borrow = compute_max_borrow(collateral_value, ltv_bps)
            .expect("safe inputs should not overflow");

        prop_assert!(
            max_borrow <= collateral_value,
            "max borrow must not exceed collateral: borrow={} collateral={} ltv_bps={}",
            max_borrow,
            collateral_value,
            ltv_bps,
        );
        prop_assert!(max_borrow >= 0, "max borrow must stay non-negative");
    }

    #[test]
    fn max_borrow_is_monotonic_in_ltv(
        collateral_value in safe_collateral_strategy(),
        ltv_a in 0u32..=BPS_SCALE,
        ltv_b in 0u32..=BPS_SCALE,
    ) {
        let lower_ltv = ltv_a.min(ltv_b);
        let higher_ltv = ltv_a.max(ltv_b);

        let lower_borrow = compute_max_borrow(collateral_value, lower_ltv)
            .expect("safe lower LTV input should not overflow");
        let higher_borrow = compute_max_borrow(collateral_value, higher_ltv)
            .expect("safe higher LTV input should not overflow");

        prop_assert!(
            higher_borrow >= lower_borrow,
            "max borrow must not decrease as LTV rises: lower={} higher={}",
            lower_borrow,
            higher_borrow,
        );
    }

    #[test]
    fn max_borrow_never_panics_and_reports_typed_errors(
        collateral_value in any::<i128>(),
        ltv_bps in any::<u32>(),
    ) {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            compute_max_borrow(collateral_value, ltv_bps)
        }));

        prop_assert!(outcome.is_ok(), "compute_max_borrow must not panic");
        let result = outcome.expect("panic already asserted absent");

        if collateral_value < 0 || ltv_bps > BPS_SCALE {
            prop_assert_eq!(result, Err(MathError::OutOfRange));
        } else {
            prop_assert!(
                matches!(result, Ok(_) | Err(MathError::Overflow)),
                "valid inputs should produce max borrow or typed overflow, got {:?}",
                result,
            );
        }
    }

    #[test]
    fn full_ltv_overflow_returns_typed_error(
        collateral_value in overflow_collateral_strategy(),
    ) {
        prop_assert_eq!(
            compute_max_borrow(collateral_value, BPS_SCALE),
            Err(MathError::Overflow)
        );
    }
}

/// Pins the basis-point boundaries used by the solvency invariant.
#[test]
fn ltv_boundaries_are_exact() {
    assert_eq!(compute_max_borrow(123_456, 0), Ok(0));
    assert_eq!(compute_max_borrow(123_456, BPS_SCALE), Ok(123_456));
    assert_eq!(compute_max_borrow(0, BPS_SCALE), Ok(0));
    assert_eq!(compute_max_borrow(1, BPS_SCALE - 1), Ok(0));
}

/// Rejects invalid inputs before any arithmetic is attempted.
#[test]
fn invalid_inputs_return_out_of_range() {
    assert_eq!(compute_max_borrow(-1, 0), Err(MathError::OutOfRange));
    assert_eq!(
        compute_max_borrow(1, BPS_SCALE + 1),
        Err(MathError::OutOfRange)
    );
}

/// Covers the largest possible collateral value on the overflow path.
#[test]
fn i128_max_collateral_at_full_ltv_returns_overflow() {
    assert_eq!(
        compute_max_borrow(i128::MAX, BPS_SCALE),
        Err(MathError::Overflow)
    );
}