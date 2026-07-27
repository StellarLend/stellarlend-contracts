# Vesting Invariants

## Formula

`Grant::vested_at(now)` computes the vested amount at Unix timestamp `now`:

```
cliff_end = start_seconds + cliff_seconds

if now < cliff_end:
    vested = 0
elif duration_seconds == 0:
    vested = total
else:
    end = start_seconds + duration_seconds
    effective = min(now, end)
    elapsed = effective - start_seconds
    vested = (total * elapsed) / duration_seconds
```

## Worked Example

A grant with `total = 1_000`, `start_seconds = 1_000`,
`cliff_seconds = 200`, `duration_seconds = 800`:

| `now` | `vested_at` | Notes |
|-------|-------------|-------|
| 1_199 | 0 | Before cliff end (1_200) |
| 1_200 | 0 | Exactly at cliff end — `elapsed = 0` |
| 1_400 | 250 | 200 s after cliff: `(1000 * 200) / 800` |
| 1_600 | 500 | 400 s after cliff |
| 1_800 | 750 | 600 s after cliff |
| 2_000 | 1_000 | Full duration elapsed |
| 9_999 | 1_000 | Capped by `end` |

## Invariants (verified by proptest)

### 1. Monotonicity

For any grant `g` and timestamps `t1 ≤ t2`:

```
g.vested_at(t1) ≤ g.vested_at(t2)
```

The vested amount is a non-decreasing function of time. This is guaranteed because
`elapsed` is monotonic in `now` (capped at `end`) and the division is a
positive-scalar multiplication.

### 2. Principal bound

For any grant `g` and timestamp `t`:

```
g.vested_at(t) ≤ g.total
```

The vested amount never exceeds the grant principal. This holds because the numerator
`total * elapsed ≤ total * duration_seconds`, so the quotient is at most `total`.

### 3. Cliff zero

For any grant `g` with `cliff_seconds > 0` and any `now < start_seconds + cliff_seconds`:

```
g.vested_at(now) = 0
```

No tokens vest before the cliff boundary.
