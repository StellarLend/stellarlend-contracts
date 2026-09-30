# Inbound Window Tuning

This guide explains how the bridge rolling-window inbound cap works and how an
operator should choose `max_per_window` and `window_size` for different risk
postures.

The cap is a defense-in-depth control. Validator quorum and epoch checks decide
whether an inbound message is authorized. The inbound window limits how much
authorized inbound value can be admitted before operators get time to detect and
respond to a bad validator set, relayer bug, or integration failure.

## Algorithm

`Bridge::admit_inbound(amount, current_time)` tracks four fields:

- `max_per_window`: maximum cumulative inbound value allowed in the window.
- `window_size`: length of the window in ledger-time seconds.
- `window_start`: ledger time where the current window began.
- `window_inbound_total`: value already admitted in the current window.

For each inbound amount:

1. Reject negative `amount`.
2. Reject when `max_per_window == 0`; this is fail-closed, not unlimited.
3. If `current_time >= window_start + window_size`, start a fresh window at
   `current_time` and reset `window_inbound_total` to zero.
4. Compute `window_inbound_total + amount` with checked arithmetic.
5. Reject if the new total is greater than `max_per_window`.
6. On success only, store the new total.

Failed admission never mutates the running total. A rejected over-cap transfer
does not consume any capacity, so a later smaller transfer can still fit.

## Fail-Closed Startup

A fresh `Bridge` starts with:

```text
max_per_window = 0
window_size = 86_400
window_start = 0
window_inbound_total = 0
```

That means day-one inbound flow is disabled until an operator calls
`set_inbound_cap(max_per_window, window_size, current_time)` with a positive
cap. This is intentional: the safe default is to require an explicit operating
limit before any value can enter.

Use an explicit zero cap as an emergency pause. It rejects all inbound amounts,
including zero-value transfers, until a positive cap is configured again.

## Parameter Selection

Choose `max_per_window` as the largest loss you are willing to tolerate before
manual intervention, then choose `window_size` as the detection and response
period.

| Posture | Example config | Throughput implication | Use when |
|---|---:|---|---|
| Emergency pause | `max_per_window = 0`, `window_size = 86_400` | No inbound flow | Incident response, initial deployment before funding |
| Conservative | `max_per_window = 1_000`, `window_size = 86_400` | Up to 1,000 units per day | New deployment, thin liquidity, manual monitoring |
| Balanced | `max_per_window = 10_000`, `window_size = 86_400` | Up to 10,000 units per day | Normal operations with daily review |
| Permissive | `max_per_window = 20_000`, `window_size = 3_600` | Up to 20,000 units per hour | Mature monitoring and automated alerting |

Shorter windows replenish capacity sooner but increase the maximum value that
can move over a day. For example, `20_000` per hour allows up to `480_000` units
over 24 hours if every hourly window is filled.

## Conservative Worked Example

Configure a conservative daily cap at time zero:

```text
set_inbound_cap(max_per_window = 1_000, window_size = 86_400, current_time = 0)
```

Then:

```text
admit_inbound(600, current_time = 100)  -> Ok, total = 600
admit_inbound(400, current_time = 200)  -> Ok, total = 1_000
admit_inbound(1,   current_time = 300)  -> Err, total stays 1_000
```

At exactly one full window later, capacity replenishes:

```text
admit_inbound(1_000, current_time = 86_400) -> Ok
window_start = 86_400
window_inbound_total = 1_000
```

## Permissive Worked Example

Configure an hourly cap:

```text
set_inbound_cap(max_per_window = 20_000, window_size = 3_600, current_time = 10)
```

Then three same-window admissions can fill the hour exactly:

```text
admit_inbound(7_500, current_time = 100) -> Ok, total = 7_500
admit_inbound(7_500, current_time = 200) -> Ok, total = 15_000
admit_inbound(5_000, current_time = 300) -> Ok, total = 20_000
```

Any additional value before `current_time = 3_610` is rejected. At
`current_time = 3_610`, the window rolls and the full hourly cap is available
again.

## Operator Checklist

- Configure a positive cap before enabling production inbound flow.
- Keep `max_per_window` below the amount operators can tolerate losing within
  the response window.
- Use a shorter `window_size` only when monitoring and incident response are
  fast enough to justify the higher daily throughput.
- Use `max_per_window = 0` to pause inbound flow during incidents.
- Reconfigure deliberately: `set_inbound_cap` starts a clean window and clears
  the prior running total.
