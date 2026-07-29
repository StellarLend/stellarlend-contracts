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
    /// Pending admin address is not set.
    PendingAdminNotSet = 1004,
    /// Requested borrow is below the protocol minimum.
    BelowMinimumBorrow = 1008,
    /// Contract has not been initialised yet.
    NotInitialized = 1009,
    /// `initialize` was called a second time.
    AlreadyInitialized = 1010,
    /// Position is adequately collateralised; liquidation not allowed.
    PositionHealthy = 1011,
    /// Repayment amount exceeds the outstanding debt (including accrued interest).
    RepayAmountTooHigh = 1012,
    /// Protocol-level debt ceiling would be exceeded.
    DebtCeilingExceeded = 2001,
    /// Asset deposit cap would be exceeded.
    DepositCapExceeded = 2002,
    /// A borrow would push total outstanding debt for the asset beyond the
    /// configured per-asset `borrow_cap`.
    BorrowCapExceeded = 2003,
    /// Flash-loan fee is outside the permitted range.
    InvalidFeeBps = 2005,
    /// Flash-loan utilisation fee is outside the permitted range.
    InvalidFlashUtilizationBps = 2006,
    /// Collateral balance is insufficient for the requested withdrawal.
    InsufficientCollateral = 2007,
    /// Liquidator cannot self-liquidate.
    SelfLiquidation = 2008,
    /// Isolation-mode debt ceiling would be exceeded.
    IsolationCeilingExceeded = 2009,
    /// Isolation-mode ceiling value is invalid.
    InvalidIsolationCeiling = 2010,
    /// Liquidation parameters are invalid.
    InvalidLiquidationParams = 2011,
    /// The asset has not been configured via set_asset_params.
    AssetNotConfigured = 3001,
    /// Oracle price record is missing for the requested asset.
    PriceFeedNotFound = 3002,
    /// Operation would result in an unsafe health factor.
    HealthFactorTooLow = 3003,
    /// Oracle price is outside the configured bounds.
    PriceOutOfBounds = 3004,
    /// Price is temporarily unavailable.
    PriceUnavailable = 3005,
    /// Upgrade has not been initialised.
    UpgradeNotInitialized = 4001,
    /// Upgrade proposal not found.
    ProposalNotFound = 4002,
    /// Upgrade proposal is not yet ready for execution.
    ProposalNotReady = 4003,
    /// Upgrade proposal has expired.
    ProposalExpired = 4004,
    /// Upgrade proposal has already been executed.
    ProposalAlreadyExecuted = 4005,
    /// Caller has already approved this proposal.
    AlreadyApproved = 4006,
    /// Insufficient approvals to execute the upgrade.
    InsufficientUpgradeApprovals = 4007,
    /// Upgrade version string is invalid.
    InvalidUpgradeVersion = 4008,
    /// Approver not found.
    ApproverNotFound = 4009,
    /// Maximum number of approvers reached.
    MaxApproversReached = 4010,
    /// Upgrade configuration is invalid.
    InvalidUpgradeConfig = 4011,
    /// Oracle signature is invalid.
    InvalidOracleSignature = 5001,
    /// Oracle timestamp is stale.
    StaleOracleTimestamp = 5002,
    /// Oracle public key has not been set.
    OraclePubkeyNotSet = 5003,
    /// Oracle price max-move bound exceeded.
    MaxMoveBpsExceeded = 5004,
    /// Oracle price replay detected.
    OracleReplay = 5005,
    /// No bad debt to write off.
    NoBadDebt = 6001,
    /// Write-off amount exceeds recorded bad debt.
    WriteOffExceedsBadDebt = 6002,
    /// Liquidation threshold is outside the valid range.
    InvalidLiquidationThresholdBps = 7000,
    /// Close factor is outside the valid range.
    InvalidCloseFactorBps = 7001,
    /// Liquidation incentive is outside the valid range.
    InvalidLiquidationIncentiveBps = 7002,
    /// Deposit cap value is invalid.
    InvalidDepositCap = 7005,
    /// Rate parameters are internally inconsistent.
    InvalidRateParams = 7006,
    /// Liquidation grace period exceeds the protocol maximum.
    InvalidLiquidationGracePeriod = 7007,
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
        assert_eq!(LendingError::PendingAdminNotSet as u32, 1004);
        assert_eq!(LendingError::BelowMinimumBorrow as u32, 1008);
        assert_eq!(LendingError::NotInitialized as u32, 1009);
        assert_eq!(LendingError::AlreadyInitialized as u32, 1010);
        assert_eq!(LendingError::PositionHealthy as u32, 1011);
        assert_eq!(LendingError::RepayAmountTooHigh as u32, 1012);
        assert_eq!(LendingError::DebtCeilingExceeded as u32, 2001);
        assert_eq!(LendingError::DepositCapExceeded as u32, 2002);
        assert_eq!(LendingError::BorrowCapExceeded as u32, 2003);
        assert_eq!(LendingError::InvalidFeeBps as u32, 2005);
        assert_eq!(LendingError::InvalidFlashUtilizationBps as u32, 2006);
        assert_eq!(LendingError::InsufficientCollateral as u32, 2007);
        assert_eq!(LendingError::SelfLiquidation as u32, 2008);
        assert_eq!(LendingError::IsolationCeilingExceeded as u32, 2009);
        assert_eq!(LendingError::InvalidIsolationCeiling as u32, 2010);
        assert_eq!(LendingError::InvalidLiquidationParams as u32, 2011);
        assert_eq!(LendingError::AssetNotConfigured as u32, 3001);
        assert_eq!(LendingError::PriceFeedNotFound as u32, 3002);
        assert_eq!(LendingError::HealthFactorTooLow as u32, 3003);
        assert_eq!(LendingError::PriceOutOfBounds as u32, 3004);
        assert_eq!(LendingError::PriceUnavailable as u32, 3005);
        assert_eq!(LendingError::UpgradeNotInitialized as u32, 4001);
        assert_eq!(LendingError::ProposalNotFound as u32, 4002);
        assert_eq!(LendingError::ProposalNotReady as u32, 4003);
        assert_eq!(LendingError::ProposalExpired as u32, 4004);
        assert_eq!(LendingError::ProposalAlreadyExecuted as u32, 4005);
        assert_eq!(LendingError::AlreadyApproved as u32, 4006);
        assert_eq!(LendingError::InsufficientUpgradeApprovals as u32, 4007);
        assert_eq!(LendingError::InvalidUpgradeVersion as u32, 4008);
        assert_eq!(LendingError::ApproverNotFound as u32, 4009);
        assert_eq!(LendingError::MaxApproversReached as u32, 4010);
        assert_eq!(LendingError::InvalidUpgradeConfig as u32, 4011);
        assert_eq!(LendingError::InvalidOracleSignature as u32, 5001);
        assert_eq!(LendingError::StaleOracleTimestamp as u32, 5002);
        assert_eq!(LendingError::OraclePubkeyNotSet as u32, 5003);
        assert_eq!(LendingError::MaxMoveBpsExceeded as u32, 5004);
        assert_eq!(LendingError::OracleReplay as u32, 5005);
        assert_eq!(LendingError::NoBadDebt as u32, 6001);
        assert_eq!(LendingError::WriteOffExceedsBadDebt as u32, 6002);
        assert_eq!(LendingError::InvalidLiquidationThresholdBps as u32, 7000);
        assert_eq!(LendingError::InvalidCloseFactorBps as u32, 7001);
        assert_eq!(LendingError::InvalidLiquidationIncentiveBps as u32, 7002);
        assert_eq!(LendingError::InvalidDepositCap as u32, 7005);
        assert_eq!(LendingError::InvalidRateParams as u32, 7006);
    }
}
