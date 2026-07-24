# Pause-Aware Vesting Time Offset

## Overview

The vesting contract tracks cumulative paused duration and subtracts it from the
current ledger timestamp before feeding it into `Grant::vested_at`. This means a
pause truly freezes vesting accrual — it does not merely block claiming.

## State variables

| Key                | Type  | Purpose                                                         |
|--------------------|-------|-----------------------------------------------------------------|
| `Paused`           | bool  | Whether the contract is currently paused                        |
| `PausedAt`         | u64   | Ledger timestamp at which the current pause began              |
| `TotalPausedSecs`  | u64   | Cumulative seconds the contract has spent in a paused state    |

## Timeline diagram

```
Wall clock ────────────────────────────────────────────────────────────────►
           0    100   200   300   400   500   600   700   800   900  1000
           │     │     │     │     │     │     │     │     │     │     │
Ledger:    ├────▶│pause│◄────200s────►│resume│─────────────────────────────
                             (paused interval = 200 s)

Effective: ├──────────────────────────────────────────────────────────────►
           0    100   200   200   200   300   400   500   600   700   800
           (clock frozen during pause; resumes from where it left off)
```

After resuming, `effective_now = ledger_now - total_paused_secs`, so a grant
with a 1 000 s linear schedule will have vested `800/1000 × total_amount` at
ledger second 1 000 (i.e. 800 effective seconds, not 1 000).

## Worked example: pause then resume, then claim

```
Grant:  total_amount = 10_000 tokens
        start_ts     = 0
        cliff_secs   = 0
        duration     = 1_000 s

Timeline:
  t =   0  — initialize & create_grant
  t = 300  — pause()          (PausedAt = 300)
  t = 600  — resume()         (interval = 300 s → TotalPausedSecs = 300)
  t = 800  — claim()

At t = 800:
  effective_now = 800 − 300 = 500 s
  vested        = 10_000 × 500 / 1_000 = 5_000 tokens

Without the offset, vested would have been 10_000 × 800 / 1_000 = 8_000 tokens —
the 300-second pause interval would have accrued 3_000 extra tokens incorrectly.
```

## Multiple pause/resume cycles

Each `resume()` call adds `now - paused_at` to `TotalPausedSecs`:

```
pause at t=100, resume at t=200  → total_paused = 100
pause at t=400, resume at t=500  → total_paused = 200
claim at t=600  → effective_now = 600 − 200 = 400
```

## Edge cases

| Scenario                        | Behaviour                                              |
|---------------------------------|--------------------------------------------------------|
| Zero-length pause (pause+resume at same timestamp) | Adds 0 s — no effect   |
| Pause spanning the cliff        | Cliff is evaluated against `effective_now`, not wall clock |
| `claim` while paused            | Returns `VestingError::ContractPaused`                 |
| `revoke` while paused           | Returns `VestingError::ContractPaused`                 |
| Arithmetic overflow in accumulation | `saturating_add` / `saturating_sub` prevent panics |
