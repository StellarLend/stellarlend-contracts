# LendingError Code Reference

This document lists all stable error codes emitted by the StellarLend protocol. Error codes are part of the on-chain ABI: front-ends, indexers, and integrators decode `Error(Contract, #n)` responses using these discriminants.

**Stability guarantee:** Error code numbers are **permanent** once assigned. New variants may be added, but existing codes will never be renumbered or reused for different meanings.

## Code Ranges

Error codes are organized by category:

| Range      | Category                        |
|------------|---------------------------------|
| `1000-1999` | Input validation and authorization |
| `2000-2999` | Risk caps and collateral management |
| `3000-3999` | Oracle and price feed errors |
| `4000-4999` | Upgrade and governance |
| `5000-5999` | Oracle signature validation |
| `6000-6999` | Bad debt management |
| `7000-7999` | Parameter validation |
| `8000-8999` | Configuration management |

---

## 1xxx: Input Validation and Authorization

| Code | Variant | Description |
|------|---------|-------------|
| `1001` | `InvalidAmount` | Amount must be positive and non-zero |
| `1002` | `Overflow` | Resulting value would exceed `i128::MAX` |
| `1003` | `Unauthorized` | Caller is not authorized for this operation |
| `1004` | `PendingAdminNotSet` | Pending admin address is not set |
| `1008` | `BelowMinimumBorrow` | Requested borrow is below the protocol minimum |
| `1009` | `NotInitialized` | Contract has not been initialized yet |
| `1010` | `AlreadyInitialized` | `initialize` was called a second time |
| `1011` | `PositionHealthy` | Position is adequately collateralized; liquidation not allowed |
| `1012` | `RepayAmountTooHigh` | Repayment amount exceeds the outstanding debt (including accrued interest) |
| `1013` | `InvalidPayloadVersion` | `receive` called with a payload version other than `1` |
| `1014` | `MalformedPayload` | `receive` called with a payload that is too short, too long, or contains values that cannot be decoded |
| `1015` | `AssetNotSupported` | `receive` called with an action `Symbol` that is not `"deposit"` or `"repay"` |
| `1016` | `UnauthorizedSender` | `receive` called with `from` equal to the token asset or the lending contract itself (prevents self-call / reentrancy attacks) |

---

## 2xxx: Risk Caps and Collateral Management

| Code | Variant | Description |
|------|---------|-------------|
| `2001` | `DebtCeilingExceeded` | Protocol-level debt ceiling would be exceeded |
| `2002` | `DepositCapExceeded` | Asset deposit cap would be exceeded |
| `2003` | `BorrowCapExceeded` | A borrow would push total outstanding debt for the asset beyond the configured per-asset `borrow_cap` |
| `2005` | `InvalidFeeBps` | Flash-loan fee is outside the permitted range |
| `2006` | `InvalidFlashUtilizationBps` | Flash-loan utilization fee is outside the permitted range |
| `2007` | `InsufficientCollateral` | Collateral balance is insufficient for the requested withdrawal |
| `2008` | `SelfLiquidation` | Liquidator cannot self-liquidate |
| `2009` | `IsolationCeilingExceeded` | Isolation-mode debt ceiling would be exceeded |
| `2010` | `InvalidIsolationCeiling` | Isolation-mode ceiling value is invalid |
| `2011` | `InvalidLiquidationParams` | Liquidation parameters are invalid |
| `2012` | `IsolationCeilingBelowDebt` | Isolation-mode ceiling would be exceeded by the current operation or ceiling value is below existing debt |
| `2013` | `IsolationDebtInvariant` | Isolation-mode debt tracking invariant violation detected |

---

## 3xxx: Oracle and Price Feed Errors

| Code | Variant | Description |
|------|---------|-------------|
| `3001` | `AssetNotConfigured` | The asset has not been configured via `set_asset_params` |
| `3002` | `PriceFeedNotFound` | Oracle price record is missing for the requested asset |
| `3003` | `HealthFactorTooLow` | Operation would result in an unsafe health factor |
| `3004` | `PriceOutOfBounds` | Oracle price is outside the configured bounds |
| `3005` | `PriceUnavailable` | Price is temporarily unavailable |

---

## 4xxx: Upgrade and Governance

