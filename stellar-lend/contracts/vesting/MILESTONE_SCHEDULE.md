# Milestone Vesting Schedule

## Rationale

The original vesting contract (as described in PR #502) supported only a
**linear** schedule with a cliff: tokens vest continuously from a start time
to an end time, with nothing vested before a configurable cliff.

Many real-world token allocations, however, vest in **discrete tranches** at
fixed calendar dates — for example 25 % on each of four quarterly
anniversaries.  The milestone schedule formalises that pattern so the vesting
contract can express both continuous and discrete unlocks.

## How It Works

A milestone schedule is an ordered list of `(timestamp, cumulative_amount)`
pairs.

At any point in time the vested amount is simply the `cumulative_amount` of
the **latest milestone whose timestamp has passed**.  If no milestone has
passed yet the vested amount is zero.  There is no interpolation between
milestones — the vested balance steps up discretely at each milestone
timestamp.

### Example

A grant of `10_000` tokens with four quarterly unlocks:

| Timestamp (unix) | Cumulative | Interpretation            |
| ---------------- | ---------- | ------------------------- |
| 1_750_000_000    | 2_500      | 25 % after Q1             |
| 1_757_884_800    | 5_000      | 50 % after Q2             |
| 1_765_660_800    | 7_500      | 75 % after Q3             |
| 1_773_436_800    | 10_000     | 100 % after Q4 (fully vested) |

* **Before Q1** (timestamp < 1_750_000_000): vested = 0
* **At Q1** (timestamp ≥ 1_750_000_000 but < 1_757_884_800): vested = 2_500
* **At Q2** (timestamp ≥ 1_757_884_800 but < 1_765_660_800): vested = 5_000
* **At Q3** (timestamp ≥ 1_765_660_800 but < 1_773_436_800): vested = 7_500
* **At or after Q4** (timestamp ≥ 1_773_436_800): vested = 10_000

Claiming works exactly as with the linear schedule: `claimable = vested - claimed`.

## Validation Rules

When a milestone grant is created via `add_grant`, the following checks are
enforced:

1. **At least one milestone** is required.
2. **Strictly increasing timestamps** — each milestone's timestamp must be
   greater than the previous one.
3. **Strictly increasing cumulative amounts** — each milestone's cumulative
   must be greater than the previous one.
4. **Final cumulative equals principal** — the cumulative of the last
   milestone must exactly match the `principal` of the grant.
5. **No milestone cumulative exceeds principal** — each individual cumulative
   is checked against the grant's principal.

If any rule is violated `add_grant` returns an appropriate `VestingError`
variant.

## Edge Cases & Guarantees

### Before first milestone
The vested amount is `0`.  No tokens are claimable.

### Exactly at a milestone timestamp
The milestone's full cumulative amount is vested (inclusive boundary).

### Between milestones
The vested amount remains at the previous milestone's cumulative until the
next milestone is reached.

### After final milestone
The vested amount is `principal` (fully vested).  It will never exceed
`principal`.

### Single milestone
A schedule with a single milestone is valid, provided its cumulative equals
`principal`.  This models a "cliff-only" grant where all tokens unlock at one
future date.

### Arithmetic
All computations use checked `i128` arithmetic.  Overflow conditions return
a safe sentinel of `0` (which is unreachable with realistic principal values
given Soroban's 64-bit timestamp range).

## Compatibility With Linear Vesting

Existing linear grants are unaffected by the addition of the milestone
variant.  `add_grant` dispatches validation based on the schedule type, and
`vested_at` / `claimable` / `claim` / `sync` operate identically on both
schedule types.

## API Reference

| Function | Description |
| -------- | ----------- |
| `initialize(admin)` | One-time admin setup |
| `add_grant(admin, recipient, principal, schedule)` | Create a grant (Linear or Milestone) |
| `vested_at(recipient, timestamp)` | Vested amount at any timestamp |
| `claimable(recipient)` | Vested minus already claimed |
| `claim(recipient, amount)` | Claim vested tokens (recipient must auth) |
| `get_grant(recipient)` | Read stored grant |
| `sync(recipient)` | Alias for `claimable` |
| `get_admin()` | Read admin address |
