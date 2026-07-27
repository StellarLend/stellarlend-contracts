# Emergency Shutdown & Guardian Role

## Overview

The StellarLend lending contract has a two-tier emergency authority model:

| Role      | Can trigger Shutdown | Can set Recovery | Can set Normal |
|-----------|---------------------|-----------------|----------------|
| Guardian  | ✅ Yes              | ❌ No           | ❌ No          |
| Admin     | ✅ Yes              | ✅ Yes          | ✅ Yes         |

This separation allows a fast-response operator (the guardian) to halt the protocol immediately without being granted general admin power. Only the admin can lift an emergency and resume normal operations.

## Emergency States

| State      | Deposit | Borrow | Repay | Withdraw |
|------------|---------|--------|-------|----------|
| `Normal`   | ✅      | ✅     | ✅    | ✅       |
| `Shutdown` | ❌      | ❌     | ❌    | ❌       |
| `Recovery` | ❌      | ❌     | ✅    | ✅       |

- **Normal** — full protocol operation.
- **Shutdown** — all user-facing actions are blocked. Used for immediate halts (e.g., exploit detected).
- **Recovery** — new positions cannot be opened; users may only repay debt and withdraw collateral.

## Contract Functions

### `set_guardian(guardian: Address)`
- **Auth**: admin only.
- Sets the guardian address. Replaces any previously configured guardian.
- There is no revocation function; call `set_guardian` with a trusted address to rotate.

### `get_guardian() -> Option<Address>`
- Returns the current guardian address, or `None` if unset.

### `set_emergency_state(new_state: EmergencyState)`
- **Auth**:
  - `Shutdown` → guardian **or** admin.
  - `Recovery` → admin only.
  - `Normal` → admin only.
- Emits `EmergencyStateChangedEvent { old_state, new_state }` on every transition.
- If no guardian is configured, only the admin may call this function for any state.

## Security Notes

1. **Principle of least privilege** — The guardian address should be a hot key or automated monitor. It cannot transfer funds, modify parameters, or lift the halt. Its blast radius is limited to triggering a shutdown.

2. **Admin key hygiene** — The admin key is the only path back to Normal. It should be kept in cold storage (hardware wallet or multisig) and never exposed to automated systems.

3. **No guardian ≠ no emergency stop** — When no guardian is configured, the admin retains the ability to call `set_emergency_state(Shutdown)`. Setting a guardian is optional but recommended for production deployments.

4. **Guardian rotation** — To rotate the guardian (e.g., after a key compromise), the admin calls `set_guardian(new_guardian)`. The old address loses its capability immediately.

5. **Event monitoring** — Every state change emits `EmergencyStateChangedEvent`. Off-chain monitors should subscribe to this event to detect unexpected transitions.

## Deployment Checklist

- [ ] Call `set_guardian` with a dedicated monitoring/response address after deployment.
- [ ] Store the admin key in cold storage or a multisig.
- [ ] Configure an event monitor for `EmergencyStateChangedEvent`.
- [ ] Document the guardian rotation procedure in your runbook.
- [ ] Test the shutdown path on testnet before mainnet deployment.
