# Soroban Token Vesting Contract

A token vesting contract designed for the StellarLend protocol treasury and team allocations. Provides scheduled releases with cliff and linear increments.

## Roles
- **Admin**: Has control to create schedules, emergency pause functions, revoke unvested schedules, and transfer admin rights.
- **Beneficiary**: The user assigned a schedule. Can call `claim()` to unlock their vested tokens after the cliff.

## Core Features
1. **Cliff & Linear Release**: A user vests zero tokens until `cliff_time`, at which point they linearly vest tokens up to `end_time`.
2. **Revocability**: If configured, the admin can revoke a schedule. Total amount of tokens kept by beneficiary = currently vested tokens at revoke time. Remaining unvested amount goes back to the admin.
3. **Emergency Pause**: Admin can halt `create_schedule` and `claim` globally using `pause()`/`unpause()`.
4. **Single-Grant Model**: Each grantee can hold at most one active vesting schedule at any given time. The contract does not support multi-grant aggregates (e.g. no `get_grants`, `claimable_total`, or `total_locked`); a beneficiary's address directly maps to their single schedule.

## Security Notes

### Time Assumptions
- All times are represented as **u64 UNIX timestamps** in seconds.
- Soroban ledger timestamp is authoritative (accessed via `env.ledger().timestamp()`).
- No leap-second handling; timestamps are continuous.
- Cliff and end times must satisfy: `start_time < cliff_time <= end_time` (strictly enforced in contract).

### Clock Source
- **Authority**: Soroban ledger clock is the single source of truth for vesting calculations.
- **Precision**: 1-second granularity. Vesting calculations use integer division; fractional amounts are truncated.
- **Clock Skew**: Validators maintain synchronized clocks. Vesting contract assumes accurate ledger timestamps.
- **Implications**: Treasury schedules should account for ~5-10 minute variance in actual vs. expected unlock times due to ledger network propagation.

### Admin Trust Model
- **High-Trust Model**: Admin authority is significant and includes:
  - Creating new vesting schedules (can lock large quantities of tokens).
  - Revoking schedules (can reclaim unvested tokens at any time).
  - Pausing/unpausing all operations (can halt beneficiary claims).
  - Transferring admin role (via `propose_admin` / `accept_admin` two-step process).
- **Mitigation**: Two-step admin transfer prevents accidental loss of control. New admin must explicitly `accept_admin()` to take role.
- **Receiver Guarantees**: Once a vesting schedule is created and cliff is reached:
  - Beneficiary is **guaranteed** to receive vested tokens at `claim()` time (unless admin revokes with `revocable=true`).
  - Non-revocable schedules (`revocable=false`) cannot be revoked, providing beneficiary certainty.

### Reentrancy & Atomicity
- **Reentrancy Prevention**: Vesting contract makes external calls only to trusted token contract via `TokenClient::transfer`.
- **No Callbacks**: Token transfers do not trigger callbacks that could re-enter the vesting contract.
- **Atomic Operations**: Schedule creation and claim operations are atomic (single transaction).

### Arithmetic Safety
- Uses **checked arithmetic** (`checked_mul`, `checked_add`) to prevent overflow.
- Panics on arithmetic errors (detected during contract instantiation; prevents silent failures).
- Vested token calculation uses: `(elapsed / duration) * total_amount` with integer truncation.

## Treasury Emission Schedule Example

### Scenario: 100M Treasury Tokens, 1-Year Cliff + 3-Year Linear Vesting

```rust
// Pseudocode:
let start_time = 1_700_000_000; // ~Nov 15, 2023 (UNIX timestamp)
let cliff_time = start_time + (365 * 24 * 60 * 60); // +1 year
let end_time = cliff_time + (3 * 365 * 24 * 60 * 60); // +3 years after cliff
let total_amount = 100_000_000_000_000; // 100M tokens (with decimals)

vesting_contract.create_schedule(
    beneficiary,
    total_amount,
    start_time,
    cliff_time,
    end_time,
    revocable = true
);
```

**Timeline:**
- **T=0 to T+1 year**: Beneficiary receives 0 tokens (cliff period).
- **T+1 year to T+4 years**: Beneficiary can claim linearly: `(elapsed / 3_years) * 100M`.
  - At T+1.5 years: ~16.7M available.
  - At T+2.5 years: ~50M available.
  - At T+4 years: 100M (all) available.
- **T+4 years+**: All 100M claimed.

### Receiver Guarantees
- If `revocable=false`, treasury cannot revoke schedule. Beneficiary has certainty of receipt.
- If `revocable=true`, treasury can revoke at any time, returning unvested tokens. Beneficiary keeps vested amount earned up to revocation.

## Admin Role Management

### Two-Step Admin Transfer (Prevents Accidental Loss)

**Step 1**: Current admin proposes new admin.
```rust
admin.propose_admin(new_admin_address);
```

**Step 2**: New admin must accept the role.
```rust
new_admin.accept_admin();
```

**Benefit**: Prevents typos or invalid addresses from locking out the protocol.

## Event Emission (If Enabled)

The contract can emit events for audit trails:
- `ScheduleCreated(beneficiary, total_amount, cliff_time, end_time)`
- `TokensClaimed(beneficiary, amount_claimed)`
- `ScheduleRevoked(beneficiary, vested_returned_to_beneficiary, unvested_returned_to_admin)`

*Note: Current implementation tracks state via storage; events would require Soroban event framework integration.*

## Testing

### Integration Test Coverage (>95%)

1. **Cliff Vesting Tests**:
   - No tokens claimable before cliff.
   - Claim rejection before cliff.

2. **Partial Unlock Tests**:
   - Linear vesting at 25%, 50%, 75% of vesting period.
   - Multiple claims accumulate correctly.

3. **Full Unlock Tests**:
   - All tokens available after end time.
   - Contract fully emptied.

4. **Admin Management Tests**:
   - Two-step admin transfer.
   - Non-revocable schedule protection.
   - Revocable schedule revocation (mid-vesting).

5. **Pause/Unpause Tests**:
   - Operations blocked when paused.
   - Operations resume after unpause.

6. **Multiple Beneficiary Tests**:
   - Independent schedules for multiple recipients.
   - Isolated claim operations.

7. **Realistic Treasury Tests**:
   - 100M token schedule with multi-year vesting.

## Deployment Checklist

- [ ] Verify token contract address is correct.
- [ ] Confirm cliff and end times align with treasury governance decisions.
- [ ] Test revocability flag per beneficiary category (team: revocable; strategic: non-revocable).
- [ ] Ensure admin key is securely managed (consider multi-sig).
- [ ] Set up monitoring for `claim()` calls to track beneficiary withdrawals.
- [ ] Document all schedules in off-chain treasury ledger.

## Known Limitations

1. **No Partial Revocation**: Revoke is all-or-nothing. Cannot selectively revoke portions.
2. **Immutable Timelines**: Once created, schedule times cannot be adjusted (except revocation).
3. **No Delegation**: Beneficiary cannot delegate claim rights to another account.
4. **Single Token**: One token per contract instance. Multiple tokens require multiple contracts.
