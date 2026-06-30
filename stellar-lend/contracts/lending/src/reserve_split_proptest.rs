//! Proptest invariants for reserve interest splitting.
//!
//! This module contains property-based tests for `split_interest_by_reserve_factor`
//! that enforce invariants such as value conservation, non-negativity, and proper
//! handling of edge cases like zero inputs and rounding.
//!
//! The invariants ensure that the mathematical properties of interest splitting
//! are preserved across the entire input domain, providing stronger guarantees
//! than any finite set of unit tests.
//!
//! # Key Invariants
//!
//! 1. **Value Conservation** - The sum of depositor and protocol parts must
//!    equal the total interest.
//!
//! 2. **Non‑negativity** - Neither part may be negative.
//!
//! 3. **Conservative Rounding** - Fractional units always fall to the
//!    depositor side via integer floor division.
//!
//! 4. **Typed Error Handling** - Overflow and range errors return `MathError` variants
//!    rather than panicking.
//!
//! 5. **Edge‑case Coverage** - All special cases (zero inputs, max values)
//!    are explicitly exercised.
//!
//! These invariants are crucial because any violation would break accounting
//! consistency or expose the protocol to unexpected loss.
//!
//! Invariants are exercised using `proptest`, a property‑based testing framework
//! that generates random input cases within well‑defined boundaries to provide
//! high‑coverage, repeatable verification that the invariants hold everywhere.
//!
//! # Running the Tests
//!
//! Tests are automatically included in the test suite and can be run via:
//!
//! ```bash
//! cargo test reserve_split_proptest
//! ```
//!
//! They run quickly and exercise no‑core contracts, making them ideal for CI and
//! edge‑case detection.

#![cfg(test)]

use super::math::split_interest_by_reserve_factor;
use proptest::prelude::*;

/// Strategy for total_interest: non‑negative values that are safe to multiply.
fn arb_total_interest_safe() -> impl Strategy<Value = i128> {
    0i128..=i128::MAX / 10_000
}

/// Overflow strategy: force overflow in total_interest * reserve_factor_bps.
fn arb_total_interest_overflow() -> impl Strategy<Value = i128> {
    (i128::MAX / 10_000 + 1)..=i128::MAX
}

