# Vesting Contract — Admin Pause / Resume

This document describes the emergency pause mechanism added to the StellarLend
vesting contract, its operational semantics, and how it aligns with the
protocol-wide pause posture.

---

## Motivation

Every other StellarLend contract exposes an admin-gated pause switch for
incident response. The vesting contract previously had no such control: if a
grant was misconfigured or treasury funds needed to be frozen during an incident,
the admin had no way to halt `claim` or `revoke` without upgrading the contract.

This change adds a minimal, stateful pause flag that blocks settlement while
leaving vesting accrual untouched.

---

## Behaviour

### What is blocked while paused

| Operation  | Paused? | Note                                   |
|------------|---------|----------------------------------------|
| `claim`    | **Yes** | Returns `ContractPaused` immediately.  |
| `revoke`   | **Yes** | Returns `ContractPaused` immediately.  |
| `add_grant`| No      | Scheduling new grants is always allowed. |

### What is NOT affected

- **Vesting math.** Tokens continue to vest linearly according to each grant's
  schedule. The pause does not alter `start_seconds`, `duration_seconds`,
  `cliff_seconds`, or `released`. Time spent paused is not "lost" — accrued
  tokens are fully claimable once the pause is lifted.
- **State inspection.** `get_grants`, `total_locked`, `balance_of`, and
  `is_paused` are read-only and are never gated.

### Error ordering for `revoke`

Auth is checked before the pause gate:

1. If the caller is not the admin → `Unauthorized` (regardless of pause state).
2. If the contract is paused → `ContractPaused`.

This prevents non-admin callers from learning whether the contract is paused.

---

## API

### `pause(caller: &str) -> Result<(), VestingError>`

Sets the internal pause flag to `true`.

- **Authorization** — `caller` must equal `self.admin`; otherwise returns
  `VestingError::Unauthorized`.
- **Idempotent** — calling `pause` while already paused succeeds without error.
- **No math impact** — vesting schedules continue to accrue during the pause.

### `resume(caller: &str) -> Result<(), VestingError>`

Clears the internal pause flag, re-enabling `claim` and `revoke`.

- **Authorization** — `caller` must equal `self.admin`; otherwise returns
  `VestingError::Unauthorized`.
- **Idempotent** — calling `resume` while not paused succeeds without error.

### `is_paused() -> bool`

Returns `true` if the contract is currently paused. No authorization required.

Frontends and integrators should query this before presenting claim or revoke
actions to users so they can surface a clear "paused" message instead of a
failed transaction.

---

## Error Variants

| Variant          | When returned                                              |
|------------------|------------------------------------------------------------|
| `Unauthorized`   | Caller is not the admin (`pause`, `resume`, `revoke`).     |
| `ContractPaused` | `claim` or `revoke` called while `paused == true`.         |
| `NoSuchGrant`    | `revoke` called for a grantee with no recorded schedules.  |
| `AlreadyRevoked` | All of the grantee's schedules are already revoked.        |

---

## Operational Guide

### Incident response: pause

```text
1. Detect misconfigured grant, treasury issue, or unexpected contract behaviour.
2. Call  pause("admin")  to immediately halt all settlement.
3. Investigate. Vesting math continues to accrue; no tokens are lost.
4. Apply any necessary fixes (e.g. correct a grant off-chain record,
   coordinate a treasury action).
5. Call  resume("admin")  to re-enable claim and revoke.
```

### Alignment with the broader protocol

The lending contract's pause uses a TTL-based `PauseState` with auto-expiry and
per-operation granularity (`PauseType::All`, `Deposit`, `Borrow`, etc.).

The vesting contract is a simpler, non-Soroban crate with no ledger concept, so
the pause is a plain boolean. The intent and admin-only authorization model are
identical: only the configured admin may set or clear the flag, and settlement is
blocked while it is active.

---

## Security Assumptions

1. **Admin trust.** The admin should be a multisig or DAO-governed address.
   A compromised admin key can indefinitely pause the vesting contract.

2. **No bypass.** Both `claim` and `revoke` call `check_not_paused` before any
   state mutation. There is no settlement path that skips this check.

3. **Atomicity.** If `claim` or `revoke` returns `ContractPaused`, no state has
   been mutated — `total_locked`, grant fields, and all balances are unchanged.

4. **Accrual continuity.** The pause flag is never read by `vested_at`, `sync`,
   or `add_grant`. Vesting schedules advance through time independently of
   settlement availability.

---

## Example Usage (Rust)

```rust
let mut contract = VestingContract::new("multisig", "treasury");
contract.add_grant("alice", 1_000_000, 0, 86_400, 3_600);

// Incident detected — freeze settlement immediately.
contract.pause("multisig").expect("pause");
assert!(contract.is_paused());

// Attempt to claim during the pause.
assert_eq!(
    contract.claim("alice", 7_200),
    Err(VestingError::ContractPaused),
);

// Incident resolved — re-enable settlement.
contract.resume("multisig").expect("resume");
assert!(!contract.is_paused());

// Alice can now claim the tokens that accrued during the pause.
let claimed = contract.claim("alice", 7_200).expect("claim after resume");
```