| Code | Variant | Description |
|------|---------|-------------|
| `4001` | `UpgradeNotInitialized` | Upgrade has not been initialized |
| `4002` | `ProposalNotFound` | Upgrade proposal not found |
| `4003` | `ProposalNotReady` | Upgrade proposal is not yet ready for execution |
| `4004` | `ProposalExpired` | Upgrade proposal has expired |
| `4005` | `ProposalAlreadyExecuted` | Upgrade proposal has already been executed |
| `4006` | `AlreadyApproved` | Caller has already approved this proposal |
| `4007` | `InsufficientUpgradeApprovals` | Insufficient approvals to execute the upgrade |
| `4008` | `InvalidUpgradeVersion` | Upgrade version string is invalid |
| `4009` | `ApproverNotFound` | Approver not found |
| `4010` | `MaxApproversReached` | Maximum number of approvers reached |
| `4011` | `InvalidUpgradeConfig` | Upgrade configuration is invalid |
| `4012` | `UpgradeProposalCancelled` | The upgrade proposal has been cancelled and cannot be acted on |
| `4013` | `UpgradeProposalNotCancellable` | The upgrade proposal is not in a cancellable (pending) state |
| `4014` | `ApproverSetChanged` | The upgrade approver set changed after the proposal was created, so in-flight approvals are no longer authorized to execute |
| `4015` | `UpgradeSubmissionConflict` | A stale/duplicate upgrade submission would create contradictory state and is rejected rather than silently re-applied |
| `4016` | `ApprovalBindingMismatch` | The recorded approval binding does not match the expected nonce-bound domain-separated authorization for this proposal/approver pair |
| `4017` | `OperationExpired` | A prepared operation expired before commit |

---

## 5xxx: Oracle Signature Validation

| Code | Variant | Description |
|------|---------|-------------|
| `5001` | `InvalidOracleSignature` | Oracle signature is invalid |
| `5002` | `StaleOracleTimestamp` | Oracle timestamp is stale |
| `5003` | `OraclePubkeyNotSet` | Oracle public key has not been set |
| `5004` | `MaxMoveBpsExceeded` | Oracle price max-move bound exceeded |
| `5005` | `OracleReplay` | Oracle price replay detected |

---

## 6xxx: Bad Debt Management

| Code | Variant | Description |
|------|---------|-------------|
| `6001` | `NoBadDebt` | No bad debt to write off |
| `6002` | `WriteOffExceedsBadDebt` | Write-off amount exceeds recorded bad debt |

---

## 7xxx: Parameter Validation

| Code | Variant | Description |
|------|---------|-------------|
| `7000` | `InvalidLiquidationThresholdBps` | Liquidation threshold is outside the valid range |
| `7001` | `InvalidCloseFactorBps` | Close factor is outside the valid range |
| `7002` | `InvalidLiquidationIncentiveBps` | Liquidation incentive is outside the valid range |
| `7005` | `InvalidDepositCap` | Deposit cap value is invalid |
| `7006` | `InvalidRateParams` | Rate parameters are internally inconsistent |
| `7007` | `InvalidLiquidationGracePeriod` | Liquidation grace period exceeds the protocol maximum |

---

## 8xxx: Configuration Management

| Code | Variant | Description |
|------|---------|-------------|
| `8001` | `BackupNotFound` | Config backup not found |

---

## Maintenance Guidelines

When adding a new error variant:

1. Choose an appropriate code in the correct range (see table above)
2. Add the variant to `stellar-lend-common/src/lib.rs` with its discriminant
3. Update `stellar-lend-common/src/error_codes_test.rs`:
   - Add the variant to the `ALL_LENDING_ERRORS` constant table
   - Add a match arm in the `all_variants_covered` exhaustive match
   - Update the expected count in `exhaustive_error_code_stability`
4. Update this document with the new code
5. **Never reuse or renumber an existing code** — error codes are part of the on-chain ABI

## Testing

The exhaustive stability test in `error_codes_test.rs` enforces:

- **Uniqueness**: No two variants share the same code
- **Stability**: Each variant maps to its documented discriminant
- **Completeness**: The test fails to compile if a variant is added without updating the stability table

Run the test suite with:

```bash
cargo test -p stellar-lend-common error_codes
```

---

**Last Updated:** 2026-10-18  
**Total Error Codes:** 62
