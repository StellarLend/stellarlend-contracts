#![cfg(test)]

use super::math::{compute_liquidation_bonus, compute_max_borrow, MathError, BPS_SCALE};
use proptest::prelude::*;

/// Restrict the property suite to inputs that can be exercised without
/// hitting the checked-multiply overflow path in the bonus helper.
fn safe_debt_strategy() -> impl Strategy<Value = i128> {
    0i128..=i128::MAX / 10_000
}

/// Focus overflow cases on values that are guaranteed to overflow the
/// multiplication in `compute_liquidation_bonus` when the bonus rate is non-zero.
fn overflow_debt_strategy() -> impl Strategy<Value = i128> {
    (i128::MAX / 10_000 + 1)..=i128::MAX
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn liquidation_bonus_is_non_negative_and_bounded_by_debt(
        debt_to_cover in safe_debt_strategy(),
        liquidation_bonus_bps in 0u32..=BPS_SCALE,
    ) {
        let bonus = compute_liquidation_bonus(debt_to_cover, liquidation_bonus_bps)
            .expect("safe inputs should not overflow");
        assert!(bonus >= 0, "bonus must stay non-negative");
        assert!(bonus <= debt_to_cover, "bonus must not exceed debt coverage");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn liquidation_bonus_is_monotonic_in_debt_to_cover(
        debt_a in safe_debt_strategy(),
        debt_b in safe_debt_strategy(),
        liquidation_bonus_bps in 0u32..=BPS_SCALE,
    ) {
        let bonus_a = compute_liquidation_bonus(debt_a, liquidation_bonus_bps)
            .expect("safe inputs should not overflow");
        let bonus_b = compute_liquidation_bonus(debt_b, liquidation_bonus_bps)
            .expect("safe inputs should not overflow");

        if debt_a <= debt_b {
            assert!(bonus_a <= bonus_b, "bonus should not decrease as debt rises");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn liquidation_bonus_stays_within_max_borrow_bound(
        collateral_value in safe_debt_strategy(),
        ltv_bps in 0u32..=BPS_SCALE,
        liquidation_bonus_bps in 0u32..=BPS_SCALE,
    ) {
        let max_borrow = compute_max_borrow(collateral_value, ltv_bps)
            .expect("safe inputs should not overflow");
        let bonus = compute_liquidation_bonus(max_borrow, liquidation_bonus_bps)
            .expect("safe inputs should not overflow");

        assert!(bonus <= max_borrow, "liquidation bonus must stay within the borrow cap");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn liquidation_bonus_overflow_returns_math_error(
        debt_to_cover in overflow_debt_strategy(),
        liquidation_bonus_bps in 1u32..=BPS_SCALE,
    ) {
        let result = compute_liquidation_bonus(debt_to_cover, liquidation_bonus_bps);
        assert!(
            matches!(result, Err(MathError::Overflow)),
            "overflowing multiply should return MathError::Overflow, got {:?}",
            result
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn max_borrow_overflow_returns_math_error(
        collateral_value in overflow_debt_strategy(),
        ltv_bps in 1u32..=BPS_SCALE,
    ) {
        let result = compute_max_borrow(collateral_value, ltv_bps);
        assert!(
            matches!(result, Err(MathError::Overflow)),
            "overflowing multiply should return MathError::Overflow, got {:?}",
            result
        );
    }
}
