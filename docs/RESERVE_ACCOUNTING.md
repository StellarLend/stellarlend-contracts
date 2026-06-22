# Reserve Factor Accounting

## Overview

The reserve factor determines what fraction of borrower interest is retained by the
protocol when lending debt is settled. The borrower still owes the full accrued
interest. The reserve share is credited to protocol reserves, and the remainder
compounds into borrower principal.

```
reserve_amount = ceil(interest_amount * reserve_factor_bps / 10_000)
lender_amount  = interest_amount - reserve_amount
```

**Range:** 0-5000 bps (0%-50%). Default: 0 bps.

---

## Storage Layout

| Key | Type | Description |
|---|---|---|
| `DataKey::ReserveFactorBps` | `i128` | Admin-configured reserve factor in bps |
| `DataKey::TotalReserve` | `i128` | Aggregate reserve accrued from lending interest |
| `DataKey::Debt(user)` | `DebtPosition` | Borrower principal after the non-reserve interest share compounds |
| `DepositDataKey::ProtocolReserve(asset)` | `i128` | Flash-loan fee bucket (separate from above) |

> **Important:** Flash-loan fees are credited to `DepositDataKey::ProtocolReserve`,
> not to `DataKey::TotalReserve`. `get_total_reserve()` does **not** include
> flash-loan fees.

---

## Interest Accrual Path

Called by borrow and repay when an existing position is settled:

```
settle_accrual_with_reserve(position, now, rate, factor)
  -> interest = accrue_interest(position.principal, elapsed, rate)
  -> reserve_amount = ceil(interest * factor / 10_000)
  -> compounded_interest = interest - reserve_amount
  -> position.principal += compounded_interest
  -> TotalReserve += reserve_amount
```

---

## Flash-Loan Fee Path

Called by `flash_loan.rs` after successful repayment:

```
fee = amount * fee_bps / 10_000   (default: 9 bps)
DepositDataKey::ProtocolReserve(asset) += fee
```

Flash-loan fees are **not** routed through `accrue_reserve` and therefore do
not appear in `get_total_reserve()`.

---

## Rounding Semantics

Reserve splitting rounds up in the protocol's favor when the factor is non-zero.
Consequences:

- `reserve_amount + lender_amount == interest_amount` always (no value created or destroyed).
- Sub-threshold interest (e.g. 1 stroop at 10% factor) yields `reserve_amount = 1`.
- Zero factor always yields `reserve_amount = 0`.
- Flash-loan minimum non-zero fee at 9 bps: 1_112 stroops.

---

## Security Invariants

1. `total_reserve >= 0` at all times.
2. `total_reserve` is monotonically non-decreasing until a guarded reserve withdrawal is added.
3. Reserve factor is capped at 5000 bps; values outside `[0, 5000]` are rejected.
4. All reserve split and reserve-credit arithmetic uses checked operations.
5. Borrower principal only receives `interest - reserve_amount`; reserve interest is not double-counted.

---

## Examples

### 10% factor, 10_000 stroops interest

```
reserve_amount = ceil(10_000 * 1_000 / 10_000) = 1_000
lender_amount  = 10_000 - 1_000                 = 9_000
```

### 9 bps flash-loan fee, 100_000 stroops loan

```
fee = 100_000 × 9 ÷ 10_000 = 90
total_repayment = 100_000 + 90 = 100_090
```

### Near-zero rounding (10% factor, 9 stroops interest)

```
reserve_amount = ceil(9 * 1_000 / 10_000) = 1
lender_amount  = 9 - 1                       = 8
```

---

## References

- `contracts/lending/src/debt.rs` - reserve split and settlement helpers
- `contracts/lending/src/lib.rs` - reserve factor setter and total reserve view
- `contracts/lending/src/reserve_factor_test.rs` - reserve factor tests
- `contracts/hello-world/src/reserve.rs` — accrual, withdrawal, view functions
- `contracts/hello-world/src/flash_loan.rs` — fee calculation and fee bucket write
- `contracts/hello-world/src/tests/reserve_test.rs` — full test suite including
  edge-case coverage added in issue #659
