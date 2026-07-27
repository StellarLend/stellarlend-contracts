# TWAP Snapshot Ring-Buffer Policy

This document describes the bounded snapshot storage policy for the StellarLend
AMM TWAP accumulator (`amm_twap.rs`), its sizing rationale, the eviction
algorithm, and the correctness guarantees that ensure `get_twap` always resolves
after eviction.

---

## Motivation

The TWAP accumulator persists a `Vec<TwapSnapshot>` per asset in Soroban
persistent storage. Without a cap:

- The vector grows indefinitely as the pool ages.
- Every `get_twap` read deserialises the entire vector, making read cost
  proportional to pool age.
- Storage rent climbs without bound, eventually making reads unaffordable for
  long-lived pools.

The fix is a **fixed-capacity ring buffer** that evicts the oldest entry once
the cap is reached, keeping read cost and rent bounded regardless of pool age.

---

## Constants

| Constant                 | Value    | Meaning                                                  |
|--------------------------|----------|----------------------------------------------------------|
| `SNAPSHOT_INTERVAL_SECS` | 60 s     | Minimum gap between consecutive snapshots.               |
| `MAX_TWAP_WINDOW_SECS`   | 86 400 s | Maximum `window_secs` supported by `get_twap` (24 h).   |
| `MAX_SNAPSHOTS`          | 1 440    | Ring-buffer capacity per asset.                          |
| `EVICTION_SAFETY_FACTOR` | 2        | Multiplier applied to `MAX_TWAP_WINDOW_SECS` for safety. |

### Sizing derivation

```text
MAX_SNAPSHOTS = MAX_TWAP_WINDOW_SECS / SNAPSHOT_INTERVAL_SECS
             = 86_400 / 60
             = 1_440
```

At one snapshot per 60 s, 1 440 snapshots cover exactly 24 hours of history —
enough to serve any `get_twap` call with `window_secs ≤ MAX_TWAP_WINDOW_SECS`.

The `EVICTION_SAFETY_FACTOR = 2` means the oldest entry in a full ring is at
least `MAX_TWAP_WINDOW_SECS × 2 = 48 h` old before it may be evicted.  This
creates a two-window safety margin so that:

1. A pool that has not seen any swap for a full 24-hour window still has a valid
   start anchor for the longest supported query.
2. Transient ledger-timestamp jitter or delayed writes cannot accidentally evict
   an entry that is still needed.

### Storage cost bound

At `1 440` snapshots × ~48 bytes per entry, each asset consumes at most
≈ 69 KiB of persistent storage. Rent is therefore bounded and predictable,
independent of how long the pool has been active.

---

## Write Throttle

`maybe_write_snapshot` is called on every `update_twap_accumulators` invocation,
but only persists a new entry when:

```
state.last_timestamp − last_snap.timestamp ≥ SNAPSHOT_INTERVAL_SECS
```

At a block time of ≈ 5 s, this limits snapshots to at most one per 12 ledger
closes, keeping the write rate proportional to real-time elapsed rather than
transaction count.

---

## Eviction Algorithm

When a new snapshot would push the ring past `MAX_SNAPSHOTS`:

1. **Read the oldest snapshot** (index 0, the head of the ring).
2. **Compute its age**: `age = now − oldest.timestamp`.
3. **Apply the safety gate**:
   - If `age > MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR`:  
     Remove index 0, then append the new snapshot.  Ring stays at `MAX_SNAPSHOTS`.
   - Otherwise:  
     **Skip the write entirely.**  The ring retains all existing snapshots until
     the oldest is old enough to evict safely.

The skip branch is hit only during a pool's initial fill-up phase (the first
`MAX_SNAPSHOTS` seconds × `EVICTION_SAFETY_FACTOR` of its life), after which
the ring rotates steadily.

### Amortised cost

Under steady-state operation each `maybe_write_snapshot` call that triggers
eviction removes exactly one entry and appends one entry: **O(1) amortised**.
The underlying `Vec::remove(0)` is O(n) in Soroban's SDK (element shift), but
`n` is bounded by `MAX_SNAPSHOTS = 1 440`, so the absolute cost is constant.

### Why not a circular-index pointer?

