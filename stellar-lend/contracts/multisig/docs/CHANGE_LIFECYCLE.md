# Multisig Governance Lifecycle

This document describes the proposal-based governance lifecycle implemented in the
Multisig contract (`stellar-lend/contracts/multisig`). All critical governance
changes — threshold updates, signer-set rotation, and arbitrary contract
invocations — flow through a create → approve → execute pipeline with domain-separated
authorization bindings and proposal expiry.

---

## 1. Overview & Threat Model

The primary security invariant of the multisig contract is to prevent a compromised
quorum or individual signer from taking over governance in a single ledger.

If an attacker temporarily controls enough signers to meet the approval threshold, they
might attempt to:

1. Lower the threshold to `1` to execute subsequent proposals unilaterally.
2. Replace the signer set with their own controlled keys.

To mitigate this, the contract enforces:

- **Multi-signature quorum**: Every proposal must receive at least `threshold` distinct
  signer approvals before it can be executed.
- **Domain-separated approval binding**: Each approval is cryptographically scoped to
  `(contract_id, proposal_id, approver)` via `require_auth_for_args`, so an authorization
  gathered for one proposal cannot satisfy quorum on a different proposal (issue #1278).
- **Proposal expiry**: Proposals expire after a TTL, preventing stale proposals from
  being executed indefinitely.
- **Signer-shrink guard**: A `RotateSigners` action whose new set is smaller than the
  current threshold is rejected, preventing permanent bricking of the multisig.
- **Cancellation**: Any signer can cancel an active proposal before it reaches quorum.

---

## 2. Proposal Lifecycle

```mermaid
stateDiagram-v-->
    [*] --> Created: create_proposal(caller, action, payload_hash, ttl_ledgers)
    note right of Created
        id = next_proposal_id()
        expires_at = current_ledger + ttl_ledgers
        status = Active
        approvals = []
    end note
    Created --> Created: approve_proposal(caller, id)
    note right of Created
        Adds caller to approvals.
        If approvals.len() >= threshold:
            status = Passed
    end note
    Created --> Executed: execute_proposal(caller, id, payload_hash)
    note right of Executed
        Requires status == Passed
        Requires payload_hash match
        Dispatches ProposalAction
        status = Executed
    end note
    Created --> Expired: current_ledger > expires_at
    Created --> Cancelled: cancel_proposal(caller, id)
    Created --> Active: revoke_approval (future)
```

### States

| State | Description |
|---|---|
| `Active` | Proposal created, awaiting approvals. |
| `Passed` | Approval count has reached the threshold. |
| `Executed` | Proposal has been executed; action dispatched. |
| `Expired` | Ledger sequence has passed `expires_at`. |
| `Cancelled` | A signer cancelled the proposal. |

---

## 3. Entrypoints

### 3.1 Initialization

```rust
pub fn initialize(
    env: Env,
    signers: Vec<Address>,
    threshold: u32,
) -> Result<(), MultisigError>
```

- **Auth**: None (deployer trusted).
- **Action**: Stores the initial signer set and threshold. Sets `ProposalCount` to 0.
- **Errors**:
  - `AlreadyInitialized` — contract already has a signer set.
  - `InvalidSigners` — `signers` is empty.
  - `InvalidThreshold` — `threshold == 0` or `threshold > signers.len()`.

### 3.2 Create Proposal

```rust
pub fn create_proposal(
    env: Env,
    caller: Address,
    action: ProposalAction,
    payload_hash: Bytes,
    ttl_ledgers: u64,
) -> Result<u64, MultisigError>
```

- **Auth**: `caller.require_auth()` — must be a registered signer.
- **Action**: Allocates a new proposal ID, computes `expires_at = current_ledger + ttl_ledgers`,
  and stores the proposal in `Active` status.
- **Errors**:
  - `Unauthorized` — caller is not in the signer set.
  - `InvalidTtl` — `ttl_ledgers > 3_110_400`.
  - `ProposalIdOverflow` — `ProposalCount` has wrapped.

### 3.3 Approve Proposal

```rust
pub fn approve_proposal(env: Env, caller: Address, id: u64) -> Result<(), MultisigError>
```

- **Auth**: Domain-separated binding via `require_auth_for_args`. The caller must
  authorize `sha256(APPROVAL_DOMAIN_SEPARATOR || contract_id || id || caller)`.
  See [APPROVAL_DOMAIN_BINDING.md](APPROVAL_DOMAIN_BINDING.md) for the full threat model.
- **Action**: Adds `caller` to the proposal's approvals. If the approval count reaches
  the threshold, transitions status to `Passed`. Persists the binding hash under
  `ApprovalBinding(id, caller)`.
- **Errors**:
  - `Unauthorized` — caller is not a registered signer.
  - `ProposalNotFound` — no proposal with this ID.
  - `ProposalExpired` — ledger has passed `expires_at`.
  - `AlreadyExecuted` — proposal already executed.
  - `AlreadyCancelled` — proposal was cancelled.
  - `ProposalNotPassed` — proposal is in an unexpected state.
  - `AlreadyApproved` — caller already approved this proposal.

### 3.4 Execute Proposal

```rust
pub fn execute_proposal(
    env: Env,
    caller: Address,
    id: u64,
    payload_hash: Bytes,
) -> Result<(), MultisigError>
```

- **Auth**: `caller.require_auth()` — must be a registered signer.
- **Action**: Validates the proposal is `Passed`, non-expired, non-executed, and that
  `payload_hash` matches the hash recorded at creation. Dispatches the `ProposalAction`
  via `dispatch_action`. Emits `ProposalExecutedEvent`.
- **Errors**:
  - `Unauthorized` — caller is not a registered signer.
  - `ProposalNotFound`, `ProposalExpired`, `AlreadyExecuted`, `AlreadyCancelled`,
    `ProposalNotPassed`, `PayloadHashMismatch` — as above.
  - Action-specific errors from `dispatch_action` (e.g., `InvalidThreshold`,
    `InvalidSigners`, `InvalidAction`).

### 3.5 Batch Execute

```rust
pub fn batch_execute(
    env: Env,
    caller: Address,
    ids: Vec<u64>,
    payload_hashes: Vec<Bytes>,
) -> Result<(), MultisigError>
```

- **Auth**: `caller.require_auth()` — must be a registered signer.
- **Action**: Validates all proposals first (status, expiry, payload hash, duplicates).
  If every proposal is eligible, executes them in order. If any proposal fails, the
  entire batch is rejected — Soroban's panic-based rollback guarantees all-or-nothing.
  Emits `BatchExecutedEvent`.
- **Errors**:
  - `BatchSizeExceeded` — `ids.len() > MAX_BATCH_SIZE` (32).
  - `PayloadHashMismatch` — count mismatch or hash mismatch.
  - `DuplicateProposalId` — same ID appears more than once.
  - Plus all `execute_proposal` errors.

### 3.6 Cancel Proposal

```rust
pub fn cancel_proposal(env: Env, caller: Address, id: u64) -> Result<(), MultisigError>
```

- **Auth**: `caller.require_auth()` — must be a registered signer.
- **Action**: Transitions an `Active` proposal to `Cancelled`.
- **Errors**:
  - `Unauthorized` — caller is not a registered signer.
  - `ProposalNotFound`, `ProposalExpired`, `AlreadyExecuted`, `AlreadyCancelled`,
    `ProposalNotPassed` — as above.

### 3.7 View Functions

| Fn | Returns | Description |
|---|---|---|
| `get_threshold(env)` | `u32` | Current approval threshold. |
| `get_signers(env)` | `Vec<Address>` | Current signer set (empty if uninitialized). |
| `get_proposal(env, id)` | `Result<Proposal, MultisigError>` | Full proposal state. |
| `get_approval_binding(env, id, approver)` | `Option<BytesN<32>>` | Stored binding hash for `(id, approver)`. |
| `verify_approval_binding(env, id, approver)` | `bool` | True iff stored binding matches recomputed hash. |
| `approval_binding_hash(env, id, approver)` | `BytesN<32>` | Precompute the binding hash for auth args. |

---

## 4. ProposalAction Variants

Actions are carried on a proposal and dispatched at execution time.

### `SetThreshold(u32)`

- **Action**: Overwrites `MultisigDataKey::Threshold`.
- **Guard**: `new_threshold == 0` → `InvalidThreshold`.

### `RotateSigners(Vec<Address>)`

- **Action**: Overwrites `MultisigDataKey::Signers`.
- **Guards**:
  - `new_signers.is_empty()` → `InvalidSigners`.
  - `new_signers.len() < threshold` → `InvalidAction` (signer-shrink bricking guard).

### `InvokeContract(Address, Symbol, Vec<Val>)`

- **Action**: Cross-contract call via `env.invoke_contract`.
- **Guard**: Payload hash still binds the approved action so it cannot be swapped.

---

## 5. Domain-Separated Approval Binding (Issue #1278)

Instead of a bare `require_auth()`, `approve_proposal` requires the caller to authorize
the domain-separated payload:

```text
sha256(
    APPROVAL_DOMAIN_SEPARATOR
    || contract_id_xdr
    || proposal_id (8-byte big-endian)
    || approver_xdr
)
```

via `require_auth_for_args`. An authorization produced for proposal `A` therefore
cannot satisfy approval of proposal `B`. The same hash is persisted under
`MultisigDataKey::ApprovalBinding(id, approver)` for off-chain verification.

- `APPROVAL_DOMAIN_SEPARATOR` = `"STELLARLEND_MULTISIG_APPROVAL_V1"`

See [APPROVAL_DOMAIN_BINDING.md](APPROVAL_DOMAIN_BINDING.md) for the full layout and
threat model.

---

## 6. Signer-Shrink Guard

Applying a `RotateSigners` action whose new set is smaller than the current threshold
would permanently brick the multisig because quorum could never be reached again. The
contract rejects such rotations with `InvalidAction` in `dispatch_action`.

To shrink the signer set below the current threshold, first reduce the threshold via a
`SetThreshold` proposal, then execute the `RotateSigners` proposal.

---

## 7. Storage Layout

| Key | Type | Description |
|---|---|---|
| `MultisigDataKey::Threshold` | `u32` | Live approval threshold. |
| `MultisigDataKey::Signers` | `Vec<Address>` | Live signer set. |
| `MultisigDataKey::ProposalCount` | `u64` | Monotonic counter for proposal IDs. |
| `MultisigDataKey::Proposal(u64)` | `Proposal` | Full proposal state by ID. |
| `MultisigDataKey::ApprovalBinding(u64, Address)` | `BytesN<32>` | Domain-separated binding hash for `(id, approver)`. |

### `Proposal` struct

| Field | Type | Description |
|---|---|---|
| `id` | `u64` | Unique proposal ID. |
| `proposer` | `Address` | Signer who created the proposal. |
| `action` | `ProposalAction` | Typed action to dispatch on execution. |
| `payload_hash` | `Bytes` | SHA-256 hash of the encoded action payload. |
| `approvals` | `Vec<Address>` | Distinct signers who approved. |
| `status` | `ProposalStatus` | `Active`, `Passed`, `Executed`, `Expired`, `Cancelled`. |
| `expires_at` | `u64` | Ledger sequence after which the proposal expires. |

---

## 8. Constants

| Name | Value | Description |
|---|---|---|
| `MAX_BATCH_SIZE` | `32` | Maximum proposals per `batch_execute` call. |
| `APPROVAL_DOMAIN_SEPARATOR` | `"STELLARLEND_MULTISIG_APPROVAL_V1"` | Domain separator for approval auth bindings. |

---

## 9. Error Reference

| Variant | Code | Description |
|---|---|---|
| `Unauthorized` | 1 | Caller is not a registered signer. |
| `ProposalNotFound` | 2 | No proposal with the given ID. |
| `ProposalNotPassed` | 3 | Proposal is not in `Passed` status. |
| `ProposalExpired` | 4 | Ledger has passed `expires_at`. |
| `AlreadyExecuted` | 5 | Proposal already executed. |
| `AlreadyApproved` | 6 | Caller already approved this proposal. |
| `PayloadHashMismatch` | 7 | Presented hash does not match recorded hash. |
| `QuorumNotReached` | 8 | Proposal is `Active` but not yet `Passed`. |
| `InvalidAction` | 9 | Action-specific guard failed (e.g., signer-shrink below threshold). |
| `InvalidThreshold` | 10 | Threshold is 0 or exceeds signer count. |
| `InvalidSigners` | 11 | Signer set is empty. |
| `AlreadyCancelled` | 12 | Proposal was already cancelled. |
| `InvalidTtl` | 13 | `ttl_ledgers` exceeds maximum. |
| `BatchSizeExceeded` | 14 | `batch_execute` called with too many IDs. |
| `DuplicateProposalId` | 15 | Same ID appears twice in a batch. |
| `AlreadyInitialized` | 16 | `initialize` called on an already-initialized contract. |
| `ProposalIdOverflow` | 17 | `ProposalCount` has wrapped. |
