# Cross-Asset Borrow/Repay Round-Trip Integration Test

## Rationale
While internal routines for borrowing and repaying assets (`borrow_asset_internal` and `repay_asset_internal` in `cross_asset.rs`) are tested thoroughly in isolation, real-world usage relies on their correctness across ledger time intervals where interest accrues and debt lists mutate.

This integration test exercises the full lifecycle of a loan:
1. Depositing collateral.
2. Borrowing a cross asset.
3. Advancing the ledger clock so interest accrues.
4. Fully repaying the debt with a generous overpayment to capture the accrued interest exactly.

## Edge Cases Covered
- **Total Debt Tracking**: Ensures `total_debt` scales precisely with the borrowed principal and scales down accurately on repayment.
- **Overpay Handling**: Repayments that exceed `principal + interest` gracefully refund the excess to the caller rather than leaving a negative debt balance.
- **Debt List Maintenance**: The user's per-asset debt list removes the asset entry entirely upon full repayment.

## Worked Example
In the standard setup, a user deposits `1,000,000` of Asset B. 
They then borrow `5,000` of Asset A.
The `total_debt` for Asset A increases by exactly `5,000`, and Asset A is added to the user's debt list.

We then simulate a time-skip of $1$ year ($31,536,000$ seconds). 
During this interval, the interest compounding logic escalates the user's debt. 
The test then issues a repayment of `10,000` (which is safely above `5,000` + accrued interest). 
The test strictly asserts:
1. The remaining debt for the user drops to $0$.
2. The user's debt list is fully cleared (size $0$).
3. The total debt pool reflects the repaid principal and appropriately allocates any reserve fees.
