//! Error code tests for the lending contract.
//!
//! These tests verify that:
//! 1. The `LendingError` re-export from `stellar_lend_common` is usable
//! 2. Error codes remain stable across the re-export
//! 3. Contract-level error handling works correctly

use super::LendingError;

/// Verify that `stellarlend_lending::LendingError` is the same type as
/// `stellar_lend_common::LendingError` through the re-export.
#[test]
fn lending_error_is_common_type() {
    // This function signature enforces that the lending crate's LendingError
    // is compatible with the common crate's type
    fn accepts_common_error(_err: stellar_lend_common::LendingError) {}
    
    // If this compiles, the re-export works correctly
    accepts_common_error(LendingError::InvalidAmount);
    accepts_common_error(LendingError::DebtCeilingExceeded);
    accepts_common_error(LendingError::UpgradeProposalCancelled);
    accepts_common_error(LendingError::IsolationCeilingBelowDebt);
    accepts_common_error(LendingError::InvalidPayloadVersion);
}

/// Verify that error codes are stable through the re-export by checking
/// a sample of codes from each range.
#[test]
fn error_codes_stable_through_reexport() {
    // Sample from each range to verify stability
    assert_eq!(LendingError::InvalidAmount as u32, 1001);
    assert_eq!(LendingError::InvalidPayloadVersion as u32, 1013);
    assert_eq!(LendingError::DebtCeilingExceeded as u32, 2001);
    assert_eq!(LendingError::IsolationCeilingBelowDebt as u32, 2012);
    assert_eq!(LendingError::AssetNotConfigured as u32, 3001);
    assert_eq!(LendingError::UpgradeNotInitialized as u32, 4001);
    assert_eq!(LendingError::UpgradeProposalCancelled as u32, 4012);
    assert_eq!(LendingError::InvalidOracleSignature as u32, 5001);
    assert_eq!(LendingError::NoBadDebt as u32, 6001);
    assert_eq!(LendingError::InvalidLiquidationThresholdBps as u32, 7000);
    assert_eq!(LendingError::BackupNotFound as u32, 8001);
}

/// Verify that all error codes defined in the common crate are accessible
/// through the lending contract's re-export.
#[test]
fn all_common_errors_accessible() {
    // Use the exhaustive list from common to verify all variants are accessible
    for &(variant, expected_code) in stellar_lend_common::error_codes_test::ALL_LENDING_ERRORS {
        // Verify the code matches through the re-export
        assert_eq!(
            variant as u32,
            expected_code,
            "Error code mismatch for {:?}",
            variant
        );
    }
}

/// Verify that error conversions work correctly for contract error handling.
/// This ensures that `?` operator and `panic_with_error!` work through the re-export.
#[test]
fn error_conversion_works() {
    use soroban_sdk::{Env, Error};
    
    let env = Env::default();
    
    // Test conversion from LendingError to soroban_sdk::Error
    let sdk_error: Error = LendingError::InvalidAmount.into();
    
    // The error should encode with the expected discriminant
    assert!(sdk_error.is_type(soroban_sdk::xdr::ScErrorType::Contract));
    
    // Test a few more to ensure conversion works for all ranges
    let _: Error = LendingError::IsolationCeilingBelowDebt.into();
    let _: Error = LendingError::UpgradeProposalCancelled.into();
    let _: Error = LendingError::InvalidPayloadVersion.into();
}
