//! Shared types and helpers for the StellarLend protocol.
//!
//! Provides a canonical `LendingError` enum, the `BPS_DENOM` constant,
//! checked `scale` / `unscale` helpers, and cross-asset price normalisation
//! utilities so every crate uses identical definitions.

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
/// Only basis-point rates in the valid range `0..=BPS_DENOM` are accepted.
/// Negative rates and rates above `100%` return `None`, as do overflow cases.
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
    if rate_bps < 0 || rate_bps > BPS_DENOM {
        return None;
    }
    value.checked_mul(rate_bps)?.checked_div(BPS_DENOM)
}

/// Divide `value` by `rate_bps` and multiply by [`BPS_DENOM`] (inverse of `scale_bps`).
///
/// Only basis-point rates in the valid range `0..=BPS_DENOM` are accepted.
/// Zero, negative, and rates above `100%` return `None`, as do overflow cases.
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
    if rate_bps <= 0 || rate_bps > BPS_DENOM {
        return None;
    }
    value.checked_mul(BPS_DENOM)?.checked_div(rate_bps)
}

// ── Cross-asset price normalisation ────────────────────────────────────────

/// Common internal fixed-point scale for cross-asset value aggregation (10^18).
///
/// All dollar-denominated values computed by [`normalize_price`] and
/// [`normalize_price_ceil`] are expressed in this fixed-point scale so that
/// assets with different oracle decimal precisions (e.g. 6 vs 8 vs 18) can be
/// summed safely.
///
/// # Relationship to the Lending contract's `PRICE_DIVISOR`
///
/// The Lending contract uses a fixed `PRICE_DIVISOR = 10_000_000` instead of
/// this constant because its oracle feeds all prices in a uniform 7-decimal
/// scale.  See the doc comment on `lending::cross_asset::PRICE_DIVISOR` for
/// the full rationale.
pub const INTERNAL_DECIMALS: u32 = 18;

/// Raise 10 to `exp`, checking for overflow.
///
/// Returns `None` if `10^exp` would overflow `i128`.
///
/// # Examples
/// ```
/// use stellar_lend_common::pow10_checked;
/// assert_eq!(pow10_checked(0), Some(1));
/// assert_eq!(pow10_checked(6), Some(1_000_000));
/// assert_eq!(pow10_checked(18), Some(1_000_000_000_000_000_000));
/// ```
pub fn pow10_checked(exp: u32) -> Option<i128> {
    let mut acc: i128 = 1;
    for _ in 0..exp {
        acc = acc.checked_mul(10)?;
    }
    Some(acc)
}

/// Normalise an oracle `raw_price` (which has `asset_decimals` fractional
/// digits) to the common [`INTERNAL_DECIMALS`] scale.
///
/// # Formula
///
/// ```text
/// normalised = raw_price × 10^(INTERNAL_DECIMALS - asset_decimals)   if INTERNAL_DECIMALS ≥ asset_decimals
/// normalised = raw_price / 10^(asset_decimals - INTERNAL_DECIMALS)   otherwise
/// ```
///
/// Division uses **floor** semantics (rounds toward zero in Rust), which is
/// conservative for collateral values.  Callers that need ceiling rounding
/// (e.g. for debt values) should use [`normalize_price_ceil`].
///
/// Returns `None` on overflow.
///
/// # Examples
/// ```
/// use stellar_lend_common::{normalize_price, INTERNAL_DECIMALS};
/// // 6-decimal USD price → 18-decimal internal
/// assert_eq!(normalize_price(1_000_000, 6), Some(1_000_000_000_000_000_000));
/// // Same decimals: no conversion
/// assert_eq!(normalize_price(1_234_567, 18), Some(1_234_567));
/// // Asset has more decimals: floor division
/// assert_eq!(normalize_price(1_234_567_000, 20), Some(12_345));
/// ```
#[inline]
pub fn normalize_price(raw_price: i128, asset_decimals: u32) -> Option<i128> {
    if asset_decimals == INTERNAL_DECIMALS {
        return Some(raw_price);
    }
    if asset_decimals < INTERNAL_DECIMALS {
        let scale = pow10_checked(INTERNAL_DECIMALS - asset_decimals)?;
        raw_price.checked_mul(scale)
    } else {
        let scale = pow10_checked(asset_decimals - INTERNAL_DECIMALS)?;
        Some(raw_price / scale) // floor (rounds toward zero)
    }
}

/// Same as [`normalize_price`] but rounds **up** when dividing (ceiling).
///
/// Used for debt values to stay conservative — rounding a debt value down
/// would understate the borrower's obligation.
///
/// # Examples
/// ```
/// use stellar_lend_common::normalize_price_ceil;
/// // Ceil vs floor when asset has more decimals than INTERNAL_DECIMALS
/// // floor:  123456789 / 100 = 1234567
/// // ceil:  (123456789 + 100 - 1) / 100 = 1234568
/// assert_eq!(normalize_price_ceil(123_456_789, 20), Some(1_234_568));
/// // No difference when up-scaling
/// assert_eq!(normalize_price_ceil(1_000_000, 6), Some(1_000_000_000_000_000_000));
/// ```
#[inline]
pub fn normalize_price_ceil(raw_price: i128, asset_decimals: u32) -> Option<i128> {
    if asset_decimals <= INTERNAL_DECIMALS {
        normalize_price(raw_price, asset_decimals)
    } else {
        let scale = pow10_checked(asset_decimals - INTERNAL_DECIMALS)?;
        // ceiling division: (a + (b-1)) / b
        let adjusted = raw_price.checked_add(scale.checked_sub(1)?)?;
        Some(adjusted / scale)
    }
}

#[cfg(test)]
mod bps_roundtrip_test;

#[cfg(test)]
mod bps_inverse_proptest;

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

    #[test]
    fn scale_bps_out_of_range_rate_returns_none() {
        assert_eq!(scale_bps(1_000_000, -500), None);
        assert_eq!(scale_bps(1_000_000, BPS_DENOM + 1), None);
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

    #[test]
    fn unscale_bps_out_of_range_rate_returns_none() {
        assert_eq!(unscale_bps(1_000_000, -500), None);
        assert_eq!(unscale_bps(1_000_000, BPS_DENOM + 1), None);
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
