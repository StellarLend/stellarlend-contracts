#![cfg(test)]

extern crate std;

use super::math::{compute_health_factor, MathError, BPS_SCALE, SCALE};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Number of generated cases per property.
const HEALTH_FACTOR_PROPTEST_CASES: u32 = 256;

/// Fixed proptest seed so reviewer and CI failures replay deterministically.
const HEALTH_FACTOR_PROPTEST_SEED: u64 = 0x5EED_4E41_7AFA_C70D;

/// Largest collateral/debt input that cannot overflow the health-factor formula
/// when paired with any valid liquidation threshold.
const SAFE_VALUE_MAX: i128 = i128::MAX / SCALE;

/// Returns the bounded, seeded proptest configuration for this invariant suite.
fn seeded_config() -> ProptestConfig {
    ProptestConfig {
        cases: HEALTH_FACTOR_PROPTEST_CASES,
        rng_seed: RngSeed::Fixed(HEALTH_FACTOR_PROPTEST_SEED),
        ..ProptestConfig::default()
    }
}

/// Generates non-negative collateral or debt values that keep all intermediate
/// products in `compute_health_factor` inside `i128` bounds.
fn safe_value_strategy() -> impl Strategy<Value = i128> {
    0i128..=SAFE_VALUE_MAX
}

/// Generates positive debt denominators for monotonicity and formula checks.
fn positive_safe_debt_strategy() -> impl Strategy<Value = i128> {
    1i128..=SAFE_VALUE_MAX
}

/// Generates collateral values that must overflow the first checked multiply
/// when the liquidation threshold is 100%.
fn first_multiply_overflow_collateral_strategy() -> impl Strategy<Value = i128> {
    (i128::MAX / BPS_SCALE as i128 + 1)..=i128::MAX
}

proptest! {
    #![proptest_config(seeded_config())]

    #[test]
    fn health_factor_matches_documented_formula_for_safe_inputs(
        collateral_value in safe_value_strategy(),
        debt_value in positive_safe_debt_strategy(),
        liquidation_threshold_bps in 0u32..=BPS_SCALE,
    ) {
        let weighted_collateral = collateral_value
            .checked_mul(liquidation_threshold_bps as i128)
            .expect("safe strategy keeps threshold multiply in range")
            / BPS_SCALE as i128;
        let expected = weighted_collateral
            .checked_mul(SCALE)
            .expect("safe strategy keeps scale multiply in range")
            / debt_value;

        prop_assert_eq!(
            compute_health_factor(collateral_value, debt_value, liquidation_threshold_bps),
            Ok(expected)
        );
    }

    #[test]
    fn health_factor_is_monotonic_in_collateral(
        collateral_a in safe_value_strategy(),
        collateral_b in safe_value_strategy(),
        debt_value in positive_safe_debt_strategy(),
        liquidation_threshold_bps in 0u32..=BPS_SCALE,
    ) {
        let lower_collateral = collateral_a.min(collateral_b);
        let higher_collateral = collateral_a.max(collateral_b);

        let lower_hf = compute_health_factor(
            lower_collateral,
            debt_value,
            liquidation_threshold_bps,
        ).expect("safe collateral inputs should not overflow");
        let higher_hf = compute_health_factor(
            higher_collateral,
            debt_value,
            liquidation_threshold_bps,
        ).expect("safe collateral inputs should not overflow");

        prop_assert!(
            higher_hf >= lower_hf,
            "HF must not decrease when collateral rises: lower={} higher={}",
            lower_hf,
            higher_hf,
        );
    }

    #[test]
    fn health_factor_is_inverse_monotonic_in_debt(
        collateral_value in safe_value_strategy(),
        debt_a in positive_safe_debt_strategy(),
        debt_b in positive_safe_debt_strategy(),
        liquidation_threshold_bps in 0u32..=BPS_SCALE,
    ) {
        let lower_debt = debt_a.min(debt_b);
        let higher_debt = debt_a.max(debt_b);

        let lower_debt_hf = compute_health_factor(
            collateral_value,
            lower_debt,
            liquidation_threshold_bps,
        ).expect("safe debt inputs should not overflow");
        let higher_debt_hf = compute_health_factor(
            collateral_value,
            higher_debt,
            liquidation_threshold_bps,
        ).expect("safe debt inputs should not overflow");

        prop_assert!(
            lower_debt_hf >= higher_debt_hf,
            "HF must not increase when debt rises: lower_debt_hf={} higher_debt_hf={}",
            lower_debt_hf,
            higher_debt_hf,
        );
    }

    #[test]
    fn no_debt_returns_saturated_max_for_valid_inputs(
        collateral_value in 0i128..=i128::MAX,
        liquidation_threshold_bps in 0u32..=BPS_SCALE,
    ) {
        prop_assert_eq!(
            compute_health_factor(collateral_value, 0, liquidation_threshold_bps),
            Ok(i128::MAX)
        );
    }

    #[test]
    fn health_factor_never_panics_and_reports_typed_errors(
        collateral_value in any::<i128>(),
        debt_value in any::<i128>(),
        liquidation_threshold_bps in any::<u32>(),
    ) {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            compute_health_factor(collateral_value, debt_value, liquidation_threshold_bps)
        }));

        prop_assert!(outcome.is_ok(), "compute_health_factor must not panic");
        let result = outcome.expect("panic already asserted absent");

        if collateral_value < 0 || debt_value < 0 || liquidation_threshold_bps > BPS_SCALE {
            prop_assert_eq!(result, Err(MathError::OutOfRange));
        } else if debt_value == 0 {
            prop_assert_eq!(result, Ok(i128::MAX));
        } else {
            prop_assert!(
                matches!(result, Ok(_) | Err(MathError::Overflow)),
                "valid non-zero-debt inputs should produce HF or typed overflow, got {:?}",
                result,
            );
        }
    }

    #[test]
    fn overflowing_collateral_multiply_returns_typed_error(
        collateral_value in first_multiply_overflow_collateral_strategy(),
        debt_value in 1i128..=i128::MAX,
    ) {
        prop_assert_eq!(
            compute_health_factor(collateral_value, debt_value, BPS_SCALE),
            Err(MathError::Overflow)
        );
    }
}

/// Covers the second checked multiplication overflow path where collateral can
/// still be weighted successfully but scaling the weighted collateral cannot.
#[test]
fn scaling_weighted_collateral_overflow_returns_typed_error() {
    let collateral_value = i128::MAX / SCALE + 1;

    assert_eq!(
        compute_health_factor(collateral_value, 1, BPS_SCALE),
        Err(MathError::Overflow)
    );
}

/// Pins liquidation threshold boundaries: 0 bps zeroes HF, while 100% makes the
/// result equal to `collateral * SCALE / debt` for safe inputs.
#[test]
fn threshold_boundaries_are_exact() {
    assert_eq!(compute_health_factor(10_000, 5_000, 0), Ok(0));
    assert_eq!(
        compute_health_factor(10_000, 5_000, BPS_SCALE),
        Ok(2 * SCALE)
    );
    assert_eq!(
        compute_health_factor(10_000, 5_000, BPS_SCALE + 1),
        Err(MathError::OutOfRange)
    );
}

/// Pins the most extreme overflow input requested by the invariant issue.
#[test]
fn i128_max_collateral_overflow_returns_typed_error() {
    assert_eq!(
        compute_health_factor(i128::MAX, 1, BPS_SCALE),
        Err(MathError::Overflow)
    );
}
