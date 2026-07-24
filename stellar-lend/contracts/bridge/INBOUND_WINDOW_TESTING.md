# Inbound Rolling-Window Integration Testing

## Rationale

`Bridge::admit_inbound` enforces a per-window cumulative value cap using a
rolling window measured in ledger-time seconds. The window rolls automatically
(via `roll_window_if_expired`) on the next `admit_inbound` call that sees a
`current_time` at or past `window_start + window_size`.

Existing unit tests in `inbound_cap_test.rs` cover individual facets of this
behaviour (fail-closed, under-cap, at-cap, over-cap, rollover, overflow), but
none drives the full lifecycle — fill → reject → roll → refill — in a single
test with realistic, deterministic timestamps. The integration test in
`inbound_window_integration_test.rs` fills that gap.

## Worked Example

The integration test `inbound_window_full_lifecycle` uses the following setup:

| Parameter          | Value    |
|--------------------|----------|
| `max_per_window`   | 1_000    |
| `window_size`      | 100 sec  |
| `window_start`     | 0        |

### Stage by stage

| Step | Time | Action                   | Expected `window_inbound_total` | Rationale                          |
|------|------|--------------------------|---------------------------------|------------------------------------|
| 1a   | 10   | `admit_inbound(600, 10)` | 600                             | Under cap — admitted.              |
| 1b   | 20   | `admit_inbound(400, 20)` | 1_000                           | Lands exactly on cap — admitted.   |
| 2    | 30   | `admit_inbound(1, 30)`   | 1_000                           | Rejected (over cap), state frozen. |
| 3    | 200  | `admit_inbound(1_000, 200)` | 1_000                        | Window expired at 100; roll resets
|      |      |                          |                                 | total to 0, then admits 1_000.     |
| 4    | 250  | `admit_inbound(1, 250)`  | 1_000                           | Over cap again in new window.      |

At step 3 the window has clearly expired (`200 >= 0 + 100`), so
`roll_window_if_expired` fires, resets `window_inbound_total` to `0`, sets
`window_start = 200`, and the subsequent admission brings it back to `1_000`.

### Window start realignment after a long idle gap

If a bridge sits idle for many window lengths (say 10× `window_size`), the
stale pre-gap running total must not carry over. The test
`long_idle_gap_realigns_window` verifies this:

| Step | Time       | Action                    | Expected `window_start` | Expected `window_inbound_total` |
|------|------------|---------------------------|-------------------------|---------------------------------|
| 1    | 5          | `admit_inbound(300, 5)`   | 0                       | 300                             |
| 2    | 1_042      | `admit_inbound(1_000, 1_042)` | 1_042               | 1_000                           |

The window rolled at step 2 because `1_042 >= 0 + 100`, and the realignment
uses `current_time` (`1_042`), not a stale multiple of `window_size`. The old
total of 300 is discarded.

## Edge Cases Covered

| # | Scenario                          | Test Name                           | What it asserts                                                                 |
|---|-----------------------------------|-------------------------------------|---------------------------------------------------------------------------------|
| 1 | Unconfigured bridge (fresh)       | `unconfigured_bridge_rejects_all_inbound` | `admit_inbound` returns `BridgeError::InboundCapExceeded`; total stays 0.    |
| 2 | Zero cap explicitly configured    | `explicit_zero_cap_rejects_inbound` | `set_inbound_cap(0, ...)` is valid; `admit_inbound` always fails.              |
| 3 | Under-cap admission               | `inbound_window_full_lifecycle`     | Cumulative total increases; under-cap succeeds.                                |
| 4 | At-cap admission (exact boundary) | `inbound_window_full_lifecycle`     | Landing exactly on `max_per_window` is admitted.                               |
| 5 | Over-cap rejection                | `inbound_window_full_lifecycle`     | `admit_inbound` returns `InboundCapExceeded`; `window_inbound_total` unchanged.|
| 6 | Window roll resets total          | `inbound_window_full_lifecycle`     | After `current_time >= window_start + window_size`, total resets to 0.         |
| 7 | Refill after roll                 | `roll_resets_total_and_allows_refill` | Previously-rejected amounts become admissible in the new window.             |
| 8 | Negative amount                   | `negative_amount_rejected`          | Error message contains `must be >= 0`; state unchanged.                        |
| 9 | Arithmetic overflow               | `overflow_on_window_total_is_caught` | `checked_add` failure is caught; panic is avoided; total unchanged.            |
| 10 | Long idle gap                     | `long_idle_gap_realigns_window`     | Stale total from a previous window is not carried over after idle period.      |
| 11 | Typed error on over-cap           | (all over-cap tests)                | `err.downcast_ref::<BridgeError>()` yields `Some(&InboundCapExceeded)`.       |

## Design Notes

- **Time is deterministic.** Every test passes explicit `current_time` values
  so there is no dependency on wall-clock or ledger time — results are 100 %
  reproducible.
- **`roll_window_if_expired` is private.** It is exercised indirectly through
  `admit_inbound`, which is the correct integration-surface approach.
- **Fail-closed on zero.** The bridge defaults to `max_per_window = 0` and
  `set_inbound_cap(0, ...)` is a valid configuration. Both produce the same
  result: all inbound admissions are rejected with `InboundCapExceeded`.
- **Typed error for inbound.** Unlike the original implementation (which used
  plain `anyhow!` string errors), `admit_inbound` now uses
  `BridgeError::InboundCapExceeded`, matching the outbound side's use of
  `OutboundCapExceeded`. Callers can use `downcast_ref::<BridgeError>()` for
  precise error handling.
- **Overflow is caught, not panicked.** The `checked_add` on
  `window_inbound_total` ensures that an arithmetic overflow produces a
  controlled error rather than a panic, preserving the fail-closed invariant.
