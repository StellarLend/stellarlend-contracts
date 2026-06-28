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

## Reserve Claim Invariants (`claim_reserves`)

The admin-only entrypoint `HelloContract::claim_reserves(caller, asset, to, amount)`
debits `DepositDataKey::ProtocolReserve(asset)` exactly by `amount`. It is the
on-chain exit point for flash-loan fee revenue that has accumulated in the
`ProtocolReserve(asset)` bucket (distinct from `ReserveBalance`, see "Storage
Layout" above).

### Behavioral contract

A claim against the `ProtocolReserve(asset)` flash-loan fee bucket:

- **Bounded.** A claim with `amount > reserve_balance` is rejected with
  `RiskManagementError::InvalidParameter`. The bucket is **never overdrawn**.
- **Exact-debit.** On success, `new_balance = old_balance − amount` (integer
  arithmetic, no fees, no rounding). No off-by-one: a value of `1` stroop above
  the balance is rejected; a value exactly at the balance zeros the bucket.
- **Admin-gated.** The caller must satisfy `require_admin`; non-admin callers
  are rejected with `RiskManagementError::Unauthorized` and the bucket is
  unchanged.
- **View-consistent.** After any successful claim, `get_reserve_balance(asset)`
  returns `old_balance − amount`.
- **Asset-isolated.** A claim against one asset's `ProtocolReserve` bucket
  does not affect any other asset's bucket.
- **Zero-amount permissive.** `amount == 0` is permitted for any
  `reserve_balance >= 0` and is a no-op for storage.

### Clamp boundary

The off-by-one fork sits in a single comparison:

```
if amount > reserve_balance {
    return Err(RiskManagementError::InvalidParameter);  // over-claim rejected, no debit
}
// in non-test builds: token transfer from contract balance to `to`
reserve_balance -= amount;
env.storage().persistent().set(&ProtocolReserve(asset), &reserve_balance);
Ok(())
```

A regression that flips `>` to `>=` would let the admin withdraw
`amount == reserve_balance + 1`, leaving the bucket at `−1` and violating
reserve-claim invariant #1 (`reserve_balance >= 0`).

### Worked example

Initial: `ProtocolReserve(USDC) = 1_000`. Caller is admin; `to = treasury`.

```
claim_reserves(admin, Some(USDC), treasury, 400)
  reserve_balance : 1000 -> 600     // exact debit
  token transfer  : contract -> treasury, 400 stroops
  get_reserve_balance(Some(USDC))  -> 600

claim_reserves(admin, Some(USDC), treasury, 600)
  reserve_balance : 600 -> 0        // exact-balance branch (>)
  get_reserve_balance(Some(USDC))  -> 0

claim_reserves(admin, Some(USDC), treasury, 1)     // over-claim on depleted bucket
  -> Err(RiskManagementError::InvalidParameter)
  reserve_balance : 0                // unchanged on rejection
```

### Test coverage (`claim_reserves_test.rs`)

The test file `stellar-lend/contracts/hello-world/src/claim_reserves_test.rs`
pins every invariant above:

| Test | Invariant pinned |
|---|---|
| `test_full_claim_zeros_reserve`              | exact-debit at the upper edge |
| `test_partial_claim_leaves_exact_remainder`  | exact-debit in the interior  |
| `test_over_claim_is_rejected_never_overdrawn` | bounded (rejected, no debit) |
| `test_exact_balance_claim_zeros_reserve`     | `>` vs `>=` boundary branch  |
| `test_zero_reserve_claim_is_rejected`        | bounded when balance == 0    |
| `test_zero_reserve_zero_amount_claim_is_noop` | `0 > 0` is false → success  |
| `test_zero_amount_claim_against_positive_reserve_is_noop` | zero-amount no-op |
| `test_non_admin_claim_is_rejected`           | admin gate (`Unauthorized`) |
| `test_two_partial_claims_then_full_claim_drain` | composability + drain-recovery |
| `test_multiple_assets_isolated_balances`     | asset isolation             |
| `test_post_claim_balance_reflects_storage_state` | view consistency         |

Run with:

```
cargo test -p hello-world claim_reserves
```

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
