//! Shared types and helpers for the StellarLend protocol.
//!
//! Provides a canonical `LendingError` enum, the `BPS_DENOM` constant, and
//! checked `scale` / `unscale` helpers so every crate uses identical definitions.

#![no_std]

use soroban_sdk::contracterror;

/// Denominator for basis-point arithmetic (`10_000` = 100 %).
pub const BPS_DENOM: i128 = 10_000;

/// Protocol-wide error codes.
///
/// All variants carry a stable `u32` discriminant so that on-chain wire codes
/// remain backward-compatible when new variants are added.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LendingError {
    /// Amount must be positive and non-zero.
    InvalidAmount = 1001,
    /// Resulting value would exceed `i128::MAX`.
    Overflow = 1002,
    /// Caller is not authorised for this operation.
    Unauthorized = 1003,
    /// Contract has not been initialised yet.
    NotInitialized = 1009,
    /// `initialize` was called a second time.
    AlreadyInitialized = 1010,
    /// Requested borrow is below the protocol minimum.
    BelowMinimumBorrow = 1008,
    /// Position is adequately collateralised; liquidation not allowed.
    PositionHealthy = 1011,
    /// Protocol-level debt ceiling would be exceeded.
    DebtCeilingExceeded = 2001,
    /// Asset deposit cap would be exceeded.
    DepositCapExceeded = 2002,
    /// Collateral balance is insufficient for the requested withdrawal.
    InsufficientCollateral = 2007,
    /// Flash-loan fee is outside the permitted range.
    InvalidFeeBps = 2005,
}

/// Multiply `value` by `rate_bps` and divide by [`BPS_DENOM`].
///
/// Returns `None` on overflow.
///
/// # Examples
/// ```
/// use stellar_lend_common::{scale_bps, BPS_DENOM};
/// // 1_000_000 * 500 BPS (5 %) = 50_000
/// assert_eq!(scale_bps(1_000_000, 500), Some(50_000));
/// // 0 rate → 0
/// assert_eq!(scale_bps(42, 0), Some(0));
/// ```
#[inline]
pub fn scale_bps(value: i128, rate_bps: i128) -> Option<i128> {
    value.checked_mul(rate_bps)?.checked_div(BPS_DENOM)
}

/// Divide `value` by `rate_bps` and multiply by [`BPS_DENOM`] (inverse of `scale_bps`).
///
/// Returns `None` if `rate_bps` is zero or on overflow.
///
/// # Examples
/// ```
/// use stellar_lend_common::unscale_bps;
/// // 50_000 / 500 BPS → 1_000_000
/// assert_eq!(unscale_bps(50_000, 500), Some(1_000_000));
/// // division by zero → None
/// assert_eq!(unscale_bps(1, 0), None);
/// ```
#[inline]
pub fn unscale_bps(value: i128, rate_bps: i128) -> Option<i128> {
    if rate_bps == 0 {
        return None;
    }
    value.checked_mul(BPS_DENOM)?.checked_div(rate_bps)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── scale_bps ────────────────────────────────────────────────────────────

    #[test]
    fn scale_bps_five_percent() {
        assert_eq!(scale_bps(1_000_000, 500), Some(50_000));
    }

    #[test]
    fn scale_bps_full_hundred_percent() {
        assert_eq!(scale_bps(1_000_000, BPS_DENOM), Some(1_000_000));
    }

    #[test]
    fn scale_bps_zero_rate() {
        assert_eq!(scale_bps(99_999, 0), Some(0));
    }

    #[test]
    fn scale_bps_zero_value() {
        assert_eq!(scale_bps(0, 500), Some(0));
    }

    #[test]
    fn scale_bps_overflow_returns_none() {
        // i128::MAX * 1 overflows in checked_mul → None
        assert_eq!(scale_bps(i128::MAX, 2), None);
    }

    #[test]
    fn scale_bps_negative_value() {
        // Signed i128 arithmetic should work symmetrically
        assert_eq!(scale_bps(-1_000_000, 500), Some(-50_000));
    }

    #[test]
    fn scale_bps_one_bps() {
        // 1 BPS of 10_000 → 1
        assert_eq!(scale_bps(10_000, 1), Some(1));
    }

    // ── unscale_bps ──────────────────────────────────────────────────────────

    #[test]
    fn unscale_bps_five_percent() {
        assert_eq!(unscale_bps(50_000, 500), Some(1_000_000));
    }

    #[test]
    fn unscale_bps_full_hundred_percent() {
        assert_eq!(unscale_bps(1_000_000, BPS_DENOM), Some(1_000_000));
    }

    #[test]
    fn unscale_bps_zero_divisor_returns_none() {
        assert_eq!(unscale_bps(1_000_000, 0), None);
    }

    #[test]
    fn unscale_bps_zero_value() {
        assert_eq!(unscale_bps(0, 500), Some(0));
    }

    #[test]
    fn unscale_bps_overflow_returns_none() {
        // i128::MAX * BPS_DENOM overflows
        assert_eq!(unscale_bps(i128::MAX, 1), None);
    }

    #[test]
    fn unscale_bps_negative_value() {
        assert_eq!(unscale_bps(-50_000, 500), Some(-1_000_000));
    }

    // ── LendingError discriminants ────────────────────────────────────────────

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(LendingError::InvalidAmount as u32, 1001);
        assert_eq!(LendingError::Overflow as u32, 1002);
        assert_eq!(LendingError::Unauthorized as u32, 1003);
        assert_eq!(LendingError::NotInitialized as u32, 1009);
        assert_eq!(LendingError::AlreadyInitialized as u32, 1010);
        assert_eq!(LendingError::BelowMinimumBorrow as u32, 1008);
        assert_eq!(LendingError::PositionHealthy as u32, 1011);
        assert_eq!(LendingError::DebtCeilingExceeded as u32, 2001);
        assert_eq!(LendingError::DepositCapExceeded as u32, 2002);
        assert_eq!(LendingError::InsufficientCollateral as u32, 2007);
        assert_eq!(LendingError::InvalidFeeBps as u32, 2005);
    }
}
