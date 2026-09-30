# Outbound Cap: Rationale and Behaviour

This document describes the outbound per-window value cap added to the
Bridge contract. The outbound cap mirrors the existing inbound cap but
maintains fully independent windowing and state so that a compromised
relayer cannot drain reserves beyond a configured rate.

Rationale
- Mirror inbound cap semantics for symmetry and defense-in-depth.
- Fail-closed by default (cap defaults to `0`) so operators must opt in.
- Independent rolling windows for inbound and outbound make attacks
  against one direction unable to affect the other.

API
- `set_outbound_cap(max_per_window: i128, window_size: u64, current_time: u64)`
  - Configure the outbound cap. `max_per_window == 0` means "no outbound".
  - `window_size` must be > 0.
  - Resets the current outbound window to begin at `current_time` and clears
    the running total.
- `admit_outbound(amount: i128, current_time: u64)`
  - Admit an outbound transfer of `amount` against the configured cap.
  - Rejects negative amounts, unconfigured (zero) cap, overflow, and
    attempts that would exceed the cap. On success the amount is added to
    the outbound window running total.

Worked example

1. Operator configures a daily outbound cap of 1000 units at ledger time 0:

   `set_outbound_cap(1000, 86_400, 0)`

2. The bridge admits 200 units at time 1000: `admit_outbound(200, 1000)`.
   Running total becomes 200.

3. Another 800 units at time 10_000: `admit_outbound(800, 10_000)`.
   Running total becomes 1000 (exact cap) and is permitted.

4. Any further outbound within the same window (before time >= 86_400)
   will be rejected.

5. At time 86_400 (or later), the window rolls and the running total
   resets. Outbound traffic may resume up to the cap in the new window.

Edge cases and notes
- Fail-closed: freshly constructed Bridge has `max_outbound_per_window == 0`.
- Reconfiguring via `set_outbound_cap` clears the prior running total and
  starts a new window at the provided `current_time`.
- Both inbound and outbound use checked `i128` arithmetic for totals and
  reject overflow rather than panicking.

See the test suite `src/outbound_cap_test.rs` for precise expectations.
