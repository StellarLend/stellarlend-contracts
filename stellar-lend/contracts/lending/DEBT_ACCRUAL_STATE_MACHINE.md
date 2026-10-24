# Debt Accrual State Machine

## Scope

This document describes the elapsed-time debt accrual path in
`src/debt.rs`: `DebtPosition`, `RateSnapshot`, `BorrowRateCache`,
`accrue_interest`, `settle_accrual`, `effective_debt`,
`cached_borrow_rate`, `uncached_borrow_rate`, and
`effective_supply_rate`.

The global borrow-index path is documented separately in
[BORROW_INDEX.md](./BORROW_INDEX.md). New index-aware callers should keep that
model in sync with these ordering rules: settle previous accrual first, mutate
principal second, then refresh the per-position snapshot.

## Position States

```text
NoPosition
    |
    | borrow(amount)
    v
OpenPosition(principal > 0, last_update = T)
    |
    | effective_debt(now, rate)
    v
ReadAccruedView(no storage write)
    |
    | settle_accrual(now, rate)
    v
SettledPosition(principal includes interest through T, last_update = T)
    |
    | borrow(amount)                  | repay(amount < principal)
    v                                 v
OpenPosition(principal increased)     OpenPosition(principal reduced)
                                      |
                                      | repay(amount >= principal)
                                      v
                                  ClosedPosition(principal = 0)
```

`DebtPosition.principal` is the settled principal at `last_update`. It does not
include interest accrued after `last_update` until either `settle_accrual` or a
mutation helper folds that interest into principal.

## Rate Snapshot And Cache

`RateSnapshot` captures the aggregate inputs used by the borrow-rate model:

```text
total_debt
total_supply
rate params
```

There are two rate-read paths:

| Function | Storage behavior | Use |
|---|---|---|
| `uncached_borrow_rate(env)` | Always loads a fresh `RateSnapshot` from storage | Use when the caller explicitly needs current aggregate storage, even if another rate was read earlier in the same ledger |
| `cached_borrow_rate(env)` | Uses `DataKey::BorrowRateCache(ledger_sequence)` in temporary storage | Use for normal protocol flow when all operations in one ledger should share the first rate computed in that ledger |

The cache is valid only for the current ledger sequence. It is not explicitly
invalidated. Advancing the ledger changes the key from
`BorrowRateCache(L)` to `BorrowRateCache(L + 1)`, so the next call misses and
loads a new `RateSnapshot`.

Within a ledger, `cached_borrow_rate` intentionally remains stable even if
`TotalDebt` or `TotalDeposits` are changed after the first read. The first rate
read defines the ledger's rate snapshot; later calls in the same ledger reuse it
to avoid intra-ledger rate drift.

## Transition Ordering

### Borrow

```text
1. validate gates, amount, auth, collateral/price preconditions
2. now = env.ledger().timestamp()
3. rate = current_borrow_rate(env) -> normally cached_borrow_rate(env)
4. load position
5. settle_accrual(position, now, rate)
6. add borrow amount to settled principal
7. write DebtPosition { principal, last_update: now, snapshot unchanged/refreshed by indexed path }
8. update aggregate debt and emit event
```

The rate is read before the mutation and the same rate is used to settle the
old principal and evaluate the new position. This prevents the new borrow amount
from being charged for time that elapsed before it existed.

### Accrue / Read View

```text
effective_debt(position, now, rate):
    elapsed = now - position.last_update, saturating at 0
    interest = accrue_interest(position.principal, elapsed, rate)
    return position.principal + interest
```

`effective_debt` never writes storage. It is safe for views such as
`get_position` and health-factor checks because repeated calls at the same
timestamp return the same value.

### Settle

```text
settle_accrual(position, now, rate):
    elapsed = now - position.last_update, saturating at 0
    interest = accrue_interest(position.principal, elapsed, rate)
    principal = position.principal + interest
    last_update = now
```

After settlement, `effective_debt(settled, now, rate)` returns exactly
`settled.principal`. This is the critical double-counting guard: once
`last_update` is moved to `now`, a same-ledger or same-timestamp read has
`elapsed == 0`.

`settle_accrual_split` follows the same ordering, but also splits the gross
interest into depositor yield and reserve cut. The split is accounting-only:
`depositor_yield + reserve_cut == total_interest`, and the debt principal is
still increased by the gross interest.

### Repay

```text
1. validate gates, amount, auth
2. now = env.ledger().timestamp()
3. rate = current_borrow_rate(env) -> normally cached_borrow_rate(env)
4. load position
5. settle_accrual(position, now, rate)
6. subtract repayment from settled principal, clamping at 0
7. write DebtPosition { principal, last_update: now, snapshot unchanged/refreshed by indexed path }
8. reduce aggregate debt and emit event
```

Repay settles first, then subtracts. This ensures the borrower repays against
the full debt owed through `now`; it also prevents a borrower from avoiding
already-accrued interest by repaying before settlement.

## Cache-Hit Versus Cache-Miss Rules

```text
Ledger L, first rate read:
    cached_borrow_rate misses BorrowRateCache(L)
    load_rate_snapshot reads TotalDebt/TotalDeposits/RateParams
    compute rate
    write BorrowRateCache(L)

Ledger L, later rate read:
    cached_borrow_rate hits BorrowRateCache(L)
    return the same rate even if aggregate totals changed inside L

Ledger L + 1:
    key is BorrowRateCache(L + 1)
    previous cache is ignored
    first read recomputes from current aggregate storage
```

Use `uncached_borrow_rate` for diagnostics, tests, or explicit comparisons
where the caller needs to observe fresh aggregate storage in the same ledger.
Use `cached_borrow_rate` for normal mutation and view paths that should share a
stable per-ledger rate.

## Why The Ordering Prevents Double Counting

Double counting would happen if interest were accrued, principal updated, and a
second accrual over the same time interval were then applied again. The current
ordering avoids that by making settlement move the timestamp boundary:

```text
before:
    principal = P
    last_update = T0

settle at T1:
    interest = f(P, T1 - T0, rate)
    principal = P + interest
    last_update = T1

same-timestamp read at T1:
    elapsed = T1 - T1 = 0
    interest = 0
    effective_debt = principal
```

The doc-example test in `src/accrual_state_doc_test.rs` pins this behavior.

## Worked Example

Assume:

```text
principal = 10_000
last_update = T0
now = T0 + 1 year
rate = 500 bps (5% APR)
```

`settle_accrual` computes:

```text
interest = 10_000 * 5% = 500
principal = 10_000 + 500 = 10_500
last_update = now
```

A same-timestamp `effective_debt` call then computes:

```text
elapsed = now - now = 0
interest = 0
effective_debt = 10_500
```

If the user repays `2_000` after settlement:

```text
new principal = 10_500 - 2_000 = 8_500
last_update = now
```

If the user repays `11_000`, repayment is clamped:

```text
new principal = 0
last_update = now
```

## Related Rate Docs

- [RATE_SMOOTHING.md](./RATE_SMOOTHING.md) explains how the target borrow rate
  can be bounded per ledger before it becomes the applied borrow rate.
- [BORROW_RATE_CACHE_TESTS.md](./BORROW_RATE_CACHE_TESTS.md) documents the
  cache equivalence and stale-cache rejection examples.
- [BORROW_INDEX.md](./BORROW_INDEX.md) documents the index-based path that
  replaces per-position elapsed-time accrual for upgraded positions.
