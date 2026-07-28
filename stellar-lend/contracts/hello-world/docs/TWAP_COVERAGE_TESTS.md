# TWAP Coverage Tests

## Purpose

`has_window_coverage` and `find_snapshot_at_or_before` are the two gateway
functions that every TWAP price query passes through before any arithmetic is
done.  If either of them returns the wrong answer — or panics — every caller
sees a corrupt or reverted price.

These tests lock down the contracts between the implementation and its callers:

* **`has_window_coverage`** – decides whether the on-chain snapshot ring-buffer
  contains enough history to serve a TWAP for a given look-back window.
* **`find_snapshot_at_or_before`** – performs a binary search over the
  ring-buffer to locate the anchor snapshot that brackets the start of a TWAP
  window.

Without systematic coverage, subtle off-by-one errors (e.g., `<` vs `≤` in the
binary search) or missing early-returns (e.g., empty buffer, `window_secs`
below minimum) can silently corrupt prices in production.

---

## Test Scenarios

### `has_window_coverage`

| # | Scenario | Expected |
|---|----------|----------|
| 1 | No pool state at all (no storage entry) | `false` — early return without panic |
| 2 | `window_secs < MIN_WINDOW_SECS` | `false` — first guard rejects small windows unconditionally |
| 3 | Oldest snapshot is **newer** than window start | `false` — no anchor found |
| 4 | Oldest snapshot is **older** than window start | `true` — anchor found, elapsed time sufficient |
| 5 | Snapshot exactly **at** window start boundary | `true` — `≤` comparison qualifies the boundary |
| 6 | Snapshot **one second after** window start | `false` — boundary exclusive on the snapshot side |
| 7 | Pool state exists but **no snapshot yet**, sufficient elapsed time | `true` — falls back to `last_timestamp` |
| 8 | Pool state exists but **no snapshot yet**, insufficient elapsed time | `false` — fallback also insufficient |

### `find_snapshot_at_or_before`

| # | Scenario | Expected |
|---|----------|----------|
| 1 | Empty buffer | `None` — no comparison made, no panic |
| 2 | Target is an **exact hit** (middle or last) | `Some(snapshot)` with matching timestamp |
| 3 | Target falls **between** two snapshots | `Some(lower bound)` — nearest snapshot ≤ target |
| 4 | Target is **before** the first snapshot | `None` — no anchor satisfies `snap.ts ≤ target` |
| 5 | Target equals the **first** snapshot | `Some(first)` — left-boundary path |
| 6 | Single-element buffer, exact hit | `Some(that element)` |
| 7 | Single-element buffer, target above | `Some(that element)` |
| 8 | Single-element buffer, target below | `None` |
| 9 | Target far beyond the last snapshot | `Some(last)` — rightmost entry |

---

## Worked Example

Assume the ring-buffer holds three snapshots:

```
Index:  0      1      2
       T=100  T=200  T=300
```

Current ledger time: **T = 320**.  Requested window: **25 s**.

```
target_start = 320 − 25 = 295
```

**`find_snapshot_at_or_before(snaps, 295)`** runs a binary search:

1. `mid = 1`, `snaps[1].timestamp = 200 ≤ 295` → record candidate `T=200`, search right half.
2. `mid = 2`, `snaps[2].timestamp = 300 > 295` → do not update candidate, search left half.
3. Search exhausted → return `Some(T=200)`.

**`has_window_coverage`** then checks:

```
elapsed = now − snap.timestamp = 320 − 200 = 120
120 ≥ MIN_WINDOW_SECS (25)  →  true
```

The TWAP query proceeds with `T=200` as its start anchor.

---

## Edge Case Notes

### Empty buffer
The binary search initialises with `lo = 0, hi = len − 1` but immediately
returns `None` when `len == 0` before any indexing.  Confirmed by
`find_snapshot_returns_none_for_empty_buffer`.

### `≤` vs `<` at the boundary
The search condition `snap.timestamp ≤ target_ts` means a snapshot placed
**exactly at** the window start qualifies.  This is the intended behaviour:
if we have exactly `MIN_WINDOW_SECS` of history, we can serve the window.
See `coverage_true_when_snapshot_exactly_at_window_start` and its mirror
`coverage_false_when_snapshot_one_second_after_window_start`.

### No-snapshot fallback
When a pool has been updated at least once but the periodic snapshot writer
has not yet fired (first update is within the first `SNAPSHOT_INTERVAL_SECS`),
`has_window_coverage` falls back to `pool_state.last_timestamp`.  Tests
`coverage_true_with_pool_state_but_no_snapshots_after_enough_elapsed_time` and
`coverage_false_with_pool_state_but_no_snapshots_and_insufficient_elapsed_time`
cover both branches of this fallback.

### Binary search comparison count
`snapshot_search_metrics_for_test` exposes the internal step counter so tests
can assert O(log n) comparisons and that the empty-buffer path makes zero
comparisons.  This guards against accidental O(n) regressions.
