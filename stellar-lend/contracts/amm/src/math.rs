#![no_std]

use crate::liquidity_math::LiquidityMathError;

/// Computes the integer square root of `y` using Newton's method.
///
/// Returns `Err(LiquidityMathError::Overflow)` when `y < 0` instead of
/// panicking, so callers can propagate the error through the normal
/// `Result`-based error-handling convention used everywhere else in this crate.
///
/// # Errors
/// * [`LiquidityMathError::Overflow`] — `y` is negative.
///
/// # Examples
/// ```
/// use amm::math::try_sqrt;
/// assert_eq!(try_sqrt(25), Ok(5));
/// assert!(try_sqrt(-1).is_err());
/// ```
pub fn try_sqrt(y: i128) -> Result<i128, LiquidityMathError> {
    if y < 0 {
        return Err(LiquidityMathError::Overflow);
    }
    Ok(sqrt_unchecked(y))
}

/// Infallible integer square root — only valid for `y >= 0`.
///
/// This is an internal helper used by `try_sqrt` after the sign-check.
/// It is **not** part of the public API.
fn sqrt_unchecked(y: i128) -> i128 {
    if y > 3 {
        let mut z = y;
        let mut x = y / 2 + 1;
        while x < z {
            z = x;
            x = (y / x + x) / 2;
        }
        z
    } else if y != 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liquidity_math::LiquidityMathError;

    #[test]
    fn test_try_sqrt_basic_cases() {
        assert_eq!(try_sqrt(0), Ok(0));
        assert_eq!(try_sqrt(1), Ok(1));
        assert_eq!(try_sqrt(2), Ok(1));
        assert_eq!(try_sqrt(3), Ok(1));
        assert_eq!(try_sqrt(4), Ok(2));
        assert_eq!(try_sqrt(9), Ok(3));
        assert_eq!(try_sqrt(16), Ok(4));
        assert_eq!(try_sqrt(25), Ok(5));
        assert_eq!(try_sqrt(100), Ok(10));
        assert_eq!(try_sqrt(1_000_000), Ok(1000));
    }

    #[test]
    fn test_try_sqrt_large_value() {
        // Max representable integer sqrt for i128
        assert_eq!(try_sqrt(i128::MAX), Ok(13043817825332782212));
    }

    #[test]
    fn test_try_sqrt_negative_returns_overflow_error() {
        assert_eq!(try_sqrt(-1), Err(LiquidityMathError::Overflow));
        assert_eq!(try_sqrt(-100), Err(LiquidityMathError::Overflow));
        assert_eq!(try_sqrt(i128::MIN), Err(LiquidityMathError::Overflow));
    }
}
