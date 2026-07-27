#![cfg(test)]

extern crate std;

use super::math::{checked_mul_div_floor, MathError};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed};

/// Number of generated cases per property.
const MAX_BORROW_PROPTEST_CASES: u32 = 256;

/// Fixed seed so CI failures are deterministically reproducible.
const MAX_BORROW_PROPTEST_SEED: u64 = 0xB0BBAB0B0B0BFEED;

fn seeded_config() -> ProptestConfig {
    ProptestConfig {
        cases: MAX_BORROW_PROPTEST_CASES,
        rng_seed: RngSeed::Fixed(MAX_BORROW_PROPTEST_SEED),
        ..ProptestConfig::default()
    }
}

/// Pure helper: mirrors the on-contract formula
///   max_borrow = floor(collateral_value * ltv_bps / 10_000)
/// returning Err on invalid inputs (negative collateral, ltv_bps > 10_000,
/// or intermediate overflow).
fn compute_max_borrow(collateral_value: i128, ltv_bps: i128) -> Result<i128, MathError> {
    if collateral_value < 0 {
        return Err(MathError::OutOfRange);
    }
    if ltv_bps < 0 || ltv_bps > 10_000 {
        return Err(MathError::OutOfRange);
    }
    checked_mul_div_floor(collateral_value, ltv_bps, 10_000)
}

proptest! {
    #![proptest_config(seeded_config())]

    /// Solvency bound: max_borrow must never exceed collateral_value.
    #[test]
    fn solvency_bound(
        collateral in 0i128..=i128::MAX / 10_001,
        ltv in 0i128..=10_000i128,
    ) {
        let result = compute_max_borrow(collateral, ltv);
        prop_assert!(result.is_ok(), "unexpected error: {:?}", result);
        let max_borrow = result.unwrap();
        prop_assert!(
            max_borrow >= 0,
            "max_borrow {} < 0 for collateral={} ltv={}",
            max_borrow, collateral, ltv
        );
        prop_assert!(
            max_borrow <= collateral,
            "max_borrow {} > collateral {} for ltv={}",
            max_borrow, collateral, ltv
        );
    }

    /// Exact formula: result equals floor(collateral * ltv_bps / 10_000).
    #[test]
    fn exact_formula(
        collateral in 0i128..=i128::MAX / 10_001,
        ltv in 0i128..=10_000i128,
    ) {
        let result = compute_max_borrow(collateral, ltv);
        prop_assert!(result.is_ok());
        let expected = collateral * ltv / 10_000; // safe: bounded inputs
        prop_assert_eq!(result.unwrap(), expected);
    }

    /// LTV monotonicity: higher ltv_bps must never lower the borrow cap.
    #[test]
    fn ltv_monotonicity(
        collateral in 0i128..=i128::MAX / 10_001,
        ltv_lo in 0i128..=9_999i128,
        delta in 1i128..=10_000i128,
    ) {
        let ltv_hi = (ltv_lo + delta).min(10_000);
        let lo = compute_max_borrow(collateral, ltv_lo).unwrap();
        let hi = compute_max_borrow(collateral, ltv_hi).unwrap();
        prop_assert!(
            hi >= lo,
            "max_borrow decreased as ltv rose: lo={} (ltv={}) hi={} (ltv={})",
            lo, ltv_lo, hi, ltv_hi
        );
    }
}

// ── Boundary / error pinning ─────────────────────────────────────────────────

#[test]
fn ltv_zero_returns_zero() {
    assert_eq!(compute_max_borrow(1_000_000, 0), Ok(0));
}

#[test]
fn ltv_10000_returns_full_collateral() {
    let collateral = 1_000_000i128;
    assert_eq!(compute_max_borrow(collateral, 10_000), Ok(collateral));
}

#[test]
fn zero_collateral_returns_zero() {
    assert_eq!(compute_max_borrow(0, 5_000), Ok(0));
}

#[test]
fn negative_collateral_is_out_of_range() {
    assert_eq!(compute_max_borrow(-1, 5_000), Err(MathError::OutOfRange));
}

#[test]
fn ltv_above_10000_is_out_of_range() {
    assert_eq!(
        compute_max_borrow(1_000_000, 10_001),
        Err(MathError::OutOfRange)
    );
}

#[test]
fn negative_ltv_is_out_of_range() {
    assert_eq!(
        compute_max_borrow(1_000_000, -1),
        Err(MathError::OutOfRange)
    );
}
