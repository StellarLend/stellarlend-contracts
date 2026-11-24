# Zero Amount and Overpayment Semantics

## Overview

This document defines the expected behavior of the StellarLend protocol when handling zero, negative, or excessive amounts in state-mutating operations.

## Zero and Negative Amounts

All core lending entrypoints (`deposit`, `withdraw`, `borrow`, `repay`, `liquidate`) MUST reject zero and negative amounts.

- Providing `amount <= 0` results in `LendingError::InvalidAmount`.
- No state is mutated, and the transaction reverts.

## Overpayment (Repay)

> **Authoritative behavior:** The on-chain implementation (`debt::repay_amount` / `LendingContract::repay`) is authoritative. Overpayments are **rejected** rather than silently clamped.

When a user attempts to repay an amount greater than their outstanding debt (principal + accrued interest):

- The protocol **rejects** the call with `LendingError::RepayAmountTooHigh`.
- No state is mutated, and the outstanding debt remains unchanged.
- Debt balances are **never** allowed to become negative. A negative debt must not be used to represent a credit balance.
- On a valid repayment (`0 < amount <= settled_principal`), `repay` returns an explicit `i128` value indicating the **remaining principal debt after repayment**:
  - On exact repayment (`amount == settled_principal`): returns `0`.
  - On partial repayment (`amount < settled_principal`): returns the positive remaining principal.

## No Prior Debt (Repay)

If a user calls `repay` with `amount > 0` when they have no outstanding debt:

- The protocol **rejects** the call with `LendingError::RepayAmountTooHigh`.
- No negative debt (credit balance) is created, and debt remains `0`.
- Calling `repay` with `amount <= 0` when there is no debt still returns `LendingError::InvalidAmount`.

## View Functions

Read-only view functions such as `get_position` and `get_health_factor` are guaranteed to never report negative debt balances:

- `get_position().debt` always returns a value `>= 0`. If underlying interest arithmetic ever results in a sub-zero calculation, it is clamped to `0`.
- `get_debt_position().principal` reflects the raw stored principal, which is always written as `>= 0` by `repay` and `borrow`.
- `get_health_factor` similarly clamps the effective debt to `0` before the health-factor division.

## Implementation Notes

- Overpayment rejection is enforced in `debt::repay_amount` by comparing the repay amount against the accrual-settled principal; if `amount > settled.principal`, `DebtError::RepayAmountTooHigh` (mapped to `LendingError::RepayAmountTooHigh`) is returned.
- An additional `.max(0)` guard is applied in `LendingContract::get_position` before constructing the `PositionSummary`, providing defense-in-depth against any future rounding edge case.
- The `TotalDebt` protocol counter is decremented by `prev_principal - updated.principal` (floored at `0` via `saturating_sub`), so it also cannot become negative.
