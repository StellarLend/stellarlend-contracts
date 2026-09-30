# Borrow-Rate Cache Equivalence Tests

## Rationale

`cached_borrow_rate` and `uncached_borrow_rate` in `debt.rs` must produce
identical results **within a single ledger**, and the cache must be invalidated
when the ledger advances so that fresh aggregate totals are always reflected.

The tests in `borrow_rate_cache_equiv_test.rs` assert this invariant explicitly.

### Why a cache at all?

Computing the borrow rate reads three storage entries (`TotalDebt`,
`TotalDeposits`, `RateParams`). During a single ledger, none of these change
without an explicit write (e.g. a `borrow` or `repay` mutation), so caching the
result for the rest of the ledger avoids redundant reads without any risk of
returning stale data.

### Invalidation model

The cache key is `DataKey::BorrowRateCache(ledger_sequence)`. Because the key
includes the ledger sequence:

- **Same ledger** – the same key is reused; a cache hit returns the stored
  value without recomputation.
- **Cross ledger** – the key changes; the old entry is a different key and is
  never read, so a fresh computation always happens on the first call of the
  new ledger.

This approach requires no explicit invalidation logic — the ledger sequence
naturally partitions cache entries.

## Worked example

### Setup

```rust
let params = RateParams {
    base_rate_bps: 100,
    kink_utilization_bps: 8_000,   // 80%
    multiplier_bps: 2_000,
    jump_multiplier_bps: 10_000,
    rate_floor_bps: 50,
    rate_ceiling_bps: 10_000,
    max_rate_change_per_ledger_bps: i128::MAX,
    hysteresis_bps: 0,
};
```

### Ledger 100: 40% utilization

```
total_debt    = 4_000
total_deposits = 10_000
utilization   = 4_000 * 10_000 / 10_000 = 4_000 bps (40%)
```

Since `utilization ≤ kink`:
```
rate = base_rate + utilization * multiplier / 10_000
     = 100       + 4_000 * 2_000 / 10_000
     = 100       + 800
     = 900 bps (9 %)
```

`cached_borrow_rate` returns **900**, matching `uncached_borrow_rate`.
A cache entry `BorrowRateCache { ledger_sequence: 100, rate_bps: 900 }` is
written to temporary storage.

### Same ledger — re-read

Calling `cached_borrow_rate` again at ledger 100 hits the cache, returns
**900** without reading storage or recomputing.

### Same ledger — totals change mid-ledger

If an operation changes `TotalDebt` to 9_000 after the cache was populated:
- `uncached_borrow_rate` reads the new `TotalDebt` → **2_700 bps** (above kink).
- `cached_borrow_rate` returns the cached **900 bps** — the cache is valid for
  this ledger because it was computed once and the key hasn't changed.

This is **correct**: the cache reflects the state at the time it was first
computed within the ledger.

### Ledger 101 — totals updated, ledger advances

After advancing to ledger 101:
- The old cache key `BorrowRateCache(100)` is not consulted.
- The first call to `cached_borrow_rate` misses for key
  `BorrowRateCache(101)`, recomputes from the current storage, and returns
  **2_700 bps**.

## Edge cases covered

| Test | Scenario | Verifies |
|------|----------|----------|
| `cold_cache_matches_uncached_on_first_call` | No prior cache entry | Cold (empty) cache produces the same result as the uncached path |
| `cached_and_uncached_agree_same_ledger_multiple_calls` | 5 rapid calls in one ledger | Every call returns the same value |
| `totals_change_same_ledger_does_not_recompute_cache` | Totals mutated after first cache fill | Cache is stable within a ledger |
| `stale_cache_not_returned_after_ledger_advance_and_totals_update` | Totals change + ledger advance | Cache recomputes from new totals |
| `zero_debt_returns_base_rate_both_paths` | `total_debt = 0` | Both paths agree at 0 utilization |
| `zero_supply_falls_back_to_zero_utilization` | `total_supply = 0` | Falls back to 0 utilization, both paths agree |
| `above_kink_utilization_matches_across_ledger_advance` | Above-kink jump multiplier region | Both paths agree in jump region and across ledger advance |

## Invariant summary

```
for any ledger L, any sequence of storage mutations M within L:
    uncached_borrow_rate(env at start of M) == cached_borrow_rate(env after M)
                                                  == cached_borrow_rate(env at start of M)

for any two ledgers L1, L2 where L1 < L2:
    cached_borrow_rate at L2 reflects totals stored at L2,
      NOT totals at L1 (even if L1 cache entry exists)
```

## Re-running the tests

```sh
cargo test -p stellarlend-lending borrow_rate_cache_equiv
```
