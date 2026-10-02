//! Unit tests for the crate-private `compute_fee` swap-fee helper.
//!
//! `compute_fee(amount_in, fee_bps)` computes `amount_in * fee_bps / 10_000`
//! with checked multiplication and returns `Err(AmmPoolError::Overflow)` when
//! the product does not fit in an `i128`.  It is the single fee helper shared
//! by `swap_a_for_b`, `swap_b_for_a`, and the quoting path, so a regression
//! here silently mis-prices every swap in the pool.
//!
//! The happy-path formula is exercised indirectly through `get_swap_quote`
//! (`swap_quote_test::test_quoted_fee_matches_compute_fee_*`), but nothing
//! called the helper directly, and the overflow branch had no coverage at all:
//! no test drove `amount_in * fee_bps` past `i128::MAX`.  The fact that the
//! product is reported as `Err(AmmPoolError::Overflow)` — rather than
//! panicking or wrapping — was therefore unpinned.  These tests pin it, and
//! also correct the `compute_fee` doc comment that claimed the helper "panics
//! on overflow".
//!
//! | Invariant                                             | Test function                            |
//! |-------------------------------------------------------|------------------------------------------|
//! | `fee = amount_in * fee_bps / 10_000`                  | `test_fee_is_product_over_ten_thousand`   |
//! | Zero input amount yields a zero fee                   | `test_zero_amount_yields_zero_fee`        |
//! | Zero `fee_bps` yields a zero fee                      | `test_zero_fee_bps_yields_zero_fee`       |
//! | `MAX_FEE_BPS` (50 %) charges exactly half             | `test_max_fee_bps_charges_half`           |
//! | Integer division truncates toward zero                | `test_truncates_toward_zero`              |
//! | Largest representable product succeeds                | `test_largest_representable_product_ok`   |
//! | One step past the boundary returns `Overflow`         | `test_overflow_at_boundary_returns_err`   |
//! | Overflowing product is an error, never a panic        | `test_overflow_is_error_not_panic`        |
//! | Fee is non-decreasing in `amount_in`                  | `test_monotonic_in_amount`                |
//! | Fee is non-decreasing in `fee_bps`                    | `test_monotonic_in_fee_bps`               |

use crate::{compute_fee, AmmPoolError, MAX_FEE_BPS};

/// The denominator that turns basis points into a fraction.
const FEE_DENOMINATOR: i128 = 10_000;

#[test]
fn test_fee_is_product_over_ten_thousand() {
    // 1_000_000 * 30 / 10_000 = 3_000
    assert_eq!(compute_fee(1_000_000, 30), Ok(3_000));
    // 250_000 * 100 / 10_000 = 2_500
    assert_eq!(compute_fee(250_000, 100), Ok(2_500));
    // 42 * 10_000 = 420_000, / 10_000 = 42 (a full-fee round trip).
    assert_eq!(compute_fee(42, FEE_DENOMINATOR), Ok(42));
}

#[test]
fn test_zero_amount_yields_zero_fee() {
    assert_eq!(compute_fee(0, 30), Ok(0));
    assert_eq!(compute_fee(0, MAX_FEE_BPS), Ok(0));
    assert_eq!(compute_fee(0, FEE_DENOMINATOR), Ok(0));
}

#[test]
fn test_zero_fee_bps_yields_zero_fee() {
    assert_eq!(compute_fee(1_000_000, 0), Ok(0));
    // A zero fee must not overflow even against a saturated amount.
    assert_eq!(compute_fee(i128::MAX, 0), Ok(0));
}

#[test]
fn test_max_fee_bps_charges_half() {
    // `MAX_FEE_BPS` is 5_000 bps = 50 %; guard the constant still holds so
    // this test cannot silently stop testing the real ceiling.
    assert_eq!(MAX_FEE_BPS, 5_000);
    assert_eq!(compute_fee(1_000_000, MAX_FEE_BPS), Ok(500_000));
    // 1 * 5_000 = 5_000, which floors to 0 units below the denominator.
    assert_eq!(compute_fee(1, MAX_FEE_BPS), Ok(0));
}

