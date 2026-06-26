# Reserve Factor Accounting

## Overview

The reserve factor determines what fraction of borrower interest is retained by
the protocol. The remainder flows to lenders.

```
reserve_amount = interest_amount × reserve_factor_bps ÷ 10_000   (integer division)
lender_amount  = interest_amount − reserve_amount
```

**Range:** 0–5000 bps (0%–50%). Default: 1000 bps (10%).

---

## Storage Layout

| Key | Type | Description |
|---|---|---|
| `ReserveDataKey::ReserveBalance(asset)` | `i128` | Accumulated reserve per asset (interest accrual path) |
| `ReserveDataKey::ReserveFactor(asset)` | `i128` | Reserve factor in bps |
| `ReserveDataKey::TotalReservesV1` | `i128` | Aggregate across all assets |
| `ReserveDataKey::ProtocolRevenueV1` | `i128` | Cumulative revenue (never decremented) |
| `DepositDataKey::ProtocolReserve(asset)` | `i128` | Flash-loan fee bucket (separate from above) |

> **Important:** Flash-loan fees are credited to `DepositDataKey::ProtocolReserve`,
> not to `ReserveDataKey::ReserveBalance`. `get_total_reserves()` and
> `get_reserve_balance()` do **not** include flash-loan fees.

---

## Interest Accrual Path

Called by the repay module on each repayment:

```
accrue_reserve(env, asset, interest_amount)
  → reserve_amount = interest_amount * factor / 10_000
  → ReserveBalance += reserve_amount
  → TotalReservesV1 += reserve_amount
  → ProtocolRevenueV1 += reserve_amount   (monotonically non-decreasing)
```

---

## Flash-Loan Fee Path

Called by `flash_loan.rs` after successful repayment:

```
fee = amount * fee_bps / 10_000   (default: 9 bps)
DepositDataKey::ProtocolReserve(asset) += fee
```

Flash-loan fees are **not** routed through `accrue_reserve` and therefore do
not appear in `get_total_reserves()` or `get_reserve_balance()`.

---

## Rounding Semantics

Integer division truncates toward zero. Consequences:

- `reserve_amount + lender_amount == interest_amount` always (no value created or destroyed).
- Sub-threshold interest (e.g. 1 stroop at 10% factor) yields `reserve_amount = 0`.
- Minimum non-zero reserve: `ceil(10_000 / factor_bps)` stroops of interest.
- Flash-loan minimum non-zero fee at 9 bps: 1_112 stroops.

---

## Security Invariants

1. `reserve_balance >= 0` at all times.
2. `total_reserves == Σ per-asset reserve balances`.
3. `protocol_revenue` is monotonically non-decreasing (withdrawals do not reduce it).
4. Withdrawals are bounded by `reserve_balance`; excess is rejected with `InsufficientReserve`.
5. Reserve factor is capped at 5000 bps; values above are rejected with `InvalidReserveFactor`.
6. All arithmetic uses `checked_*` operations; overflow returns `ReserveError::Overflow`.
7. Treasury address cannot be the contract itself (`InvalidTreasury`).
8. Withdrawals respect the `pause_reserve` pause switch.

---

## Examples

### 10% factor, 10_000 stroops interest

```
reserve_amount = 10_000 × 1_000 ÷ 10_000 = 1_000
lender_amount  = 10_000 − 1_000           = 9_000
```

### 9 bps flash-loan fee, 100_000 stroops loan

```
fee = 100_000 × 9 ÷ 10_000 = 90
total_repayment = 100_000 + 90 = 100_090
```

### Near-zero rounding (10% factor, 9 stroops interest)

```
reserve_amount = 9 × 1_000 ÷ 10_000 = 0   (truncated)
lender_amount  = 9 − 0               = 9
```

---

## References

- `contracts/hello-world/src/reserve.rs` — accrual, withdrawal, view functions
- `contracts/hello-world/src/flash_loan.rs` — fee calculation and fee bucket write
- `contracts/hello-world/src/tests/reserve_test.rs` — full test suite including
  edge-case coverage added in issue #659


### Reserve Realization & Withdrawal (`withdraw_reserve`)
The protocol isolates accrued reserve mathematically using a per-asset accumulator. Because reserves are held in the same SAC token balance as depositor principal, the `withdraw_reserve` entrypoint enforces strict bounds:
1. **Authorization:** Only the configured protocol admin can trigger a reserve withdrawal.
2. **Treasury Routing:** The withdrawn amount is routed directly to a specified treasury address.
3. **Principal Isolation:** The contract strictly enforces that withdrawal `amount` is `<= current_reserve`.
4. **State Accrual:** The asset's reserve accumulator is decremented by the withdrawn amount, and a `ReserveWithdrawn` event is emitted.
