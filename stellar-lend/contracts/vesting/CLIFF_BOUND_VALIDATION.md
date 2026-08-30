# Cliff Bound Validation in `add_grant`

## Rationale

A vesting schedule is defined by three timing parameters:

| Parameter          | Meaning                                              |
|--------------------|------------------------------------------------------|
| `start_seconds`    | Unix timestamp when vesting begins                   |
| `duration_seconds` | Total length of the vesting window                   |
| `cliff_seconds`    | Delay after `start` before any tokens are claimable  |

If `cliff_seconds > duration_seconds`, the cliff gate (`start + cliff`) fires
*after* the vesting end (`start + duration`). This creates a schedule where
`vested_at` is always 0 (because the cliff is never passed within the duration
window), permanently locking the grantee's principal with no path to recovery.

To prevent fund loss, `add_grant` now validates all three boundary conditions
**before** persisting the grant. A rejected call is a no-op: no tokens are
moved and `total_locked` is unchanged.

## Accepted Constraints

| Condition                          | Result   | Error                   |
|------------------------------------|----------|-------------------------|
| `total == 0`                       | Rejected | `ZeroPrincipal`         |
| `duration_seconds == 0`            | Rejected | `ZeroDuration`          |
| `cliff_seconds > duration_seconds` | Rejected | `CliffExceedsDuration`  |
| `cliff_seconds == duration_seconds`| Accepted | —                       |
| All parameters valid               | Accepted | —                       |

`cliff_seconds == duration_seconds` is explicitly accepted. In this case the
cliff gate and the end of vesting coincide: the grantee receives the full
principal in a single step at `start + duration`.

## Worked Example

```
total          = 1_000 tokens
start_seconds  = 1_000
duration_seconds = 800
cliff_seconds  = 200
```

Timeline:

```
t=1_000  start
t=1_200  cliff end (start + cliff)   → first tokens become claimable
t=1_400  50 % elapsed                → vested = 500
t=1_800  duration end                → vested = 1_000 (fully vested)
```

Boundary case — cliff equals duration:

```
cliff_seconds = duration_seconds = 800

t=1_000  start
t=1_800  cliff end == duration end   → entire 1_000 vests at once
```

Invalid case — cliff exceeds duration (rejected):

```
cliff_seconds = 900, duration_seconds = 800

cliff end = t=1_900 > duration end = t=1_800
→ add_grant returns Err(CliffExceedsDuration)
→ no grant is stored, total_locked unchanged
```

## Edge Cases

- **Zero principal** (`total == 0`): rejected immediately; nothing to vest.
- **Zero duration** (`duration_seconds == 0`): rejected; the linear formula
  would divide by zero.
- **`cliff > duration`**: rejected; permanently locks funds.
- **`cliff == duration`**: accepted; all tokens vest at the end of the window.
- **`cliff == 0`**: always valid; vesting begins at `start`.
- **Multiple grants per grantee**: each grant is validated independently;
  a rejected grant does not affect existing valid grants.
