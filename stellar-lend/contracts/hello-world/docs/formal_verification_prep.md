# Formal Verification Preparation Notes

This document captures verification-oriented assumptions and trust boundaries for borrow, repay, and liquidate flows in the hello-world contract.

## Scope

- Modules: src/borrow.rs, src/repay.rs, src/liquidate.rs
- Purpose: add machine-check-friendly invariants and explicit security assumptions.
- Non-goals: no protocol redesign and no intentional economic behavior changes.

## Trust Boundaries

- User authority:
  - Borrow and repay are user-initiated and rely on user authorization.
  - Liquidation is liquidator-initiated and requires liquidator authorization.
- Admin authority:
  - Sets protocol and risk parameters through admin-only entry points in contract wiring.
  - Can alter pause and risk controls that indirectly affect borrow/repay/liquidate paths.
- Guardian authority:
  - Guardian power is recovery/emergency scoped and does not directly transfer user funds in these three modules.

## External Call Paths

- Borrow path:
  - Optional token transfer to borrower (contract to user) for non-native assets.
  - Reentrancy guard is active before external token client use.
- Repay path:
  - Token transfer_from (user to contract) to settle debt.
  - Reentrancy guard is active before external token client use.
- Liquidate path:
  - Oracle-dependent price reads and token metadata reads (decimals).
  - Token transfer_from for debt repayment and token transfer for collateral seizure.
  - Reentrancy guard is active before external interactions.

## Authorization and Pause Checks

- Borrow:
  - Rejects non-positive amounts.
  - Enforces borrow pause switch and collateral/risk constraints.
- Repay:
  - Rejects non-positive amounts.
  - Enforces repay pause switch and debt existence checks.
- Liquidate:
  - Rejects non-positive debt amounts.
  - Requires liquidator auth and enforces emergency/operation pause checks.
  - Requires position to be liquidatable under configured risk parameters.

## Arithmetic and Bounds

- All three modules use checked arithmetic for critical accounting transitions.
- Liquidation uses I256 for scaled seizure math and applies explicit decimal scaling bounds.
- Parameter and amount handling keeps computations in explicit integer domains.

## Verification Hooks Added

- Borrow:
  - precondition hook for positive amount and non-empty collateral/debt domains.
  - postcondition hook for principal increment and total debt consistency.
- Repay:
  - precondition hook for positive amount and outstanding debt.
  - postcondition hook for interest-first partitioning and monotonic debt reduction.
- Liquidate:
  - precondition hook for positive liquidated amount.
  - postcondition hook for debt and collateral monotonicity under liquidation caps.

## Future Verification Ticket

- Placeholder: FV-HELLO-001 (formal proofs for borrow/repay/liquidate invariants)
- Suggested proof targets:
  - CEI ordering and reentrancy non-bypass on fund-moving paths.
  - Conservation-style checks on debt and collateral transitions.
  - Boundedness and no-overflow proofs for scaled liquidation arithmetic.
