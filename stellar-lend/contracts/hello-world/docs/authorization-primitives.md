# Authorization Primitives — `hello-world` Contract

> **Scope**: This document describes the **real** authorization surface of
> `stellar-lend/contracts/hello-world/`. Always consult the source files listed
> below rather than this document when there is any doubt.

---

## Overview

The `hello-world` contract uses a **single-admin plus guardian** model. There is
no general-purpose RBAC system, no `grant_role` / `revoke_role` / `require_role_or_admin`
primitive, and no `gov_can_vote` function exported as a public authorization
helper. Each module that needs privileged access either calls `admin::require_admin`
or performs its own inline guardian check.

---

## Primary Primitives

### `admin::require_admin(env, caller)` — `src/admin.rs`

The canonical, shared admin check. **Every module that needs admin authorization
should call this function** rather than re-implementing the admin-lookup logic.

```rust
/// Require `caller` to be the stored protocol admin.
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), AdminError> {
    caller.require_auth();                    // Soroban cryptographic check
    match get_admin(env) {
        Some(admin) if admin == *caller => Ok(()),
        Some(_)                         => Err(AdminError::Unauthorized),
        None                            => Err(AdminError::NotInitialized),
    }
}
```

**Properties**:
- Calls `require_auth()` first (cryptographic, non-repudiable).
- Compares against the address stored under `AdminDataKey::Admin` in instance
  storage — the single source of truth for who the admin is.
- Returns `AdminError::NotInitialized` if no admin has been set, enabling clean
  error surfaces instead of panics.

### `caller.require_auth()` — Soroban built-in

All privileged entry points call `caller.require_auth()` (directly or via
`require_admin`). This is Stellar's cryptographic signature verification — it
verifies that the transaction was signed by the private key corresponding to
`caller`.

---

## Admin Lifecycle

```
set_admin(env, new_admin)           ← bootstrap only (no existing admin)
propose_admin(env, caller, new)     ← two-step handover proposal
accept_admin(env, caller)           ← new admin accepts
```

`propose_admin` / `accept_admin` implement a two-step handover to prevent
accidental lockout. Neither step accepts the contract's own address as a target
(`AdminError::CannotTransferToSelf`).

Source: `src/admin.rs`

---

## Guardian Model

Guardians are stored as a list of `Address` values plus a threshold integer in
`GuardianConfig` (instance storage key `GovernanceDataKey::GuardianConfig`).
They are managed by the protocol admin via `governance.rs`:

| Function | Who can call | Effect |
|---|---|---|
| `add_guardian(caller, guardian)` | Admin | Appends guardian to list (idempotent) |
| `remove_guardian(caller, guardian)` | Admin | Removes guardian from list |
| `set_guardian_threshold(caller, threshold)` | Admin | Sets required approval count |

Guardian checks (e.g., `start_recovery`, `approve_recovery`) load `GuardianConfig`
from instance storage and verify membership inline — there is no exported
`require_guardian` helper function.

Source: `src/governance.rs`

---

## Governance Voting (`gov_can_vote`)

`gov_can_vote(env, voter, proposal_id) -> bool` is implemented in
`src/governance.rs`. It is a **query function on the governance module**, not a
reusable authorization primitive. Modules should not call it to gate
state-mutating operations; they should perform their own membership check against
`GovernanceConfig.voters` or `GuardianConfig.guardians` instead.

`gov_can_vote` returns `true` when all of:
1. Governance is initialized (`GovernanceDataKey::Config` exists).
2. The proposal exists and is active (not executed, cancelled, or expired).
3. The voter is the admin, a configured voter, or a guardian.

Source: `src/governance.rs`, tested in `src/gov_can_vote_test.rs`.

---

## Authorization Patterns by Module

| Module | Admin gating | Guardian gating | Notes |
|--------|-------------|-----------------|-------|
| `admin.rs` | `require_admin` | — | Canonical admin check lives here |
| `governance.rs` | inline `caller != config.admin` check | inline guardian list membership | Does not call `admin::require_admin` — uses its own `GovernanceConfig.admin` |
| `governance.rs` — guardian mgmt | `caller != config.admin` | — | `add_guardian`, `remove_guardian`, `set_guardian_threshold` |
| `withdraw.rs` | `require_admin` (for admin paths) | — | User withdrawals use `caller.require_auth()` only |
| `repay.rs` | `require_admin` (for admin paths) | — | User repays use `caller.require_auth()` only |
| `risk_management.rs` | `require_admin` | — | |
| `oracle.rs` | `require_admin` | — | |
| `bridge.rs` | `require_admin` + guardian freeze | Guardian freeze check | |
| `flash_loan.rs` | `require_admin` (fee config) | — | Flash loan execution uses `caller.require_auth()` |

> **Known inconsistency**: Several modules re-implement their own inline admin
> check (`if caller != stored_admin { return Err(Unauthorized) }`) instead of
> calling `admin::require_admin`. This is a tracked maintenance issue — new
> modules should always delegate to `admin::require_admin`.

---

## What Does NOT Exist

The following names are **not present** anywhere in `stellar-lend/contracts/hello-world/src/`:

- `require_role_or_admin` — no RBAC helper exists
- `grant_role` / `revoke_role` — no role table exists
- `has_role` — no role table exists
- Any generic "role" concept beyond admin and guardian

If you are writing a new module and are tempted to add role-based logic, discuss
the design in an issue first. For now, use `admin::require_admin` for admin
operations and the inline guardian membership pattern for guardian operations.

---

## Threat Model Notes

| Threat | Mitigation |
|--------|-----------|
| Unauthorized admin call | `require_auth()` + address comparison in `require_admin` |
| Accidental admin lockout | Two-step `propose_admin` / `accept_admin` handover |
| Compromised admin key | Guardian-based recovery via `start_recovery` / `approve_recovery` / `execute_recovery` (requires threshold guardians) |
| Rogue guardian | Admin can remove guardians; threshold > 1 limits single-guardian power |
| Threshold set to 0 or above guardian count | **Currently unguarded** — tracked in issue #1756; `set_guardian_threshold` accepts any `u32` without validation |

---

## Source Files

- `src/admin.rs` — `require_admin`, admin handover, `AdminError`
- `src/governance.rs` — `GovernanceError`, proposals, voting, guardian management,
  `gov_can_vote`
- `src/storage.rs` — `GuardianConfig` type definition
