# Insurance Fund & Bad-Debt Accounting Specification

This document specifies the smart-contract level accounting for managing protocol insolvency and bad debt within the StellarLend protocol.

## 1. Overview

In extreme market conditions, a user's collateral may lose value so rapidly that it no longer covers their outstanding debt (Interest + Principal). When this happens, a liquidation cannot fully recover the debt, resulting in **Bad Debt**.

The protocol implements an **Insurance Fund** mechanism to track available surpluses that can be used to "socialize" or offset this bad debt, protecting the overall protocol solvency.

## 2. Definitions

- **Bad Debt ($D_{bad}$)**: The unrecovered portion of a borrow position after all available collateral has been liquidated.
- **Insurance Fund ($F_{ins}$)**: A per-asset pool of tokens (maintained via accounting) used to offset bad debt.
- **Underwater Position**: A position where `Collateral Value * Liquidation Threshold < Debt Value`.
- **Insolvent Position**: A position where `Collateral Value < Debt Value`.

## 3. Storage Structures

Three primary keys are added to the `Instance` storage to track global accounting:

| Key | Type | Description |
|-----|------|-------------|
| `TotalBadDebt(Address)` | `i128` | Cumulative unrecovered debt for a specific asset. |
| `InsuranceFundBalance(Address)` | `i128` | Current balance of the insurance fund for a specific asset. |
| `SocializedLoss(Address)` | `i128` | (Optional) Track debt that has been written off. |

## 4. Accounting Invariants

The protocol maintains the following global invariants (where $D_{total}$ is total debt and $C_{total}$ is total collateral):

1. **Global Solvency**: $\sum C_{val} \ge \sum D_{val} - \sum D_{bad}$.
2. **Insurance Fund Integrity**: $F_{ins} \ge 0$.
3. **Bad Debt Tracking**: $D_{bad}$ only increases during insolvent liquidations and decreases during an `Offset` event.

## 5. Liquidation Flow with Bad Debt

When `liquidate_position` is called:

1. **Calculate Recoverable Debt**: Determine how much of the user's debt can be covered by their remaining collateral (including the liquidator incentive).
2. **Handle Insolvent Case**:
   - If `Repay Amount > Collateral Value`:
     - $Shortfall = Repay Amount - Collateral Value$.
     - Increase `TotalBadDebt(Asset)` by $Shortfall$.
     - If `InsuranceFundBalance(Asset) > 0`:
       - $Offset = \min(Shortfall, InsuranceFundBalance)$.
       - Decrease `InsuranceFundBalance` and `TotalBadDebt` by $Offset$.
3. **Emit Events**: `BadDebtRecorded` and `InsuranceFundOffset`.

## 6. Access Control & Authorization

- **Crediting Fund**: Admin or specific protocol-authorized addresses (e.g., from flash loan fees) can credit the `InsuranceFundBalance`.
- **Offsetting Debt**: Automated during liquidation or manually triggered by the `Admin` or `Guardian` during emergency recovery.
- **Checked Arithmetic**: All updates to $D_{bad}$ and $F_{ins}$ MUST use `checked_add` and `checked_sub` to prevent overflow-driven insolvency.

## 7. Reentrancy & Security

- **Checks-Effects-Interactions**: Accounting updates to $D_{bad}$ and $F_{ins}$ occur BEFORE any external token transfers (if any) are simulated or executed.
- **Authorization**: `require_auth()` is enforced on all admin functions modifying the insurance fund or writing off debt.
