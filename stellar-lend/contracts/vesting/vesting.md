# Soroban Token Vesting Contract

A token vesting contract designed for the StellarLend protocol treasury and team allocations. Provides scheduled releases with cliff and linear increments.

## Roles
- **Admin**: Has control to create schedules, emergency pause functions, revoke unvested schedules, and transfer admin rights.
- **Beneficiary**: The user assigned a schedule. Can call `claim()` to unlock their vested tokens after the cliff.

## Core Features
1. **Cliff & Linear Release**: A user vests zero tokens until `cliff_time`, at which point they linearly vest tokens up to `end_time`.
2. **Revocability**: If configured, the admin can revoke a schedule. Total amount of tokens kept by beneficiary = currently vested tokens at revoke time. Remaining unvested amount goes back to the admin.
3. **Emergency Pause**: Admin can halt `create_schedule` and `claim` globally using `pause()`/`unpause()`.

## Security Notes
- Reentrancy is intrinsically prevented via lack of complex interactions, external calls are restricted to trusted token contracts (`transfer`).
- Arithmetic edge cases (like zero schedules or start >= end) are prevented in `create_schedule` with active limits and checked operations.
- Admin powers are significant. Two-step admin transfer (`propose_admin` / `accept_admin`) is enabled by default to prevent loss of admin control.
