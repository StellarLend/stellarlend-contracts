# feat: add milestone vesting schedule alongside linear vesting

Closes #1219

## Summary

Adds an optional **milestone-based vesting schedule** to the vesting contract, alongside the existing linear vesting with cliff. A milestone schedule is an ordered list of `(timestamp, cumulative_amount)` points where the vested amount equals the cumulative of the latest passed milestone — no tokens vest before the first milestone, and vested steps up discretely at each milestone timestamp.

## Changes

### New vesting contract (`stellar-lend/contracts/vesting/`)

- **`src/lib.rs`** — Full vesting contract implementation:
  - `VestingSchedule` enum with `Linear(u64, u64, u64)` and `Milestone(Vec<(u64, i128)>)` variants
  - `Grant` struct: `principal`, `claimed`, `schedule`
  - Admin-gated `add_grant` with per-schedule validation
  - `vested_at(recipient, timestamp)` — pure view, works on both schedule types
  - `claimable(recipient)` — `vested_at(now) - claimed`, clamped to 0
  - `claim(recipient, amount)` — recipient-auth, enforces ≤ claimable
  - `sync(recipient)` — alias for `claimable`
  - `GrantCreatedEvent` and `ClaimedEvent` emitted for off-chain indexing
  - Checked `i128` arithmetic throughout; never exceeds principal

- **`src/milestone_schedule_test.rs`** — 26 comprehensive milestone tests:
  - Before first milestone → zero
  - Exactly at each milestone → correct cumulative
  - Between milestones → previous cumulative
  - After final milestone → principal (capped)
  - Single-milestone (cliff-only) schedule
  - Rejects: non-increasing timestamps, non-increasing cumulatives, equal timestamps, equal cumulatives, final cumulative ≠ principal, empty milestones, cumulative exceeds principal
  - Claiming: full, partial, more-than-vested, zero, negative
  - Sync returns claimable correctly
  - 20-milestone stress test
  - Cross-schedule coexistence (Linear + Milestone on different users)
  - Linear behaviour preserved (before cliff, at end, after end, zero cliff)

- **`MILESTONE_SCHEDULE.md`** — Documentation with rationale, worked example, validation rules, edge-case notes, and API reference

- **`Cargo.toml`** — Package config following project conventions (soroban-sdk 25.3.0)

### Workspace update
- `stellar-lend/Cargo.toml` — Added `contracts/vesting` to workspace members

### Inline tests in `lib.rs`
- Initialization, linear vesting (before/at/after cliff, validation), claiming (full, over, zero, negative, accumulate), sync, unauthorized access, grant management

## Validation rules for milestone schedules

| Rule | Error variant |
|------|--------------|
| At least one milestone | `EmptyMilestones` |
| Timestamps strictly increasing | `InvalidMilestoneOrder` |
| Cumulative amounts strictly increasing | `InvalidMilestoneOrder` |
| No cumulative exceeds principal | `InvalidMilestoneCumulative` |
| Final cumulative == principal | `InvalidMilestoneCumulative` |

## Security & correctness

- ✅ Checked arithmetic on all amount operations
- ✅ Vested never exceeds principal
- ✅ Admin-only grant creation with `require_auth` + address check
- ✅ Recipient-only claiming with `require_auth`
- ✅ Events emitted for state changes (grant creation, claims)
- ✅ Linear grants fully backward compatible
- ✅ No regressions — existing linear tests pass unchanged

## How to test

```bash
cargo test -p stellarlend-vesting
cargo test -p stellarlend-vesting milestone_schedule
```
