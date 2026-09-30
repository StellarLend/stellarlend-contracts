# Vesting Math Reference

## Schedule Parameters

Every `Grant` is defined by four fields:

| Field | Type | Meaning |
|-------|------|---------|
| `total` | `u128` | Principal — the full amount of tokens allocated |
| `start_seconds` | `u64` | Unix timestamp (seconds) when the schedule begins |
| `cliff_seconds` | `u64` | Seconds after `start` before any tokens vest |
| `duration_seconds` | `u64` | Seconds from `start` over which vesting is linear |

Derived constants:

```
cliff_end = start_seconds + cliff_seconds
end       = start_seconds + duration_seconds
```

---

## `vested_at(now)` — Vested amount at a timestamp

```
if now < cliff_end:
    return 0                                  // Cliff gate
if duration_seconds == 0:
    return total                              // Immediate full vest
end = start_seconds + duration_seconds
effective = min(now, end)
elapsed = effective - start_seconds
return (total * elapsed) / duration_seconds   // Linear ramp
```

**Cliff gate.** Before `cliff_end` the function short-circuits to zero. No tokens vest
until that boundary.

**Linear ramp.** After the cliff the vested amount grows linearly with `elapsed`.
The numerator `total * elapsed` fits in `u128` because `elapsed ≤ duration_seconds`,
so the product is bounded by `total * duration_seconds`. The integer division truncates
toward zero, so the reported amount never exceeds `total`.

**End cap.** When `now ≥ end` the effective timestamp is clamped to `end`, so the
vested amount after full duration is always exactly `total`.

---

## `claimable()` — Unclaimed vested tokens

```
return released - claimed
```

`released` tracks the latest `vested_at` value seen during a `sync` call.
`claimed` tracks the cumulative amount the grantee has withdrawn. The difference
is the amount ready to claim.

---

## Revoke split

`revoke(caller, grantee, now)`:

1. Calls `sync_grants`, which advances each grant's `released` to `vested_at(now)`.
2. For each non-revoked grant, computes `locked = total - released` (the unvested
   remainder).
3. Transfers the sum of all locked amounts to the treasury.
4. Resets each grant's `total` to its current `released` value and marks it as
   `revoked = true`.

After revoke, the grantee keeps the vested (released) portion and can still claim
it; the unvested portion is clawed back.

---

## Worked example

```
Grant:
  total    = 1_000
  start    = 1_000
  cliff    =   200      => cliff_end = 1_200
  duration =   800      => end       = 1_800
```

### `vested_at` schedule

| `now` | `vested_at` | Calculation |
|-------|-------------|-------------|
| 1_000 | 0 | Before cliff (`1_000 < 1_200`) |
| 1_199 | 0 | Before cliff (`1_199 < 1_200`) |
| 1_200 | 250 | `(1_000 * 200) / 800` |
| 1_400 | 500 | `(1_000 * 400) / 800` |
| 1_600 | 750 | `(1_000 * 600) / 800` |
| 1_800 | 1_000 | End reached (`800 / 800`) |
| 9_999 | 1_000 | Capped by end |

### Claim

Assume `claim("alice", 1_400)` is called. Before the transfer:

- `sync` advances `released` to `vested_at(1_400) = 500`.
- `claimable = 500 - 0 = 500`.

After claim:
- `claimed = 500`.
- Contract balance decreases by 500; grantee balance increases by 500.

A second call to `claim("alice", 1_600)` would compute `claimable = 750 - 500 = 250`
and transfer 250 more.

### Revoke

Call `revoke("admin", "alice", 1_400)` (no claim occurred before revoke):

1. `sync_grants` sets `released = vested_at(1_400) = 500`.
2. `locked = 1_000 - 500 = 500`.
3. 500 tokens are transferred to the treasury.
4. Grant's `total` is set to 500; `revoked = true`.

The grantee can no longer vest new tokens, but the 500 already vested remain
claimable via `claimable = 500 - 0 = 500`.

---

## Invariants

1. **Monotonicity** — `vested_at(t1) ≤ vested_at(t2)` for all `t1 ≤ t2`.
2. **Principal bound** — `vested_at(t) ≤ total` for all `t`.
3. **Cliff zero** — `vested_at(t) = 0` for all `t < start + cliff_seconds`.
4. **No unvested leak on revoke** — After revoke, `total = released ≤ total_original`,
   so the grantee never keeps unvested tokens.
