# Repay Entrypoint Technical Documentation

## Overview

The `repay` entrypoint allows users to repay their borrowed assets, reducing both the principal debt and any accrued interest. The protocol prioritizes the payment of interest before reducing the principal amount.

## Repayment Flow

1.  **Validation**: Ensures the repayment amount is positive and that the protocol is not in a paused state.
2.  **Asset Identification**: Determines the asset contract address. If the asset is `None`, it fetches the registered native asset (XLM) contract address from the protocol's storage.
3.  **Interest Accrual**: Calculates and adds interest accrued since the last interaction to the user's total debt.
4.  **Transfer**: Transfers the repayment amount from the user to the protocol contract. This requires the user to have previously approved the protocol to spend the tokens.
5.  **Debt Reduction**:
    - Interest is paid off first.
    - Any remaining amount is applied to the principal debt.
6.  **State Update**: Updates the user's position and analytics (total repayments, debt value).
7.  **Event Emission**: Emits `repay`, `position_updated`, and `analytics_updated` events for off-chain tracking.

## Technical Details

### Interest Calculation

Interest is calculated dynamically based on the current protocol utilization using the following components:

- `base_rate`: The minimum interest rate.
- `multiplier`: The rate of increase in interest as utilization increases.
- `kink`: The utilization point beyond which the interest rate increases more sharply.

For the exact numeric bounds behind rate clamping, checked arithmetic, and extreme timestamp handling, see [Interest Numeric Assumptions](../../../docs/INTEREST_NUMERIC_ASSUMPTIONS.md).

### Storage

- `Position`: Stores the user's current debt, accrued interest, and last accrual timestamp.
- `UserAnalytics`: Tracks the user's total repayments and current debt value.
- `ProtocolAnalytics`: Tracks the total across all users.

### Errors

- `InvalidAmount`: Repayment amount must be > 0.
- `InvalidAsset`: Asset contract address is invalid or not configured.
- `InsufficientBalance`: User does not have enough tokens to cover the repayment.
- `RepayPaused`: Repayments are currently disabled by the admin.
- `NoDebt`: The user has no outstanding debt for the specified asset.

## Security Considerations

- **Authorization**: The `repay_debt` function requires the user's authorization for the transfer.
- **Rounding**: Interest calculations are performed with high precision and rounding is handled to prevent debt leakage.
- **Overflow Protection**: All calculations use checked arithmetic to prevent overflows.
