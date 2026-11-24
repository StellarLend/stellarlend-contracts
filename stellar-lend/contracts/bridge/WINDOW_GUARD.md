# Window Guard Security Architecture

This document describes the security rationale and implementation details for the inbound value limits system within the bridge contract.

## Security Rationale

The bridge contract utilizes a rolling time window mechanism to rate-limit the total value that can cross the bridge. This prevents large catastrophic exploits by ensuring the maximum value extraction within a given time frame is bounded. 

Historically, this mechanism could suffer from three significant vulnerabilities:
1. **Zero-Window Exploit (Division/Modulo by Zero):** If a window size could be initialized or misconfigured to `0`, subsequent time-bound calculations could theoretically fail or create infinite rate allowances.
2. **Clock-Rollback & Caller-Controlled Timestamp Anomalies:** If an unauthenticated caller or relayer could invoke `admit_inbound` / `admit_outbound` directly with an arbitrary `current_time`, they could pass a far-future timestamp to roll the window forward (resetting the running total to `0`), pass a backward timestamp to confuse unpatched window bounds, or pass a junk amount to exhaust the window cap and block real bridge traffic.
3. **Integer Overflow via Max Timestamps:** Unchecked additions on window boundaries could result in panic, bringing the contract execution to a halt and creating a denial-of-service vector when extreme time boundaries are provided.

## Patched Protections

To protect against these anomalies, we implemented the following bounds directly inside `lib.rs`:

1. **Explicit Zero-Length Guard:** `set_inbound_cap` and `set_outbound_cap` immediately return a strictly-typed `BridgeError::InvalidWindowSize` if operators attempt to configure `window_size = 0`.
2. **Internal Ledger-Time Admission Only:** Public `admit_inbound` and `admit_outbound` entrypoints have been removed so callers cannot supply arbitrary `current_time` values to roll the window or burn capacity with junk amounts. Instead, admission is performed internally via `admit_inbound_internal` (called from `consume_inbound_message`) and `admit_outbound_internal` (called from `send`), both reading `env.ledger().timestamp()`.
3. **Backward Time Validation:** Time monotonically moving backwards (`current_time < window_start`) triggers an early exit inside `maybe_roll_window()`. The current window remains active, and the running `window_inbound_total` continues to accrue normally against the un-rolled window limits.
4. **Checked Arithmetic Boundaries:** `window_start.checked_add(window_size)` replaces standard native or saturating operators. If a timestamp mathematically overflows the platform's integer limit, it naturally avoids rolling, thereby keeping the current window strict.

## Mathematical Example

### Before the Patch: Caller-Supplied Timestamp Exploit
1. Let `window_start = 1000` and `window_size = 100`. (Window: 1000 to 1100). Limit: 1000 units.
2. An attacker relays a transaction at `current_time = 1050` admitting 500 units.
    - Total used: 500. Limit remaining: 500.
3. The attacker calls a public `admit_inbound` with a manipulated `current_time = 2000` (or `900`) to roll the window forward or reset `window_inbound_total` to `0`, or passes `amount = 500` to exhaust the remaining cap and block real bridge traffic.
4. The attacker relays another transaction admitting 1000 units. Total value passed: 1500 units (bypassing the 1000 limit).

### After the Patch: Ledger-Time Monotonic Strictness
1. Let `window_start = 1000` and `window_size = 100`.
2. `consume_inbound_message` runs at ledger timestamp `1050`, admitting 500 units via `admit_inbound_internal`. Total used: 500. `get_inbound_window_remaining()` reports `500`.
3. No external `admit_inbound` or `admit_outbound` entrypoint exists, and callers cannot pass a custom timestamp. `maybe_roll_window()` only consults `env.ledger().timestamp()`.
4. The window enforces its strict cap until ledger time reaches `>= 1100`.

## Operational Notes for Maintainers
- **Strictly Typed Errors:** Check for `BridgeError::InvalidWindowSize` specifically if automated deployment scripts ever fail on configuration.
- **Timestamp Sources:** Window accounting uses `env.ledger().timestamp()` exclusively inside `consume_inbound_message` and `send`. Use `get_inbound_window_remaining()` and `get_outbound_window_remaining()` for read-only capacity monitoring.
- **Testing:** Modify cases in `inbound_window_integration_test.rs` and `window_guard_test.rs` to validate further time-bound logic when expanding the bridge's capabilities.
