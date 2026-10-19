//! Exhaustive error code stability tests for `LendingError`.
//!
//! This test ensures that:
//! 1. Every error variant maps to exactly one unique code
//! 2. No two variants share the same code
//! 3. Adding a new variant without updating the stability table fails to compile
//! 4. The test remains synchronized with the enum definition

use super::LendingError;

/// Exhaustive list of all `LendingError` variants with their stable discriminants.
///
/// **CRITICAL:** This table MUST include every variant in `LendingError`.
/// The exhaustive match below will fail to compile if a variant is missing.
///
/// This constant is public to allow other crates (like the lending contract)
/// to verify re-export compatibility.
pub const ALL_LENDING_ERRORS: &[(LendingError, u32)] = &[
    // 1xxx: Input validation and authorization errors
    (LendingError::InvalidAmount, 1001),
    (LendingError::Overflow, 1002),
    (LendingError::Unauthorized, 1003),
    (LendingError::PendingAdminNotSet, 1004),
    (LendingError::BelowMinimumBorrow, 1008),
    (LendingError::NotInitialized, 1009),
    (LendingError::AlreadyInitialized, 1010),
    (LendingError::PositionHealthy, 1011),
    (LendingError::RepayAmountTooHigh, 1012),
    (LendingError::InvalidPayloadVersion, 1013),
    (LendingError::MalformedPayload, 1014),
    (LendingError::AssetNotSupported, 1015),
    (LendingError::UnauthorizedSender, 1016),
    // 2xxx: Risk caps and collateral management
    (LendingError::DebtCeilingExceeded, 2001),
    (LendingError::DepositCapExceeded, 2002),
    (LendingError::BorrowCapExceeded, 2003),
    (LendingError::InvalidFeeBps, 2005),
    (LendingError::InvalidFlashUtilizationBps, 2006),
    (LendingError::InsufficientCollateral, 2007),
    (LendingError::SelfLiquidation, 2008),
    (LendingError::IsolationCeilingExceeded, 2009),
    (LendingError::InvalidIsolationCeiling, 2010),
    (LendingError::InvalidLiquidationParams, 2011),
    (LendingError::IsolationCeilingBelowDebt, 2012),
    (LendingError::IsolationDebtInvariant, 2013),
    // 3xxx: Oracle and price feed errors
    (LendingError::AssetNotConfigured, 3001),
    (LendingError::PriceFeedNotFound, 3002),
    (LendingError::HealthFactorTooLow, 3003),
    (LendingError::PriceOutOfBounds, 3004),
    (LendingError::PriceUnavailable, 3005),
    // 4xxx: Upgrade and governance errors
    (LendingError::UpgradeNotInitialized, 4001),
    (LendingError::ProposalNotFound, 4002),
    (LendingError::ProposalNotReady, 4003),
    (LendingError::ProposalExpired, 4004),
    (LendingError::ProposalAlreadyExecuted, 4005),
    (LendingError::AlreadyApproved, 4006),
    (LendingError::InsufficientUpgradeApprovals, 4007),
    (LendingError::InvalidUpgradeVersion, 4008),
    (LendingError::ApproverNotFound, 4009),
    (LendingError::MaxApproversReached, 4010),
    (LendingError::InvalidUpgradeConfig, 4011),
    (LendingError::UpgradeProposalCancelled, 4012),
    (LendingError::UpgradeProposalNotCancellable, 4013),
    (LendingError::ApproverSetChanged, 4014),
    (LendingError::UpgradeSubmissionConflict, 4015),
    (LendingError::ApprovalBindingMismatch, 4016),
    (LendingError::OperationExpired, 4017),
    // 5xxx: Oracle signature validation
    (LendingError::InvalidOracleSignature, 5001),
    (LendingError::StaleOracleTimestamp, 5002),
    (LendingError::OraclePubkeyNotSet, 5003),
    (LendingError::MaxMoveBpsExceeded, 5004),
    (LendingError::OracleReplay, 5005),
    // 6xxx: Bad debt management
    (LendingError::NoBadDebt, 6001),
    (LendingError::WriteOffExceedsBadDebt, 6002),
    // 7xxx: Parameter validation
    (LendingError::InvalidLiquidationThresholdBps, 7000),
    (LendingError::InvalidCloseFactorBps, 7001),
    (LendingError::InvalidLiquidationIncentiveBps, 7002),
    (LendingError::InvalidDepositCap, 7005),
    (LendingError::InvalidRateParams, 7006),
    (LendingError::InvalidLiquidationGracePeriod, 7007),
    // 8xxx: Configuration management
    (LendingError::BackupNotFound, 8001),
];