#[test]
fn test_truncates_toward_zero() {
    // Remainders below one fee unit are dropped, never rounded up.
    assert_eq!(compute_fee(9_999, 1), Ok(0));
    assert_eq!(compute_fee(10_000, 1), Ok(1));
    assert_eq!(compute_fee(19_999, 1), Ok(1));
    assert_eq!(compute_fee(20_000, 1), Ok(2));
    assert_eq!(compute_fee(1, 9_999), Ok(0));
}

/// The largest `amount_in` whose product with `fee_bps` still fits in `i128`
/// must succeed and must return exactly the floored quotient.
#[test]
fn test_largest_representable_product_ok() {
    let max_amount = i128::MAX / FEE_DENOMINATOR;
    assert_eq!(compute_fee(max_amount, FEE_DENOMINATOR), Ok(max_amount));

    // For a 50 % fee the largest product is at `i128::MAX / MAX_FEE_BPS`, and
    // the fee itself is then half of that amount, not the whole amount.
    let max_amount = i128::MAX / MAX_FEE_BPS;
    assert_eq!(
        compute_fee(max_amount, MAX_FEE_BPS),
        Ok(max_amount * MAX_FEE_BPS / FEE_DENOMINATOR)
    );

    // Multiplying by 1 never overflows, so the whole `i128` input range is
    // usable when the fee is one basis point.
    assert_eq!(compute_fee(i128::MAX, 1), Ok(i128::MAX / FEE_DENOMINATOR));
}

/// One unit past the boundary overflows, and the helper must say so rather
/// than wrap around or abort.
#[test]
fn test_overflow_at_boundary_returns_err() {
    let max_amount = i128::MAX / FEE_DENOMINATOR;
    assert_eq!(
        compute_fee(max_amount + 1, FEE_DENOMINATOR),
        Err(AmmPoolError::Overflow)
    );

    let max_amount = i128::MAX / MAX_FEE_BPS;
    assert_eq!(
        compute_fee(max_amount + 1, MAX_FEE_BPS),
        Err(AmmPoolError::Overflow)
    );

    // A syntactically tiny `fee_bps` still overflows against a saturated
    // amount, so the guard cannot be keyed on the fee alone.
    assert_eq!(compute_fee(i128::MAX, 2), Err(AmmPoolError::Overflow));
}

/// Regression guard for issue #1979: the helper's declared return type is
/// `Result<i128, AmmPoolError>`, so an overflowing product must be reported as
/// `Err(AmmPoolError::Overflow)` — not as a bare `i128`, not as a panic, and
/// not as a wrapped value.
#[test]
fn test_overflow_is_error_not_panic() {
    let result: Result<i128, AmmPoolError> = compute_fee(i128::MAX, FEE_DENOMINATOR);
    assert_eq!(result, Err(AmmPoolError::Overflow));

    // Sanity: the product really is unrepresentable, so the `Err` above is a
    // property of the inputs and not an artefact of the helper.
    assert!(i128::MAX.checked_mul(FEE_DENOMINATOR).is_none());
}

#[test]
fn test_monotonic_in_amount() {
    let fee_bps = 30_i128;
    let mut previous = 0_i128;
    let mut amount = 0_i128;
    // A stride co-prime to the denominator walks many remainders.
    while amount < 1_000_000 {
        let fee = compute_fee(amount, fee_bps).expect("in-range amount must not overflow");
        assert!(fee >= previous, "fee decreased at amount={amount}");
        previous = fee;
        amount += 7_919;
    }
}

#[test]
fn test_monotonic_in_fee_bps() {
    let amount = 1_000_000_i128;
    let mut previous = 0_i128;
    let mut fee_bps = 0_i128;
    while fee_bps <= MAX_FEE_BPS {
        let fee = compute_fee(amount, fee_bps).expect("in-range fee_bps must not overflow");
        assert!(fee >= previous, "fee decreased at fee_bps={fee_bps}");
        previous = fee;
        fee_bps += 1;
    }
}
