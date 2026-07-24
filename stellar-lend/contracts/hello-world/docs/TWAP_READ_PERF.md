# TWAP read-cost budget

`get_twap` reads the per-asset snapshot vector and selects the latest snapshot
at or before `target_start = now - window_secs`. This document records the
maximum vector size and lookup-comparison budget enforced by tests.

## Cost model

A TWAP read has two snapshot-related costs:

1. Soroban storage deserializes the snapshot vector. This is linear in the
   retained entry count, but `MAX_SNAPSHOTS = 1,440` gives it a fixed ceiling.
2. `find_snapshot_at_or_before` searches the ordered vector. Binary search
   bounds this step to `O(log n)` comparisons.

For a non-empty vector, the worst-case search budget is:

```text
comparisons(n) = ceil(log2(n + 1))
comparisons(1,440) = ceil(log2(1,441)) = 11
```

`TWAP_READ_SEARCH_COMPARISON_BUDGET` therefore fixes the maximum search cost at
11 snapshot comparisons. A linear scan could require 1,440 comparisons and
would fail the budget tests.

## Benchmark matrix

`twap_read_bench_test.rs` exercises the complete stable-price `get_twap` path
and instruments its snapshot lookup at increasing retained counts:

| Snapshots | Worst-case comparison budget |
|---:|---:|
| 1 | 1 |
| 4 | 3 |
| 16 | 5 |
| 64 | 7 |
| 512 | 10 |
| 1,440 | 11 |

Comparison counts are deterministic and more suitable for a CI budget than
wall-clock timings, which vary with the host and Soroban SDK build profile.

## Worked example

For 64 snapshots ordered at 60-second intervals, a target between the 33rd and
34th entries must resolve to the 33rd entry. Binary search halves the remaining
range on each comparison and needs at most:

```text
ceil(log2(64 + 1)) = 7 comparisons
```

The edge-case matrix also checks a target before the first snapshot, the exact
first and last timestamps, a middle gap, and a target after the last snapshot.

## Enforced bounds

- Snapshot storage/deserialization: at most 1,440 entries per asset.
- Ordered snapshot search: at most 11 comparisons per TWAP read.
- Search behavior: latest timestamp less than or equal to the target, including
  vector ends and a middle gap.
- Price behavior: a stable 1:1 pool still returns `PRICE_SCALE` at every tested
  snapshot count.

The current bounded vector plus binary search meets the read budget. Changing
the storage layout to individually keyed snapshots would be required to make
deserialization itself logarithmic; that migration is not necessary to stop
cost growth because the retained vector is already capped.

See [TWAP_SNAPSHOT_POLICY.md](./TWAP_SNAPSHOT_POLICY.md) for ring-buffer sizing
and eviction guarantees.
