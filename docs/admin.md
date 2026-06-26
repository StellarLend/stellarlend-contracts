# Admin and Access Control

StellarLend's lending contract enforces strict access control for all
privileged operations. This document describes the initialisation boundary,
the `require_admin` helper, and the two-step admin rotation pattern.

---

## Initialisation boundary

```
initialize(env, admin)  →  Result<(), LendingError>
```

`initialize` may be called **exactly once**.

- On the first call it stores `admin` under `DataKey::Admin` and sets the
  emergency state to `Normal`.
- On any subsequent call it returns `LendingError::AlreadyInitialized`
  immediately, before touching any state.

**Why this matters**: without this guard, anyone who can submit a transaction
after deployment could call `initialize` again with their own address and
seize admin rights over the protocol.

`initialize` does **not** call `require_auth` on the supplied `admin` address.
This matches the conventional Soroban contract pattern where the deployer is
trusted at construction time.

---

## `require_admin` helper

```rust
fn require_admin(env: &Env) -> Result<Address, LendingError>
```

This private helper is the single authoritative auth check for all privileged
operations:

1. Load `DataKey::Admin` from instance storage.
   - If missing → `Err(LendingError::NotInitialized)`.
2. Call `admin.require_auth()`.
   - Soroban will surface an auth failure if the transaction was not signed by
     the admin.
3. Return `Ok(admin)` so callers can use the address if needed.

Every privileged setter **must** call `require_admin` as its first statement,
before reading or writing any protocol state.

---

## Privileged entrypoints

| Entrypoint | Auth requirement |
|---|---|
| `set_min_borrow` | Admin only (`require_admin`) |
| `set_debt_ceiling` | Admin only (`require_admin`) |
| `set_flash_fee` | Admin only (`require_admin`) |
| `set_guardian` | Admin only (`require_admin`) |
| `propose_admin` | Admin only (`require_admin`) |
| `accept_admin` | Pending admin (explicit `require_auth`) |
| `set_emergency_state` | Admin **or** guardian (`require_auth` on guardian) |

---

## Super Admin

The protocol has a single super-admin whose address is stored under
`DataKey::Admin`. The admin has clearance for all privileged operations listed
above.

`get_admin()` returns `Result<Address, LendingError>` — a `NotInitialized`
error signals that `initialize` has not been called. Callers should use
`try_get_admin()` if the contract may be uninitialized.

---

## Two-step admin rotation

Admin rotation is a two-step handover to prevent accidental transfers to an
uncontrolled address:

1. **Propose**: current admin calls `propose_admin(new_admin)`.
   - Stores `new_admin` under `DataKey::PendingAdmin`.
   - Guarded by `require_admin`, so only the current admin can nominate a
     successor.
2. **Accept**: `new_admin` calls `accept_admin()`.
   - `new_admin.require_auth()` is called — the successor must sign the
     acceptance.
   - Clears `PendingAdmin` and overwrites `Admin` with `new_admin`.

---

## Guardian role

The guardian is an optional address that is permitted to call
`set_emergency_state` without requiring the admin key. This allows an
emergency operator to pause the protocol quickly without exposing the admin
private key in a hot path.

- Set with `set_guardian(guardian)` (admin only).
- If no guardian is set, the admin address is used as the fallback.

---

## Auth boundary summary

```
initialize          ── no auth (deployer trusted)
─── already-initialized guard prevents re-init ───────────────────────────
propose_admin       ── require_admin()
accept_admin        ── pending_admin.require_auth()
set_min_borrow      ── require_admin()
set_debt_ceiling    ── require_admin()
set_flash_fee       ── require_admin()
set_guardian        ── require_admin()
set_emergency_state ── guardian.require_auth()  (guardian defaults to admin)
```

All other entrypoints (`deposit`, `withdraw`, `borrow`, `repay`, `liquidate`)
require auth from the **user** performing the operation, not the admin.
