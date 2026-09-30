extern crate std;

use crate::math::{compute_compound_interest, MathError, MAX_RATE_BPS};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, RngSeed};
use std::panic::{catch_unwind, AssertUnwindSafe};

const PROPTEST_CASES: u32 = 256;
const PROPTEST_SEED: u64 = 0x5EED_C01A;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: PROPTEST_CASES,
        rng_seed: RngSeed::Fixed(PROPTEST_SEED),
        ..ProptestConfig::default()
    }
}

/// Bound `principal` so that `principal * MAX_RATE_BPS * elapsed` cannot
/// overflow i128 for any `elapsed` used by these proptests (all `< 2_000_000`).
fn safe_principal() -> impl Strategy<Value = i128> {
    0i128..=(i128::MAX / (MAX_RATE_BPS * 2_000_000))
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn monotonic_in_elapsed(
        principal in safe_principal(),
        rate in 0i128..=MAX_RATE_BPS,
        e1 in 0u64..1_000_000,
        e2 in 1_000_001u64..2_000_000,
    ) {
        let a = compute_compound_interest(principal, rate, e1).unwrap();
        let b = compute_compound_interest(principal, rate, e2).unwrap();
        prop_assert!(b >= a);
    }

    #[test]
    fn monotonic_in_rate(
        principal in safe_principal(),
        elapsed in 0u64..2_000_000,
        r1 in 0i128..50_000,
        r2 in 50_001i128..=MAX_RATE_BPS,
    ) {
        let a = compute_compound_interest(principal, r1, elapsed).unwrap();
        let b = compute_compound_interest(principal, r2, elapsed).unwrap();
        prop_assert!(b >= a);
    }

    #[test]
    fn never_negative(
        principal in safe_principal(),
        rate in 0i128..=MAX_RATE_BPS,
        elapsed in 0u64..2_000_000,
    ) {
        let interest = compute_compound_interest(principal, rate, elapsed).unwrap();
        prop_assert!(interest >= 0);
    }

    #[test]
    fn never_panics(
        principal in any::<i128>(),
        rate in any::<i128>(),
        elapsed in any::<u64>(),
    ) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            compute_compound_interest(principal, rate, elapsed)
        }));

        prop_assert!(result.is_ok());

        if let Ok(value) = result {
            prop_assert!(matches!(
                value,
                Ok(_)
                    | Err(MathError::OutOfRange)
                    | Err(MathError::Overflow)
                    | Err(MathError::DivisionByZero)
            ));
        }
    }
}

#[test]
fn zero_elapsed_returns_zero() {
    assert_eq!(compute_compound_interest(1000, 5000, 0), Ok(0));
}

#[test]
fn zero_rate_returns_zero() {
    assert_eq!(compute_compound_interest(1000, 0, 1000), Ok(0));
}

#[test]
fn invalid_rate_returns_error() {
    assert_eq!(
        compute_compound_interest(1000, MAX_RATE_BPS + 1, 1),
        Err(MathError::OutOfRange)
    );
}
