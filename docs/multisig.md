# Multisig Module (`stellarlend-multisig`)

## Overview

The **stellarlend-multisig** crate (`stellar-lend/contracts/multisig/`) implements a
proposal–approve–execute governance pattern for critical StellarLend protocol actions.
It is a standalone Soroban smart contract (`MultisigContract`) that enforces an
m-of-n signature threshold before any governed action takes effect.

> **Scope note:** This document describes the authoritative `stellarlend-multisig` crate found at
> `stellar-lend/contracts/multisig/src/lib.rs`. The now-deleted `hello-world` contract previously
> contained a one-line placeholder stub; the canonical multisig implementation is this crate.

---

## Proposal Flow

```
initialize([A1, A2, A3], threshold=2)
         │
A1 calls create_proposal(action, payload_hash, ttl_ledgers)
         │  ← returns proposal_id
         │
A2 calls approve_proposal(id)
         │  ← threshold (2) met; status → Passed
         │
A1 (or any signer) calls execute_proposal(id, payload_hash)
         │  ← payload_hash verified; action dispatched
         │
ProposalExecutedEvent emitted; status → Executed
```

---

## Contract Entrypoints

### `initialize(env, signers, threshold)`

Initialises the multisig with an initial signer set and approval threshold.
Must be called exactly once before any other entrypoint.

| Parameter   | Type           | Constraint                                |
|-------------|----------------|-------------------------------------------|
| `signers`   | `Vec<Address>` | Non-empty list of authorised signers      |
| `threshold` | `u32`          | `1 ≤ threshold ≤ len(signers)`            |

Panics with `"InvalidThreshold"` if the constraint is violated.

---

### `create_proposal(env, caller, action, payload_hash, ttl_ledgers) → u64`

Creates a new proposal carrying a typed `ProposalAction`.

> **Auth:** `caller` must be a registered signer (`caller.require_auth()` is enforced).

| Parameter      | Type              | Description                                                        |
|----------------|-------------------|--------------------------------------------------------------------|
| `caller`       | `Address`         | Signer proposing the action                                        |
| `action`       | `ProposalAction`  | The typed action to attach to this proposal                        |
| `payload_hash` | `Bytes`           | SHA-256/Keccak hash of the encoded action payload                  |
| `ttl_ledgers`  | `u64`             | Ledgers from now until the proposal expires                        |

**Returns:** the new `u64` proposal ID.

The `expires_at` field is set to `current_ledger + ttl_ledgers`. Callers must
choose a TTL long enough to cover the expected approval and execution time.

---

### `approve_proposal(env, caller, id)`

Adds a signer's approval to an active proposal. When the number of distinct
approvals meets or exceeds the current threshold the proposal status is
automatically advanced to `Passed`.

> **Auth:** `caller` must be a registered signer **and** must authorize the
> domain-separated approval payload
> `sha256(DOMAIN_SEPARATOR || contract_id || proposal_id || signer_set_hash || approver)` via
> `require_auth_for_args`. This binds the approval to exactly one proposal so
> it cannot be replayed across ids. See
> [`stellar-lend/contracts/multisig/APPROVAL_DOMAIN_BINDING.md`](../stellar-lend/contracts/multisig/APPROVAL_DOMAIN_BINDING.md).

| Parameter | Type      | Description                     |
|-----------|-----------|---------------------------------|
| `caller`  | `Address` | Signer casting the approval     |
| `id`      | `u64`     | ID of the proposal to approve   |

**Panics:**
- `"Unauthorized"` — caller is not a registered signer, or the domain-bound auth does not match this proposal
- `"ProposalExpired"` — current ledger has passed `expires_at`
- `"ProposalNotPassed"` — proposal is not in `Active` status
- `"AlreadyApproved"` — `caller` has already approved this proposal

---

### `execute_proposal(env, caller, id, payload_hash)`

Executes a `Passed`, non-expired, non-executed proposal.

> **Auth:** `caller` must be a registered signer.

| Parameter      | Type      | Description                                                              |
|----------------|-----------|--------------------------------------------------------------------------|
| `caller`       | `Address` | Signer triggering execution                                              |
| `id`           | `u64`     | ID of the proposal to execute                                            |
| `payload_hash` | `Bytes`   | Hash of the action payload; must match the hash recorded at creation     |

The `payload_hash` check binds the approved action so it cannot be swapped
between the approval and execution steps.

