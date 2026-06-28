# Grant Transfer Feature

## Rationale

There is no way to reassign an existing vesting Grant (e.g., a custody-address change). This feature adds an admin-gated `transfer_grant` moving a grant's remaining balance from one grantee key to another, preserving the original schedule (start, cliff, duration, claimed).

## Overview

The `transfer_grant` function allows an admin to transfer vesting grants from one grantee address to another. This is useful for:

- Custody address changes
- Recovery scenarios
- Restructuring vesting schedules
- Merging multiple grants for a single grantee

## Interface

### Function Signature

```rust
call transfer_grant(
    caller: Address,
    from: Address,
    to: Address,
    now: u64
): Result<(), TransferError>
```

### Arguments

- `caller`: The admin address that must authenticate this operation.
- `from`: The current grantee address whose grant will be transferred.
- `to`: The new grantee address that will receive the grant.
- `now`: The current Unix timestamp, used to sync vesting schedules.

### Returns

- `Ok(())` on successful transfer
- `Err(TransferError)` on failure with detailed error message

### Errors

- `Unauthorized`: Caller is not the contract admin.
- `ContractPaused`: Contract is paused; transfers disabled.
- `NoSuchGrant`: No schedules exist for the `from` address.
- `DestinationAlreadyHasGrant`: Destination already has active schedules.

## Behavior

### Step 1: Authorization

Admin-only with auth required.

### Step 2: Pause Check

Reject if the contract is paused; vesting math continues unaffected, only settlement is halted.

### Step 3: Source Validation

Reject if the source grant does not exist.

### Step 4: Destination Validation

Reject if the destination already holds a grant.

### Step 5: Schedule Synchronization

Sync both `from` and `to` grantees' schedules to `now`:

- Advance vested amounts based on current timestamp
- Update `released` fields to reflect accurate vesting state
- Preserve claimed amounts for both grants

### Step 6: Grant Movement

Move all grant entries from `from` to `to`:

- Remove source entry from internal `grants` map
- Add all grants to destination entry
- Preserve schedule fields: `total`, `claimed`, `released`, `start_seconds`, `duration_seconds`, `cliff_seconds`, `revoked`

### Step 7: State Updates

- Update `total_locked` by removing the total amount being transferred
- Extend TTL on the new grantee key via `extend_grant_ttl`

### Step 8: Event Emission

Emit a `GrantTransferred` event containing:

- `from`: Original grantee address
- `to`: New grantee address
- `amount`: Total transferred amount
- `timestamp`: Transfer timestamp

## Example Usage

```rust
// Setup a vesting grant for Alice
let grant = Grant::new(
    grantee: Address("alice"),
    total: 1000,
    start_seconds: 1000,
    duration_seconds: 1000,
    cliff_seconds: 100,
)

// At time=1500, Alice has claimed 333 tokens (33% vested)
client.transfer_grant(admin, alice, bob, 1500)?;

// Bob's grant now mirrors Alice's original schedule
let bob_grants = client.get_grants(bob);
assert_eq!(bob_grants[0].total, 1000);
assert_eq!(bob_grants[0].claimed, 333); // Claims preserved
assert_eq!(bob_grants[0].released, 667); // Released matches vesting at time=1500
```

## Edge Cases

### Destination Already Has Grants

```rust
// Fail with DestinationAlreadyHasGrant
try {
    client.transfer_grant(admin, alice, bob, now)
} catch (DestinationAlreadyHasGrant) {
    // Handle error
}
```

### Source Grant Does Not Exist

```rust
// Fail with NoSuchGrant
try {
    client.transfer_grant(admin, nonexistent, bob, now)
} catch (NoSuchGrant) {
    // Handle error
}
```

### During Pause

```rust
// Fail with ContractPaused
contract.pause(admin);
try {
    client.transfer_grant(admin, alice, bob, now)
} catch (ContractPaused) {
    // Contract is paused; transfers disabled
}
```

### Non-Admin Attempt

```rust
// Fail with Unauthorized
try {
    client.transfer_grant(user, alice, bob, now)
} catch (Unauthorized) {
    // Only admin can transfer grants
}
```

## Implementation Notes

### Sync Behavior

The sync step ensures that `released` fields reflect vesting up to `now` before the transfer, preserving the exact claimed amount and vesting schedule.

- Both source and destination are synced to maintain consistency
- This prevents loss or duplication of vested amounts
- Schedule fields remain untouched

### Conservation of Total Locked

The `total_locked` field is updated to reflect the transfer by removing the transferred amount from the source and adding it back for the destination (implicitly through the merge operation).

### TTL Management

The `extend_grant_ttl` function is called for the new grantee to ensure sufficient storage persistence for the transferred grant(s).

## Testing

Comprehensive test coverage ensures the feature handles all scenarios:

1. **Authorization Tests**
   - Non-admin cannot transfer
   - Admin can transfer

2. **State Validation Tests**
   - Transfer fails when source doesn't exist
   - Transfer fails when destination has grants
   - Transfer fails when contract is paused

3. **Schedule Preservation Tests**
   - All schedule fields preserved (total, claimed, released, start, duration, cliff, revoked)
   - Multiple grants per grantee handled correctly
   - Syncing maintains accurate vesting state

4. **Balance Tests**
   - `total_locked` value remains consistent
   - Grant balances transfer correctly
   - No tokens lost or duplicated

## Benefits

- Enables custody changes without disrupting vesting schedules
- Facilitates recovery scenarios for misplaced keys
- Allows restructuring of vesting arrangements
- Maintains complete schedule fidelity during transfer
- Secure and auditable with detailed event emission

## Security Considerations

- Admin-only access prevents unauthorized transfers
- Pause gate provides emergency shutdown capability
- Source/destination validation prevents state corruption
- Schedule preservation ensures no mathematical loss
- TTL management prevents storage exhaustion

## Related Documentation

- [`VESTING_MATH.md`](./VESTING_MATH.md) - Vesting schedule mathematics
- [`README.md`](./README.md) - General contract interface
- [`PAUSE.md`](./PAUSE.md) - Pause contract behavior
