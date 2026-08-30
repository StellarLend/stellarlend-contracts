# Milestone Vesting Status

## Current Status

Milestone-based vesting is **not implemented** in the compiled vesting
contract at `stellar-lend/contracts/vesting/src/lib.rs`.

The contract currently supports only a **single linear vesting schedule with an
optional cliff** per grantee. This file exists to document that limitation and
to prevent readers from relying on an API that does not exist on `main`.

## What Is Actually Supported

Each stored grant contains:

- `total_amount`
- `claimed_amount`
- `start_ts`
- `cliff_secs`
- `duration_secs`
- `revoked`

Vesting is computed linearly:

- nothing vests before `start_ts + cliff_secs`
- vesting then accrues linearly from `start_ts`
- the full `total_amount` is vested once `duration_secs` has elapsed

Claimable balance is:

```text
claimable = vested_at(effective_now) - claimed_amount
```

`effective_now` is the ledger timestamp adjusted by the contract's pause
accounting so that paused time does not increase vesting.

## Public Contract API On Main

The vesting contract currently exposes the following entry points:

| Function | Description |
| -------- | ----------- |
| `initialize(env, admin)` | One-time admin setup |
| `create_grant(env, caller, grantee, total_amount, start_ts, cliff_secs, duration_secs)` | Create a linear grant |
| `pause(env, caller)` | Pause claims and revocations |
| `resume(env, caller)` | Resume the contract and accumulate paused time |
| `claim(env, grantee)` | Claim the full currently vested amount |
| `revoke(env, caller, grantee)` | Revoke a grant and claw back unvested tokens |
| `get_grant(env, grantee)` | Read the stored linear grant |
| `total_paused_secs(env)` | Read total paused seconds |
| `is_paused(env)` | Read current pause status |

## What Is Not Implemented

The following milestone-related items described by earlier drafts are **not**
present in the compiled contract:

- no `MilestoneSchedule` type
- no `VestingSchedule` enum with a milestone variant
- no `add_grant(admin, recipient, principal, schedule)` entry point
- no `vested_at(recipient, timestamp)` public contract method
- no `claimable(recipient)` public contract method
- no `sync(recipient)` entry point

There is also no milestone storage model for ordered
`(timestamp, cumulative_amount)` pairs.

## Test Status

`src/milestone_schedule_test.rs` documents a possible milestone-vesting design,
but it is not wired into the compiled contract from `lib.rs` and should not be
treated as implemented behavior on `main`.

## Future Work

If milestone vesting is desired in the future, it will require a separate
contract change that adds:

- a schedule type that can represent discrete tranches
- validation rules for milestone ordering and cumulative totals
- storage and view methods for milestone-based grants
- tests wired into the compiled crate

Until then, the vesting contract should be documented and used as a
linear-with-cliff vesting contract only.