#[test]
fn exhaustive_error_code_stability() {
    // Verify that the table length matches the enum variant count
    assert_eq!(
        ALL_LENDING_ERRORS.len(),
        62,
        "ALL_LENDING_ERRORS table must include all 62 LendingError variants"
    );

    // Check each variant's discriminant matches its expected code
    for &(variant, expected_code) in ALL_LENDING_ERRORS {
        let actual_code = variant as u32;
        assert_eq!(
            actual_code, expected_code,
            "Error code mismatch for {:?}: expected {}, got {}",
            variant, expected_code, actual_code
        );
    }

    // Verify uniqueness: no two variants share the same code
    for i in 0..ALL_LENDING_ERRORS.len() {
        let (variant_i, code_i) = ALL_LENDING_ERRORS[i];
        for j in (i + 1)..ALL_LENDING_ERRORS.len() {
            let (variant_j, code_j) = ALL_LENDING_ERRORS[j];
            assert_ne!(
                code_i, code_j,
                "Code collision detected: {:?} and {:?} both have code {}",
                variant_i, variant_j, code_i
            );
        }
    }
}

/// Exhaustive match guard to ensure the table stays synchronized with the enum.
///
/// This function will fail to compile if a new variant is added to `LendingError`
/// without updating the `ALL_LENDING_ERRORS` table above.
#[test]
fn all_variants_covered() {
    // Use an exhaustive match with no wildcard pattern
    for &(variant, _code) in ALL_LENDING_ERRORS {
        #[allow(clippy::match_single_binding)]
        match variant {
            // 1xxx
            LendingError::InvalidAmount => {}
            LendingError::Overflow => {}
            LendingError::Unauthorized => {}
            LendingError::PendingAdminNotSet => {}
            LendingError::BelowMinimumBorrow => {}
            LendingError::NotInitialized => {}
            LendingError::AlreadyInitialized => {}
            LendingError::PositionHealthy => {}
            LendingError::RepayAmountTooHigh => {}
            LendingError::InvalidPayloadVersion => {}
            LendingError::MalformedPayload => {}
            LendingError::AssetNotSupported => {}
            LendingError::UnauthorizedSender => {}
            // 2xxx
            LendingError::DebtCeilingExceeded => {}
            LendingError::DepositCapExceeded => {}
            LendingError::BorrowCapExceeded => {}
            LendingError::InvalidFeeBps => {}
            LendingError::InvalidFlashUtilizationBps => {}
            LendingError::InsufficientCollateral => {}
            LendingError::SelfLiquidation => {}
            LendingError::IsolationCeilingExceeded => {}
            LendingError::InvalidIsolationCeiling => {}
            LendingError::InvalidLiquidationParams => {}
            LendingError::IsolationCeilingBelowDebt => {}
            LendingError::IsolationDebtInvariant => {}
            // 3xxx
            LendingError::AssetNotConfigured => {}
            LendingError::PriceFeedNotFound => {}
            LendingError::HealthFactorTooLow => {}
            LendingError::PriceOutOfBounds => {}
            LendingError::PriceUnavailable => {}
            // 4xxx
            LendingError::UpgradeNotInitialized => {}
            LendingError::ProposalNotFound => {}
            LendingError::ProposalNotReady => {}
            LendingError::ProposalExpired => {}
            LendingError::ProposalAlreadyExecuted => {}
            LendingError::AlreadyApproved => {}
            LendingError::InsufficientUpgradeApprovals => {}
            LendingError::InvalidUpgradeVersion => {}
            LendingError::ApproverNotFound => {}
            LendingError::MaxApproversReached => {}
            LendingError::InvalidUpgradeConfig => {}
            LendingError::UpgradeProposalCancelled => {}
            LendingError::UpgradeProposalNotCancellable => {}
            LendingError::ApproverSetChanged => {}
            LendingError::UpgradeSubmissionConflict => {}
            LendingError::ApprovalBindingMismatch => {}
            LendingError::OperationExpired => {}
            // 5xxx
            LendingError::InvalidOracleSignature => {}
            LendingError::StaleOracleTimestamp => {}
            LendingError::OraclePubkeyNotSet => {}
            LendingError::MaxMoveBpsExceeded => {}
            LendingError::OracleReplay => {}
            // 6xxx
            LendingError::NoBadDebt => {}
            LendingError::WriteOffExceedsBadDebt => {}
            // 7xxx
            LendingError::InvalidLiquidationThresholdBps => {}
            LendingError::InvalidCloseFactorBps => {}
            LendingError::InvalidLiquidationIncentiveBps => {}
            LendingError::InvalidDepositCap => {}
            LendingError::InvalidRateParams => {}
            LendingError::InvalidLiquidationGracePeriod => {}
            // 8xxx
            LendingError::BackupNotFound => {}
        }
    }
}

#[test]
fn verify_error_code_ranges() {
    // Verify that codes fall into their documented ranges
    for &(variant, code) in ALL_LENDING_ERRORS {
        let range_name = match code {
            1000..=1999 => "1xxx: Input/Auth",
            2000..=2999 => "2xxx: Risk caps",
            3000..=3999 => "3xxx: Oracle/Price",
            4000..=4999 => "4xxx: Upgrade",
            5000..=5999 => "5xxx: Oracle signature",
            6000..=6999 => "6xxx: Bad debt",
            7000..=7999 => "7xxx: Params",
            8000..=8999 => "8xxx: Config",
            _ => panic!("Error code {} for {:?} is outside documented ranges", code, variant),
        };
        // If we get here without panicking, the code is in a valid range
        let _ = range_name; // Silence unused variable warning
    }
}