**Panics:**
- `"ProposalExpired"` — current ledger has passed `expires_at`
- `"AlreadyExecuted"` — proposal was already executed
- `"AlreadyCancelled"` — proposal was cancelled
- `"ProposalNotPassed"` — proposal has not yet reached the approval threshold
- `"PayloadHashMismatch"` — supplied hash does not match the stored hash

On success the proposal status is set to `Executed` and a
`ProposalExecutedEvent` is emitted (see [Events](#events) below).

#### Action dispatch

`execute_proposal` routes the attached `ProposalAction` to its on-chain
handler via the internal `dispatch_action` function:

| Action variant       | Effect                                                                           |
|----------------------|----------------------------------------------------------------------------------|
| `SetThreshold`       | Updates the approval threshold in persistent storage                             |
| `RotateSigners`      | Replaces the full signer set in persistent storage                               |
| `InvokeContract`     | Performs a cross-contract call to the specified `contract` / `fn_symbol`         |

`dispatch_action` returns `false` (and the event records `ok: false`) when:
- `SetThreshold { new_threshold: 0 }` — threshold of zero is invalid
- `RotateSigners { new_signers: [] }` — empty signer list is invalid

---

### `cancel_proposal(env, caller, id)`

Cancels an active proposal before it is executed.

> **Auth:** `caller` must be a registered signer.

| Parameter | Type      | Description                    |
|-----------|-----------|--------------------------------|
| `caller`  | `Address` | Signer requesting cancellation |
| `id`      | `u64`     | ID of the proposal to cancel   |

**Panics:** `"ProposalNotPassed"` if the proposal is not in `Active` status
(i.e. it has already been passed, executed, expired, or cancelled).

---

## Types

### `ProposalAction`

```rust
pub struct InvokeContractParams {
    pub contract: Address,
    pub fn_symbol: Symbol,
    pub args_hash: Bytes,
}

pub enum ProposalAction {
    SetThreshold(u32),
    RotateSigners(Vec<Address>),
    InvokeContract(InvokeContractParams),
}
```

### `ProposalStatus`

```rust
pub enum ProposalStatus {
    Active,     // Accepting approvals
    Passed,     // Threshold met; awaiting execution
    Executed,   // Dispatched successfully
    Expired,    // expires_at passed before execution
    Cancelled,  // Cancelled by a signer
}
```

### `Proposal`

```rust
pub struct Proposal {
    pub id:           u64,
    pub proposer:     Address,
    pub action:       ProposalAction,
    pub payload_hash: Bytes,           // Bound at creation; verified at execution
    pub approvals:    Vec<Address>,    // Distinct signer addresses that approved
    pub status:       ProposalStatus,
    pub expires_at:   u64,             // Ledger sequence number at expiry
}
```

### `MultisigError`

| Variant               | Meaning                                                  |
|-----------------------|----------------------------------------------------------|
| `Unauthorized`        | Caller is not a registered signer                        |
| `ProposalNotFound`    | No proposal exists for the given ID                      |
| `ProposalNotPassed`   | Proposal is not in the expected status                   |
| `ProposalExpired`     | Current ledger has advanced past `expires_at`            |
| `AlreadyExecuted`     | Proposal has already been executed                       |
| `AlreadyApproved`     | Signer has already cast an approval for this proposal    |
| `PayloadHashMismatch` | Supplied payload hash does not match the stored hash     |
| `QuorumNotReached`    | Approval count is below the threshold (unused directly)  |
| `InvalidAction`       | Action variant is unrecognised or internally invalid     |
| `InvalidThreshold`    | Threshold is 0 or exceeds the signer count               |
| `InvalidSigners`      | Signer list is empty or otherwise invalid                |
| `AlreadyCancelled`    | Proposal has already been cancelled                      |

---

## Storage Layout

All state is stored in Soroban **persistent** storage under `MultisigDataKey`:

| Key                       | Type           | Description                              |
|---------------------------|----------------|------------------------------------------|
| `MultisigDataKey::Threshold`        | `u32`          | Current approval threshold               |
| `MultisigDataKey::Signers`          | `Vec<Address>` | Current registered signer set            |
| `MultisigDataKey::ProposalCount`    | `u64`          | Monotonically increasing proposal ID counter |
| `MultisigDataKey::Proposal(id)`     | `Proposal`     | Full proposal data including approvals and status |

Proposal records are **never deleted** by the contract; they remain in
storage after execution or cancellation for auditability. There is no
`cleanup_expired` entrypoint.

---

## Events

The contract emits exactly **one** event type, published by `execute_proposal`:

| Topics                              | Payload                    |
|-------------------------------------|----------------------------|
| `("multisig", "executed")`          | `ProposalExecutedEvent`    |

```rust
pub struct ProposalExecutedEvent {
    pub id:          u64,
    pub action_kind: Symbol,  // "SetThreshold" | "RotateSigners" | "InvokeContract"
    pub ok:          bool,    // true = dispatch succeeded; false = dispatch returned an error
}
```

No events are emitted for proposal creation, approval, or cancellation.

---

## Test-Only View Helpers

The following functions live inside `#[cfg(test)] mod tests` and are
**not available on-chain**. They are provided for test convenience only:

| Function                   | Returns        | Description                     |
|----------------------------|----------------|---------------------------------|
| `get_threshold(env)`       | `u32`          | Current approval threshold      |
| `get_signers(env)`         | `Vec<Address>` | Current signer list              |
| `get_proposal(env, id)`    | `Proposal`     | Proposal state by ID             |

---

## Security Model

| Threat                             | Mitigation                                                                                   |
|------------------------------------|----------------------------------------------------------------------------------------------|
| Single signer key compromise       | m-of-n threshold; one compromised key cannot execute proposals alone                         |
| Replay of executed proposals       | `ProposalStatus::Executed` checked; `"AlreadyExecuted"` returned on any second attempt       |
| Action swap between approval and execution | `payload_hash` bound at creation and re-verified at execution                        |
| Signer-set rotation replay         | Signer-set hash captured per proposal and included in approval authorization              |
| Execution retry / partial dispatch | Monotonic nonce marker is consumed only after successful dispatch in the same transaction |
| Old proposal ID reuse              | Monotonic `ProposalCount` counter — IDs never repeat                                         |
| Stale proposal execution           | `expires_at` stored on every proposal; both `approve_proposal` and `execute_proposal` enforce it |
| Rushed execution                   | Caller controls `ttl_ledgers`; integrators should set a TTL that enforces a review period    |
| Signer-set instant takeover        | `RotateSigners` is a governed `ProposalAction` requiring threshold approvals                 |

---

## Test Coverage

The `stellarlend-multisig` crate ships several `#[cfg(test)]` modules:

| Module                      | Coverage area                                    |
|-----------------------------|--------------------------------------------------|
| `tests` (in `lib.rs`)       | Core lifecycle: initialize, create, approve, execute, cancel |
| `quorum_edge_test`          | Quorum boundary conditions and deduplication     |
| `signer_cooldown_test`      | Signer-set rotation edge cases                   |
| `action_allowlist_test`     | `ProposalAction` variant dispatch correctness    |
| `upgrade_e2e_test`          | End-to-end upgrade via `InvokeContract`          |
| `execution_router_test`     | `dispatch_action` routing for all action variants |

Run the full suite with:

```bash
cargo test -p stellarlend-multisig
```

---

## Extending with New Actions

To govern a new protocol parameter, add a variant to `ProposalAction` and
handle it in `dispatch_action`:

1. Add a variant to `ProposalAction` in `lib.rs`:
   ```rust
   SetReserveFactor { new_factor: i128 },
   ```
2. Handle it in `dispatch_action`:
   ```rust
   ProposalAction::SetReserveFactor { new_factor } => {
       env.storage().persistent().set(&DataKey::ReserveFactor, new_factor);
       true
   }
   ```
3. Update `action_kind_symbol` to return the correct `Symbol` for the new variant.
4. Add tests covering the new action in the relevant test module.

---

## Failure Recovery

### Governance deadlock (threshold too high)

If the threshold is set higher than the number of available signers (e.g. a
key is lost), no further proposals can be executed. Recovery options:

1. **Key recovery** — recover the lost signing key from secure backup.
2. **Social recovery** — if the broader lending protocol has a guardian
   mechanism, use it to rotate the multisig contract's admin key, then
   re-initialise with a corrected threshold via a `SetThreshold` proposal.

Prevention: keep at least one more signer than the threshold (n-of-m where
m > n) so a single key loss does not deadlock governance.

### Malicious proposal approved before detection

If a malicious proposal reaches the approval threshold before it is detected:

1. Any signer can call `cancel_proposal` while the proposal status is still
   `Active`. Note that once status transitions to `Passed` the proposal can
   no longer be cancelled via `cancel_proposal` (it only accepts `Active`
   proposals) — execute the blocking action (e.g. a `SetThreshold` proposal
   raising the threshold) before the attacker executes the malicious one, or
   rotate signers via a competing `RotateSigners` proposal.
2. After resolution, rotate the compromised key by submitting and executing a
   `RotateSigners` proposal with the replacement signer set.
