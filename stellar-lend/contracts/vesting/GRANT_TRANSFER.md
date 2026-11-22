# Grant Transfer Feature

## Rationale

There is no way to reassign an existing vesting Grant (e.g., a custody-address change). This feature adds an admin-gated `transfer_grant` that moves a grant from one grantee key to another, preserving the original schedule (`start_ts`, `cliff_secs`, `duration_secs`, `claimed_amount`).

## Overview

The `transfer_grant` function allows an admin to transfer a vesting grant from one grantee address to another. This is useful for:

- Custody address changes
- Recovery scenarios
- Restructuring vesting schedules

## Interface

### Function Signature

```rust
pub fn transfer_grant(
    env: Env,
    caller: Address,
    from: Address,
    to: Address,
) -> Result<(), VestingError>
```

### Arguments

- `caller`: The admin address that must authorize this operation.
- `from`: The current grantee address whose grant will be transferred.
- `to`: The new grantee address that will receive the grant.

### Returns

- `Ok(())` on successful transfer
- `Err(VestingError)` on failure

### Errors

- `Unauthorized`: Caller is not the contract admin.
- `ContractPaused`: Contract is paused; transfers disabled.
- `GrantNotFound`: No grant exists for the `from` address.
- `AlreadyRevoked`: Source grant has already been revoked.
- `InvalidGrant`: `from` and `to` are the same address.
- `DestinationAlreadyHasGrant`: Destination already has a grant entry.

## Behavior

### Step 1: Authorization

Admin-only.

### Step 2: Pause Check

Reject if the contract is paused; vesting math continues unaffected, only settlement is halted.

### Step 3: Source Validation

Reject if the source grant does not exist or has been revoked.

### Step 4: Destination Validation

Reject if the destination already holds a grant (including a revoked entry still stored under that key).

### Step 5: Grant Movement

Move the grant from `from` to `to`:

- Remove the source entry under `VestingKey::Grant(from)`
- Write the grant under `VestingKey::Grant(to)` with `grantee` updated to `to`
- Preserve schedule fields: `total_amount`, `claimed_amount`, `start_ts`, `cliff_secs`, `duration_secs`, `revoked`

Vesting accrual is computed on read via `vested_at` / pause-adjusted `effective_now`, so no separate `released` sync step is required.

### Step 6: Event Emission

Emit a `grant_transferred` event with the new grantee as the actor.

## Example Usage

```rust
// Setup a vesting grant for Alice: 1000 tokens over 1000s, no cliff
client.create_grant(&admin, &alice, &1000, &start, &0, &1000);

// After 500s Alice claims half
env.ledger().with_mut(|li| li.timestamp += 500);
assert_eq!(client.claim(&alice), 500);

client.transfer_grant(&admin, &alice, &bob);

// Bob's grant mirrors Alice's schedule with claims preserved
let bob_grant = client.get_grant(&bob).unwrap();
assert_eq!(bob_grant.total_amount, 1000);
assert_eq!(bob_grant.claimed_amount, 500);
assert_eq!(bob_grant.start_ts, start);
assert_eq!(bob_grant.cliff_secs, 0);
assert_eq!(bob_grant.duration_secs, 1000);
assert!(client.get_grant(&alice).is_none());
```

## Edge Cases

### Destination Already Has Grant

Fails with `DestinationAlreadyHasGrant`. Source grant is left unchanged.

### Source Grant Does Not Exist

Fails with `GrantNotFound`.

### During Pause

Fails with `ContractPaused`.

### Non-Admin Attempt

Fails with `Unauthorized`.

### Same Address

Fails with `InvalidGrant` when `from == to`.

## Testing

Coverage in `src/grant_transfer_test.rs`:

1. **Authorization** — non-admin rejected; admin succeeds
2. **State validation** — missing source, occupied destination, paused contract, revoked source, same address
3. **Schedule preservation** — `start_ts` / `cliff_secs` / `duration_secs` / `claimed_amount` preserved
4. **Post-transfer claims** — new grantee can claim remaining vested tokens

## Related Documentation

- [`VESTING_MATH.md`](./VESTING_MATH.md) - Vesting schedule mathematics
- [`README.md`](./README.md) - General contract interface
- [`PAUSE.md`](./PAUSE.md) - Pause contract behavior
