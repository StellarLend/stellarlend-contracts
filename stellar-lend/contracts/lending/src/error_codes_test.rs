#![cfg(test)]
use super::LendingError;

#[test]
fn test_error_code_stability_and_uniqueness() {
    let cases = [
        (LendingError::InvalidAmount, 1001),
        (LendingError::Overflow, 1002),
        (LendingError::Unauthorized, 1003),
        (LendingError::PendingAdminNotSet, 1004),
        (LendingError::BelowMinimumBorrow, 1008),
        (LendingError::NotInitialized, 1009),
        (LendingError::AlreadyInitialized, 1010),
        (LendingError::PositionHealthy, 1011),
        (LendingError::DebtCeilingExceeded, 2001),
        (LendingError::DepositCapExceeded, 2002),
        (LendingError::InvalidFeeBps, 2005),
        (LendingError::InvalidFlashUtilizationBps, 2006),
        (LendingError::InsufficientCollateral, 2007),
        (LendingError::SelfLiquidation, 2008),
        (LendingError::InvalidOracleSignature, 5001),
        (LendingError::StaleOracleTimestamp, 5002),
        (LendingError::OraclePubkeyNotSet, 5003),
        (LendingError::UpgradeNotInitialized, 3001),
        (LendingError::ProposalNotFound, 3002),
        (LendingError::ProposalNotReady, 3003),
        (LendingError::ProposalExpired, 3004),
        (LendingError::ProposalAlreadyExecuted, 3005),
        (LendingError::AlreadyApproved, 3006),
        (LendingError::InsufficientUpgradeApprovals, 3007),
        (LendingError::InvalidUpgradeVersion, 3008),
        (LendingError::ApproverNotFound, 3009),
        (LendingError::MaxApproversReached, 3010),
        (LendingError::InvalidUpgradeConfig, 3011),
    ];

    for i in 0..cases.len() {
        let (err_i, code_i) = cases[i];
        assert_eq!(err_i as u32, code_i, "Error code mismatch for {:?}", err_i);

        for j in i + 1..cases.len() {
            let (err_j, code_j) = cases[j];
            assert!(
                code_i != code_j,
                "Collision detected: {:?} and {:?} both have code {}",
                err_i,
                err_j,
                code_i
            );
        }
    }
}
