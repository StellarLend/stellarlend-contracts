# Window Guard Security Architecture

This document describes the security rationale and implementation details for the inbound value limits system within the bridge contract.

## Security Rationale

The bridge contract utilizes a rolling time window mechanism to rate-limit the total value that can cross the bridge. This prevents large catastrophic exploits by ensuring the maximum value extraction within a given time frame is bounded. 

Historically, this mechanism could suffer from three significant vulnerabilities:
1. **Zero-Window Exploit (Division/Modulo by Zero):** If a window size could be initialized or misconfigured to `0`, subsequent time-bound calculations could theoretically fail or create infinite rate allowances.
2. **Clock-Rollback Anomalies:** If the relayer providing `current_time` manages to pass a timestamp older than the current `window_start`, an unpatched window rolling mechanism could improperly "reset" its bounds or fail gracefully, potentially wiping the current inbound total and allowing an attacker to double-dip the inbound limits.
3. **Integer Overflow via Max Timestamps:** Unchecked additions on window boundaries could result in panic, bringing the contract execution to a halt and creating a denial-of-service vector when extreme time boundaries are provided.

## Patched Protections

To protect against these anomalies, we implemented the following bounds directly inside `lib.rs`:

1. **Explicit Zero-Length Guard:** `set_inbound_cap` now immediately returns a strictly-typed `BridgeError::InvalidWindowSize` if operators attempt to configure `window_size = 0`.
2. **Backward Time Validation:** Time monotonically moving backwards (`current_time < window_start`) triggers an early exit inside `roll_window_if_expired()`. The current window remains active, and the running `window_inbound_total` continues to accrue normally against the un-rolled window limits. 
3. **Checked Arithmetic Boundaries:** `window_start.checked_add(window_size)` replaces standard native or saturating operators. If a timestamp mathematically overflows the platform's integer limit, it naturally avoids rolling, thereby keeping the current window strict.

## Mathematical Example

### Before the Patch: Non-Monotonic Exploit
1. Let `window_start = 1000` and `window_size = 100`. (Window: 1000 to 1100). Limit: 1000 units.
2. An attacker relays a transaction at `current_time = 1050` admitting 500 units.
    - Total used: 500. Limit remaining: 500.
3. The attacker relays a transaction with a manipulated `current_time = 900`. 
    - Unpatched logic might miscalculate `window_start + window_size`, or misinterpret `900` relative to `1000` using unchecked subtraction or incorrect monotonic assumptions, erroneously resetting `window_start` to `900` and `window_inbound_total` to `0`.
4. The attacker relays another transaction at `current_time = 1050` admitting 1000 units. Total value passed: 1500 units (bypassing the 1000 limit).

### After the Patch: Monotonic Strictness
1. Let `window_start = 1000` and `window_size = 100`.
2. Relayer transmits `current_time = 1050`, admitting 500 units. Total used: 500.
3. Attacker tries `current_time = 900`, admitting 100 units.
    - `roll_window_if_expired()` sees `900 < 1000` and skips window roll.
    - Total used becomes `500 + 100 = 600`.
4. The window enforces its strict cap until `current_time >= 1100`.

## Operational Notes for Maintainers
- **Strictly Typed Errors:** Check for `BridgeError::InvalidWindowSize` specifically if automated deployment scripts ever fail on configuration.
- **Timestamp Sources:** Ensure the relayer node providing timestamps remains strongly synced with the ledger time. If a node drifts backward slightly, legitimate transactions might temporarily accrue against older window thresholds instead of rolling into fresh limit caps.
- **Testing:** Modify cases directly in `window_guard_test.rs` to validate further time-bound logic when expanding the bridge's capabilities.
