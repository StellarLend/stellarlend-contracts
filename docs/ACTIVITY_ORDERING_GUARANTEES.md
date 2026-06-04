# Activity Feed Ordering and Pagination Guarantees

## Overview

`get_recent_activity` and `get_user_activity` return entries from a bounded
in-contract log (`ActivityLog`, max 10,000 entries). This document describes
the ordering contract, pagination semantics, and eviction behaviour that
indexers and UI consumers can rely on.

---

## Ordering

Entries are returned **newest-first** (reverse insertion order).

- Index `0` of the returned vector is always the most recently recorded entry.
- Timestamps are non-decreasing in insertion order, so the returned slice has
  non-increasing timestamps.
- Within a single ledger (same timestamp) the relative order of entries
  matches insertion order, reversed.

---

## Pagination

Both functions accept `limit: u32` and `offset: u32`.

| Condition | Result |
|---|---|
| `offset >= total` | Empty vector |
| `offset + limit > total` | Returns the remaining `total - offset` entries |
| `limit == 0` | Empty vector |
| `offset` very large (e.g. `u32::MAX/2`) | Empty vector, no panic |

Stable pagination: walking the log with consecutive `(limit, offset)` windows
covers every entry exactly once with no gaps and no overlaps, provided the log
is not modified between calls.

---

## Eviction

When the log reaches 10,000 entries the **oldest** entry (lowest insertion
index / lowest timestamp) is evicted via `pop_front` before the new entry is
appended. The log therefore always holds the most recent ≤ 10,000 entries.

---

## Per-User Feed

`get_user_activity` filters the global log by `Address` equality before
applying `limit`/`offset`. Each user's feed contains only their own entries;
no cross-user data leakage is possible.

---

## Event Schema

Activity entries carry an `activity_type: Symbol` field. Current values:

| Symbol | Emitted by |
|---|---|
| `"deposit"` | `deposit_collateral`, `deposit_collateral_asset` |
| `"borrow"` | `borrow`, `borrow_asset` |
| `"repay"` | `repay_debt`, `repay_asset` |
| `"withdraw"` | `withdraw`, `withdraw_asset`, `ca_withdraw_collateral` |
| `"liquidate"` | `liquidate` |

The `activity_type` symbol set is additive-only. Indexers should handle
unknown symbols gracefully rather than failing.

See `docs/EVENT_SCHEMA_VERSIONING.md` for the broader event versioning policy.

---

## Test Coverage

`src/tests/analytics_test.rs` contains deterministic tests for all guarantees
above:

| Test | Guarantee verified |
|---|---|
| `test_activity_ordering_newest_first_under_load` | Newest-first ordering |
| `test_activity_pagination_covers_full_log_under_load` | Full coverage, no gaps |
| `test_activity_pagination_no_overlap_between_pages` | No overlap between pages |
| `test_activity_log_eviction_at_capacity` | Cap enforced at 10,000 |
| `test_activity_log_eviction_drops_oldest_entry` | Oldest entry evicted first |
| `test_user_activity_feed_isolation_under_load` | Per-user isolation |
| `test_user_activity_feed_pagination_under_load` | User feed full coverage |
| `test_pagination_offset_equals_total_returns_empty` | Boundary: offset == total |
| `test_pagination_limit_larger_than_remaining_returns_remainder` | Partial last page |
| `test_pagination_zero_limit_returns_empty` | Zero limit |
| `test_pagination_large_offset_no_panic` | No overflow on large offset |


# Activity Ordering Guarantees

This document describes the ordering and pagination guarantees for StellarLend activity feeds.

## Ordering

Activities are ordered by two criteria:

1. **Ledger Sequence** (descending): Higher ledger numbers first
2. **Event Index** (descending): Within the same ledger, higher event indices first

This ordering is **stable** and **total** — every activity has a unique position in the sequence.

## Cursor Format

Pagination uses cursor-based navigation with the format:


base64(ledger_sequence:event_index)


Example: `MTAwMDow` decodes to `1000:0`

### Why Cursor-Based Pagination?

**Offset-based pagination** (`?page=2&limit=20`) fails when:
- New events arrive between requests
- Events are deleted or reordered
- The same offset points to different items on each call

**Cursor-based pagination** guarantees:
- No duplicates: Events before the cursor are never returned again
- No gaps: Events after the cursor are returned in order
- Stability: New events arriving don't affect existing pages

## Pagination Flow

### First Request
GET /api/lending/activity?limit=20

Response:
```json
{
  "data": [...],
  "pagination": {
    "nextCursor": "MTAwMDow",
    "hasMore": true,
    "limit": 20
  }
}


Subsequent Request
GET /api/lending/activity?cursor=MTAwMDow&limit=20

The server decodes MTAwMDow to ledger=1000, event=0, then starts from the next position (ledger=1000, event=1 or ledger=999).

End of Feed

When hasMore is false and nextCursor is null, all activities have been consumed.

Edge Cases

New Events Arriving
If new events are written to ledger 5001 while a client is paginating from 5000:

The client does not see the new events in their current pagination
The client can discover them by starting a new pagination from the latest cursor
Existing pages remain stable

Empty Ledgers
If a ledger has no lending activity, the cursor naturally advances to the next ledger with events. No special handling is needed.

Reorgs
Stellar has finality after ~5 seconds. The API assumes ledger sequences are immutable after this period. Cursors pointing to finalized ledgers remain valid indefinitely.

Implementation Notes
Cursors are opaque to clients — they should be treated as opaque strings
The server may change the cursor encoding without breaking clients
Clients should persist the nextCursor for resumable pagination
The limit parameter is advisory; the server may return fewer items

