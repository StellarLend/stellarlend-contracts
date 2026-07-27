# TWAP Max-Buffer Lookup Budget

This document records the lookup-cost bound for `get_twap` when the snapshot
buffer is at maximum occupancy.

## Why this matters

`get_twap` serves a TWAP window by:

1. Extrapolating the current cumulative price to `now`.
2. Finding the latest snapshot at or before `target_start = now - window_secs`.
3. Dividing the cumulative delta by the elapsed time.

The expensive part is step 2 when the snapshot ring is full. We want a concrete,
reviewable bound for that worst case without changing the returned TWAP value.

## Buffer shape under test

The max-buffer tests fill the ring to:

```text
MAX_SNAPSHOTS = 1,440
SNAPSHOT_INTERVAL_SECS = 60
```

That represents a fully occupied 24-hour snapshot history.

## Measured lookup budget

`find_snapshot_at_or_before` already uses binary search. For a full ring of
1,440 snapshots, the worst-case comparison count is:

```text
ceil(log2(1440)) = 11
```

The new `twap_maxbuffer_perf_test.rs` assertions lock that budget in:

- Short window lookup: `<= 11` comparisons
- Long window lookup: `<= 11` comparisons
- Mid-gap lookup: `<= 11` comparisons

This gives a stable, deterministic performance bound that is much less brittle
than wall-clock benchmarking in unit tests.

## Worked example

Assume the ring contains snapshots at:

```text
60, 120, 180, ... , 86,400
```

If `target_start = 95`, the correct anchor is the `60` second snapshot.
The binary search does not walk linearly through the vector. Instead, it narrows
the search interval until it returns the latest snapshot whose timestamp is
`<= 95`, which is `60`.

The same logic applies near the tail of the buffer for short windows and near
the head of the buffer for long windows. In all cases the lookup cost remains
bounded by 11 comparisons for 1,440 entries.

## Edge cases covered

- Full buffer with a short query window near the newest snapshots
- Full buffer with a long query window near the oldest retained snapshots
- Target timestamp between two snapshots
- TWAP value unchanged at a stable 1:1 price while the ring is full

## What this does not change

- Snapshot write cadence
- Eviction policy
- TWAP math
- Returned prices

The only implementation addition is internal instrumentation that counts binary
search comparisons in tests, so reviewers can enforce the cost bound directly.
