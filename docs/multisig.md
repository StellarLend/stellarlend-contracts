# Multisig Module

## Overview

The **multisig** module (`src/multisig.rs`) implements a proposal–approve–execute governance pattern for critical StellarLend protocol parameters. It is a thin, focused layer on top of `governance.rs` that adds admin-set management (`ms_set_admins`) and a clean public API for the multisig flow.

---

## Flow

```
ms_set_admins([A1, A2, A3], threshold=2)
          │
A1 calls ms_propose_set_min_cr(new_ratio=20000)
          │  ← A1 auto-approves
          │
A2 calls ms_approve(proposal_id)
          │  ← threshold (2) met
          │
[wait for execution timelock — default 2 days]
          │
A3 calls ms_execute(proposal_id)
          │
Protocol parameter updated; proposal marked Executed
```

---

## Storage Layout

Shares all storage with `governance.rs` via `GovernanceDataKey`:

| Key | Type | Description |
|-----|------|-------------|
| `GovernanceDataKey::MultisigAdmins` | `Vec<Address>` | Current admin set |
| `GovernanceDataKey::MultisigThreshold` | `u32` | Approval quorum |
| `GovernanceDataKey::ProposalCounter` | `u64` | Monotonic proposal ID counter |
| `GovernanceDataKey::Proposal(id)` | `Proposal` | Proposal data |
| `GovernanceDataKey::ProposalApprovals(id)` | `Vec<Address>` | Per-proposal approvals |

---

## Functions

### `ms_set_admins(env, caller, admins, threshold)`

> **Auth:** Existing admin (or any caller at first bootstrap)

Replaces the multisig admin set and threshold atomically.

| Param | Type | Constraint |
|-------|------|-----------|
| `admins` | `Vec<Address>` | Non-empty, no duplicates |
| `threshold` | `u32` | `1 ≤ threshold ≤ len(admins)` |

**Errors:** `Unauthorized`, `InvalidMultisigConfig`

---

### `ms_propose_set_min_cr(env, proposer, new_ratio)`

> **Auth:** Registered multisig admin

Creates a `MinCollateralRatio` proposal. The proposer automatically approves.

| Param | Type | Constraint |
|-------|------|-----------|
| `new_ratio` | `i128` | > 10,000 bps (> 100%) |

**Returns:** `u64` proposal ID

**Errors:** `Unauthorized`, `InvalidProposal`

**Events:** `proposal_created(proposal_id, proposer)` + `proposal_approved(proposal_id, proposer)`

---

### `ms_approve(env, approver, proposal_id)`

> **Auth:** Registered multisig admin

Adds one approval to a proposal. Duplicate approvals rejected.

**Errors:** `Unauthorized`, `ProposalNotFound`, `AlreadyVoted`

**Events:** `proposal_approved(proposal_id, approver)`

---

### `ms_execute(env, executor, proposal_id)`

> **Auth:** Registered multisig admin

Executes the proposal after the approval threshold is met **and** the execution timelock has elapsed.

**Errors:** `Unauthorized`, `InsufficientApprovals`, `ProposalNotReady`, `ProposalAlreadyExecuted`

**Events:** `proposal_executed(proposal_id, executor)`

---

## View Functions

| Function | Returns | Description |
|----------|---------|-------------|
| `get_ms_admins(env)` | `Option<Vec<Address>>` | Current admin list |
| `get_ms_threshold(env)` | `u32` | Approval threshold (default `1`) |
| `get_ms_proposal(env, id)` | `Option<Proposal>` | Proposal by ID |
| `get_ms_approvals(env, id)` | `Option<Vec<Address>>` | Approvals for a proposal |

---

## Security Model

| Threat | Mitigation |
|--------|-----------|
| Single admin key compromise | t-of-n threshold before any parameter changes |
| Replay of executed proposals | `ProposalStatus::Executed` checked; `ProposalAlreadyExecuted` returned on second attempt |
| Old proposal ID reuse | Monotonic counter in `governance.rs` — IDs never decrease |
| Front-running a proposal | Proposer auto-approves in the same call, so no window between creation and first approval |
| Rushed execution | Execution timelock (default 2 days) gives time to detect malicious proposals |

---

## Extending with New Actions

To add a new governable parameter (e.g. `SetReserveFactor`):

1. Add a variant to `ProposalType` in `governance.rs`:
   ```rust
   SetReserveFactor(i128),
   ```
2. Add a new propose function in `multisig.rs`:
   ```rust
   pub fn ms_propose_set_reserve_factor(env: &Env, proposer: Address, factor: i128)
       -> Result<u64, GovernanceError> { ... }
   ```
3. Add execution logic inside `execute_proposal` in `governance.rs`:
   ```rust
   ProposalType::SetReserveFactor(f) => { /* persist */ }
   ```
4. Add tests in `multisig_test.rs`.
5. Expose the entrypoint in `lib.rs`.

---

## Integration — `lib.rs` changes needed

Add to `lib.rs`:

```rust
pub mod multisig;

use multisig::{ms_set_admins, ms_propose_set_min_cr, ms_approve, ms_execute};
```

Then expose on `HelloContract`:

```rust
pub fn ms_set_admins(env: Env, caller: Address, admins: Vec<Address>, threshold: u32)
    -> Result<(), GovernanceError> { multisig::ms_set_admins(&env, caller, admins, threshold) }

pub fn ms_propose_set_min_cr(env: Env, proposer: Address, new_ratio: i128)
    -> Result<u64, GovernanceError> { multisig::ms_propose_set_min_cr(&env, proposer, new_ratio) }

pub fn ms_approve(env: Env, approver: Address, proposal_id: u64)
    -> Result<(), GovernanceError> { multisig::ms_approve(&env, approver, proposal_id) }

pub fn ms_execute(env: Env, executor: Address, proposal_id: u64)
    -> Result<(), GovernanceError> { multisig::ms_execute(&env, executor, proposal_id) }
```

---

## Events Reference

All events emitted via helpers in `governance.rs`:

| Event | Topics | Payload |
|-------|--------|---------|
| `proposal_created` | `(proposal_id, proposer)` | — |
| `proposal_approved` | `(proposal_id, approver)` | — |
| `proposal_executed` | `(proposal_id, executor)` | — |
| `proposal_failed` | `(proposal_id)` | — |