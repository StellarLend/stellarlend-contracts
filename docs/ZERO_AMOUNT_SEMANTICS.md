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
- The `repay` function returns an explicit value indicating the remaining debt (which will be `0` in the case of overpayment).
- By clamping rather than rejecting overpayments, the protocol ensures users can easily clear their entire debt even as interest accrues between transaction creation and execution.

## View Functions

Read-only view functions such as `get_position` and `get_health_factor` are guaranteed to never report negative debt balances. If underlying math ever results in a sub-zero calculation, it is clamped to a floor of `0`.
