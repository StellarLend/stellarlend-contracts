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

Activity Ordering Guarantees

Overview

This document specifies the ordering and pagination guarantees for lending activity data returned by the API.

Ordering Model

Primary Key

Activity events are ordered by a composite key:
(ledger_sequence ASC, event_index ASC)

Where:
- ledger_sequence: The Stellar ledger sequence number. Monotonically increasing, immutable once closed.
- event_index: The position of the event within the ledger. Stable within a closed ledger.


# Why Ledger Sequence?

Stellar ledger sequences provide:

- Total ordering: Every event has a unique position in the global ledger history
- Immutability: Closed ledgers cannot change, so ordering is stable forever
- Monotonicity: New events always have higher ledger sequences than old ones
- Verifiability: Clients can verify ordering against the blockchain itself

# Event Index Stability
Within a single ledger, events are ordered by:

1 Transaction application order (as determined by consensus)
2 Event emission order within the transaction

The event_index captures this ordering and is stable for any given ledger.

# Pagination Cursor
Format
cursor = base64url(ledger_sequence + ":" + event_index)

Example:
ledger_sequence = 5000000
event_index = 13
cursor = base64url("5000000:13") = "NTAwMDAwMDoxMw"

Properties
| Property       | Guarantee                                            |
| -------------- | ---------------------------------------------------- |
| **Opaque**     | Clients must not parse or construct cursors manually |
| **Stable**     | Same event always produces same cursor               |
| **Comparable** | Cursors sort lexicographically by (ledger, index)    |
| **Stateless**  | Server can reconstruct position from cursor alone    |

# Cursor Lifecycle
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Page 1    │────→│   Page 2    │────→│   Page 3    │
│  cursor=null│     │ cursor="A"  │     │ cursor="B"  │
│  data[0..19]│     │  data[20..39]│     │  data[40..42]│
│ next="A"   │     │  next="B"   │     │  next=null   │
└─────────────┘     └─────────────┘     └─────────────┘


# Ordering Guarantees
G-1: No Missing Events
If a client paginates through all pages without gaps, every event present at the time of the first request will be returned exactly once.

Proof:
Events are ordered by immutable (ledger_sequence, event_index)
The cursor captures the exact position of the last returned event
The next page starts from the next position
New events have higher ledger sequences and appear after the cursor

G-2: No Duplicate Events
No event appears in more than one page for a single pagination sequence.
Proof:
Each page's cursor points to the position after the last event
The next query filters to events >= cursor position
Strict inequality ensures the last event of page N is excluded from page N+1

G-3: Stable Ordering Under Concurrent Writes
New events arriving between page requests do not affect the ordering or presence of already-returned events.
Proof:
New events have ledger_sequence >= current ledger
The cursor captures a position in past ledgers
Future ledger sequences are always > past cursor positions
Therefore new events sort after the cursor and don't affect past pages

G-4: Monotonic Cursor Progression
For any valid cursor C, the next cursor C' satisfies C' > C in the total order.

Formal:
∀C: decode(C) = (L, I) → decode(nextCursor(L, I)) = (L', I') where (L', I') > (L, I)


G-5: Bounded Page Size
Every page contains at most limit events, and the server enforces limit <= MAX_PAGE_SIZE.

Implementation:
const pageSize = Math.min(requestedLimit, MAX_PAGE_SIZE); // MAX_PAGE_SIZE = 100

G-6: Consistent Has-More Flag
hasNextPage is true if and only if there exist events after the current page's cursor.
Implementation:
Request limit + 1 events from the database
If results.length > limit, set hasNextPage = true and trim to limit
If results.length <= limit, set hasNextPage = false


Failure Scenarios
Scenario 1: Client Uses Expired Cursor
Problem: Client stores a cursor and resumes pagination after a long delay. The underlying data may have been compacted or archived.
Behavior: The API accepts any valid cursor. If the ledger has been archived, the query returns empty results (not an error). The client should detect hasNextPage = false and stop.

Scenario 2: Ledger Reorganization
Problem: In a consensus failure or fork, ledger N might be replaced with a different ledger N'.
Behavior: Stellar has instant finality (5 seconds). Once a ledger is closed, it is immutable. The cursor format assumes this property. If the network forks, clients may see duplicate or missing events across the fork boundary. This is handled at the network layer, not the API layer.

Scenario 3: Event Index Overflow
Problem: A ledger contains more than MAX_EVENT_INDEX (1,000,000) events.
Behavior: This is practically impossible on Stellar (typical ledger has < 1000 transactions). If it occurs, the cursor would overflow and pagination would fail. The API returns an error and the client must restart from the beginning.

# Comparison: Cursor vs Offset Pagination
| Feature             | Cursor (Ledger-Based) | Offset-Based             |
| ------------------- | --------------------- | ------------------------ |
| Stable under writes | ✅ Yes                 | ❌ No (duplicates/misses) |
| Time complexity     | O(log n) seek         | O(offset) scan           |
| Statelessness       | ✅ Stateless           | ❌ Requires count         |
| Random access       | ❌ Sequential only     | ✅ Yes                    |
| Total count         | ❌ Not provided        | ✅ Available              |
| Implementation      | Complex               | Simple                   |



Implementation Notes
Database Index
For optimal performance, the events table should have a composite index:
CREATE INDEX idx_events_ledger_index ON events(ledger_sequence, event_index);

Cursor Parsing
function parseCursor(cursor: string): { ledger: number; index: number } {
  const decoded = Buffer.from(cursor, 'base64url').toString('utf-8');
  const [ledger, index] = decoded.split(':').map(Number);
  return { ledger, index };
}

Query Construction
SELECT * FROM events
WHERE (ledger_sequence, event_index) > ($cursor_ledger, $cursor_index)
ORDER BY ledger_sequence ASC, event_index ASC
LIMIT $limit;

References
Stellar Ledger Concepts
Soroban Events
API Pagination Best Practices
