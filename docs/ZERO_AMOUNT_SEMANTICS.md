# Zero Amount and Overpayment Semantics

## Overview

This document defines the expected behavior of the StellarLend protocol when handling zero, negative, or excessive amounts in state-mutating operations.

## Zero and Negative Amounts

All core lending entrypoints (`deposit`, `withdraw`, `borrow`, `repay`, `liquidate`) MUST reject zero and negative amounts.

- Providing `amount <= 0` results in `LendingError::InvalidAmount`.
- No state is mutated, and the transaction reverts.

## Overpayment (Repay)

When a user repays an amount greater than their outstanding debt (principal + accrued interest):

- The protocol **silently clamps** the repayment to the exact outstanding balance.
- The remaining debt becomes exactly `0`.
- Debt balances are **never** allowed to become negative. A negative debt must not be used to represent a credit balance.
- The `repay` function returns an explicit `i128` value indicating the **remaining principal debt after repayment**:
  - On exact or overpayment: returns `0`.
  - On partial repayment: returns the positive remaining principal.
- By clamping rather than rejecting overpayments, the protocol ensures users can easily clear their entire debt even as interest accrues between transaction creation and execution.

## No Prior Debt (Repay)

If a user calls `repay` when they have no outstanding debt:

- The protocol treats this as a zero-debt repay and clamps cleanly to `0`.
- No negative debt (credit balance) is created.
- The return value is `0`.

## View Functions

Read-only view functions such as `get_position` and `get_health_factor` are guaranteed to never report negative debt balances:

- `get_position().debt` always returns a value `>= 0`. If underlying interest arithmetic ever results in a sub-zero calculation, it is clamped to `0`.
- `get_debt_position().principal` reflects the raw stored principal, which is always written as `>= 0` by `repay` and `borrow`.
- `get_health_factor` similarly clamps the effective debt to `0` before the health-factor division.

## Implementation Notes

- The clamp is applied in `debt::repay_amount` by comparing the repay amount against the accrual-settled principal; if `amount >= settled.principal`, the resulting principal is set to `0`.
- An additional `.max(0)` guard is applied in `LendingContract::get_position` before constructing the `PositionSummary`, providing defense-in-depth against any future rounding edge case.
- The `TotalDebt` protocol counter is decremented by `prev_principal - updated.principal` (floored at `0` via `saturating_sub`), so it also cannot become negative.