Soroban persistent storage serialises the entire `Vec` on every write; there is
no in-place mutation.  A true ring buffer with a head-pointer would require
storing an extra index or reordering elements on every read, neither of which
reduces the serialisation cost.  The `remove(0)` approach is equivalent in
on-chain cost and is simpler to audit.

---

## Lookup: `find_snapshot_at_or_before`

`get_twap` uses a **binary search** over the snapshot ring to find the entry
with the greatest timestamp ≤ `target_ts`.

```
target_ts = now − window_secs
```

### Complexity

O(log `MAX_SNAPSHOTS`) = O(log 1440) ≈ 11 comparisons.

### Correctness after eviction

Because the eviction safety gate ensures:

```
oldest_retained.timestamp ≥ now − MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR
```

…and every valid `get_twap` call satisfies `window_secs ≤ MAX_TWAP_WINDOW_SECS`,
the binary search is guaranteed to find at least one qualifying anchor:

```
target_ts = now − window_secs
          ≥ now − MAX_TWAP_WINDOW_SECS
          > now − MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR
          ≥ oldest_retained.timestamp
```

In other words, after the ring reaches steady-state rotation the oldest snapshot
is always older than `target_ts`, so it always qualifies as a valid start anchor
for any supported window.

---

## Invariants

| Invariant | Description |
|-----------|-------------|
| **Bounded size** | `snaps.len() ≤ MAX_SNAPSHOTS` at all times. |
| **Monotone order** | Snapshots are ordered strictly by ascending timestamp. |
| **Write rate** | At most one snapshot per `SNAPSHOT_INTERVAL_SECS` wall-clock seconds. |
| **Safety gate** | No snapshot is evicted while its age ≤ `MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR`. |
| **TWAP correctness** | `get_twap(window_secs)` resolves correctly for any `window_secs ≤ MAX_TWAP_WINDOW_SECS` after the ring reaches steady-state. |
| **No retroactive math change** | Eviction never alters `TwapPoolState` (cumulative values, reserves, timestamp). Only the snapshot ring is trimmed. |

---

## Operational Notes

### New pools (fill-up phase)

During the first `MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR` seconds (≈ 48 h)
of a pool's life, the ring fills up but the oldest entry is not yet old enough to
evict.  In this phase, writes that would overflow are silently skipped.

From the caller's perspective:
- `get_twap` with `window_secs ≤ total elapsed time` works correctly.
- `get_twap` with `window_secs > total elapsed time` falls back to the earliest
  available snapshot and requires at least `MIN_WINDOW_SECS` of history.

### Monitoring

Off-chain monitoring can inspect the ring by reading the `TwapSnaps(asset)` key
from persistent storage.  The ring length should be stable at `MAX_SNAPSHOTS`
for any pool older than 48 h.

### Changing `MAX_SNAPSHOTS`

If `MAX_SNAPSHOTS` is decreased in a contract upgrade, existing rings may be
longer than the new cap.  The eviction logic will drain excess entries over time
(one per interval) without requiring a migration.  `get_twap` remains correct
throughout because the binary search always finds the nearest qualifying anchor.

If `MAX_SNAPSHOTS` is increased, larger rings will accumulate naturally. Rent
will increase proportionally.

---

## Test Coverage (`twap_eviction_test.rs`)

| Test | Verifies |
|------|----------|
| `cap_exactly_reached_then_eviction_keeps_ring_at_max` | Ring is at MAX_SNAPSHOTS after exactly MAX_SNAPSHOTS writes; one more write evicts and stays at cap. |
| `many_writes_never_exceed_cap` | 3 × MAX_SNAPSHOTS writes never let ring exceed cap. |
| `twap_within_window_correct_after_eviction` | TWAP result is unchanged by eviction. |
| `window_boundary_snapshot_never_evicted_prematurely` | Safety gate prevents eviction when oldest entry is still within the safety threshold. |
| `oldest_evicted_once_safely_outside_window` | Oldest snapshot advances after safe eviction. |
| `find_snapshot_resolves_correctly_after_eviction` | Binary search finds correct start anchor after eviction. |
| `twap_resolves_on_sparse_ring` | Sparse ring falls back to available history gracefully. |
| `independent_assets_have_independent_rings` | Two pools do not share or contaminate each other's ring. |
| `ring_is_monotonically_ordered_after_evictions` | Timestamp ordering is preserved after many eviction cycles. |