/// Reserve factor strategy: valid basis points range 0‑10_000 (0‑100%).
fn arb_reserve_factor_bps() -> impl Strategy<Value = u32> {
    0u32..=10_000
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(1024))]

    /// Conservation invariant: depositor_yield + reserve_cut == total_interest.
    ///
    /// Ensures the mathematical split preserves the total value exactly.
    #[test]
    fn prop_split_conservation(
        total_interest in arb_total_interest_safe(),
        reserve_factor_bps in arb_reserve_factor_bps(),
    ) {
        let (depositor_yield, reserve_cut) = split_interest_by_reserve_factor(
            total_interest, reserve_factor_bps,
        ).expect("valid inputs should not fail");
        assert_eq!(
            depositor_yield + reserve_cut,
            total_interest,
            "total interest not conserved: {} + {} != {}",
            depositor_yield, reserve_cut, total_interest
        );
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(1024))]

    /// Non‑negativity invariant: both split parts must be ≥ 0.
    ///
    /// Rejection of negative values guarantees accounting integrity.
    #[test]
    fn prop_split_non_negative(
        total_interest in arb_total_interest_safe(),
        reserve_factor_bps in arb_reserve_factor_bps(),
    ) {
        let (depositor_yield, reserve_cut) = split_interest_by_reserve_factor(
            total_interest, reserve_factor_bps,
        ).expect("valid inputs should not fail");
        assert!(depositor_yield >= 0, "negative depositor yield: {}", depositor_yield);
        assert!(reserve_cut >= 0, "negative reserve cut: {}", reserve_cut);
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(512))]

    /// Conservative rounding: fractional unit falls to depositor (floor division).
    ///
    /// The protocol never takes more than its exact share; remainder always lands
    /// with depositors as per the function's contract.
    #[test]
    fn prop_split_rounding_conservative(
        total_interest in arb_total_interest_safe(),
        reserve_factor_bps in arb_reserve_factor_bps(),
    ) {
        let (depositor_yield, reserve_cut) = split_interest_by_reserve_factor(
            total_interest, reserve_factor_bps,
        ).expect("valid inputs should not fail");
        let scale = 10_000i128;
        let exact_share = (total_interest)
            .checked_mul(reserve_factor_bps as i128)
            .unwrap()
            .checked_div(scale)
            .unwrap();
        assert_eq!(
            reserve_cut, exact_share,
            "reserve cut should be floor(total * rf / 10_000"
        );
        assert!(reserve_cut <= exact_share + 1, "reserve cut should never exceed exact share by more than 1");
        assert!(reserve_cut >= exact_share || (total_interest == 0 && reserve_factor_bps == 0), "reserve cut should be either exact or exact-1");
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(512))]

    /// Overflow handling: returning MathError::Overflow.
    ///
    /// Ensures that extreme values are caught and reported typed rather than
    /// causing silent overflow or panics.
    #[test]
    fn prop_split_overflow_returns_error(
        total_interest in arb_total_interest_overflow(),
        reserve_factor_bps in 1001u32..=10_000,
    ) {
        let result = split_interest_by_reserve_factor(total_interest, reserve_factor_bps);
        match result {
            Ok(_) => panic!( "overflow inputs should return MathError::Overflow" ),
            Err(e) => assert_eq!(e, super::math::MathError::Overflow, "expected Overflow error"),
        }
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(256))]

    /// Zero interest invariants: both parts zero.
    ///
    /// Regardless of reserve factor, zero total interest always produces zero split.
    #[test]
    fn prop_split_zero_interest() {
        let total_interest = 0i128;
        prop_for_all!(
            |rf_bps: u32| {
                let (depositor_yield, reserve_cut) = split_interest_by_reserve_factor(
                    total_interest, rf_bps,
                ).expect("zero interest should always succeed");
                assert_eq!(depositor_yield, 0, "depositor yield should be 0");
                assert_eq!(reserve_cut, 0, "reserve cut should be 0");
            }
        );
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(256))]

    /// Zero reserve factor: all interest to depositors.
    ///
    /// With rf_bps == 0, protocol share is exactly zero.
    #[test]
    fn prop_split_zero_reserve_factor() {
        prop_for_all!(
            |total_interest: i128| {
                let (depositor_yield, reserve_cut) = split_interest_by_reserve_factor(
                    total_interest, 0,
                ).expect("zero reserve factor should always succeed");
                assert_eq!(
                    depositor_yield,
                    total_interest,
                    "zero reserve factor should send all to depositors"
                );
                assert_eq!(reserve_cut, 0, "zero reserve factor should leave reserve unchanged");
            }
        );
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(256))]

    /// 100% reserve factor: all interest to protocol.
    ///
    /// With rf_bps == 10_000, depositors receive nothing.
    #[test]
    fn prop_split_full_reserve_factor() {
        prop_for_all!(
            |total_interest: i128| {
                let (depositor_yield, reserve_cut) = split_interest_by_reserve_factor(
                    total_interest, 10_000,
                ).expect("full reserve factor should always succeed");
                assert_eq!(depositor_yield, 0, "full reserve factor should leave depositors empty");
                assert_eq!(
                    reserve_cut,
                    total_interest,
                    "full reserve factor should send all to reserve"
                );
            }
        );
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(256))]

    /// Negative interest rejection: MathError::OutOfRange.
    ///
    /// Inputs < 0 are explicitly rejected as out of range.
    #[test]
    fn prop_split_negative_interest_rejected() {
        let total_interest = -1i128;
        prop_for_all!(
            |rf_bps: u32| {
                let result = split_interest_by_reserve_factor(total_interest, rf_bps);
                assert_eq!(result, Err(super::math::MathError::OutOfRange));
            }
        );
    }
}

proptest! {
    #[proptest_config(ProptestConfig::with_cases(256))]

    /// Reserve factor >100% rejection: MathError::OutOfRange.
    ///
    /// rf_bps > 10_000 is never accepted.
    #[test]
    fn prop_split_reserve_factor_above_100pc_rejected() {
        prop_for_all!(
            |total_interest: i128| {
                let result = split_interest_by_reserve_factor(total_interest, 10_001);
                assert_eq!(result, Err(super::math::MathError::OutOfRange));
            }
        );
    }
}
