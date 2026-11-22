# Batch Execute (`batch_execute`)

## Rationale

The multisig contract's single-`execute_proposal` entrypoint processes proposals
one at a time. When several parameter changes must land together — for example,
raising the approval threshold while simultaneously rotating the signer set —
executing them individually creates a window where the system is in an
intermediate state:

1. `SetThreshold(3)` executes → threshold is 3 but signers are still `[A, B, C]`.
2. `RotateSigners([D, E, F])` executes → signers are now `[D, E, F]`.

If an attacker or accidental front-runner injected a proposal between steps 1
and 2, the coordinated intent would be broken. `batch_execute` eliminates this
window by executing all proposals in a single atomic operation.

## Interface

```rust
pub fn batch_execute(
    env: Env,
    caller: Address,
    ids: Vec<u64>,
    payload_hashes: Vec<soroban_sdk::Bytes>,
);
```

- **`caller`** — Must be a registered signer and pass `require_auth`.
- **`ids`** — Ordered list of proposal IDs to execute.
- **`payload_hashes`** — One hash per ID; each must match the hash stored at
  proposal creation time (prevents action-swap attacks).

## Semantics

### Two-phase execution

1. **Validation phase** — Every proposal is checked for eligibility without
   modifying any execution state:
   - Proposal exists (`ProposalNotFound`).
   - Status is `Passed` (not `Active`, `Executed`, `Cancelled`, or `Expired`).
   - Not expired (`ProposalExpired`).
   - Payload hash matches (`PayloadHashMismatch`).
   - No duplicate IDs in the batch (`DuplicateProposalId`).
   - Batch size does not exceed `MAX_BATCH_SIZE` (`BatchSizeExceeded`).
   - `payload_hashes.len() == ids.len()` (`PayloadHashMismatch`).

2. **Execution phase** — Each validated proposal is dispatched in order.
   If any `dispatch_action` returns `false` (e.g., `SetThreshold(0)` or
   `RotateSigners([])`), the contract panics. Because Soroban reverts all
   storage writes on a panic, any side-effects from earlier proposals in the
   batch are rolled back — guaranteeing **all-or-nothing** semantics.

### Event

On success a single `BatchExecutedEvent` is emitted:

```rust
pub struct BatchExecutedEvent {
    pub ids: Vec<u64>,
}
```

Topic: `("multisig", "batch_executed")`

## Worked example

Given a multisig with signers `[A, B, C]` and threshold `2`:

1. **Create proposals**:
   - `P1`: `SetThreshold(3)`, hash `0xaaa`, expires in 1000 ledgers.
   - `P2`: `RotateSigners([D, E])`, hash `0xbbb`, expires in 1000 ledgers.

2. **Approve**:
   - A and B approve P1 → status `Passed`.
   - A and B approve P2 → status `Passed`.

3. **Batch execute**:
   ```rust
   batch_execute(caller: A, ids: [P1, P2], payload_hashes: [0xaaa, 0xbbb])
   ```

4. **Result**:
   - P1 executes → threshold becomes 3.
   - P2 executes → signers become `[D, E]`.
   - `BatchExecutedEvent { ids: [P1, P2] }` is emitted.
   - Both proposals are marked `Executed`.

No other transaction can observe threshold=3 with signers `[A, B, C]` because
the entire sequence is atomic.

## Edge cases

| Scenario | Behaviour |
|---|---|
| **One ineligible proposal** | The entire batch panics and no proposal is executed (validation-phase failure). |
| **All eligible** | All proposals execute and `BatchExecutedEvent` is emitted. |
| **Duplicate ID** | Panics with `DuplicateProposalId`; nothing in the batch executes. |
| **Empty batch** | Succeeds (no-op); `BatchExecutedEvent { ids: [] }` is emitted. |
| **Batch over `MAX_BATCH_SIZE`** | Panics with `BatchSizeExceeded` before any storage reads. |
| **Dispatch failure (e.g. threshold=0)** | Panics with `InvalidAction` during the execution phase; Soroban rolls back earlier proposals' side-effects. |
| **Payload hash mismatch** | Panics with `PayloadHashMismatch`; nothing executes. |
| **Proposal already executed** | Panics with `AlreadyExecuted`; nothing executes. |
| **Expired proposal** | The expired proposal is marked `Expired` in storage and a panic is raised; nothing else executes. |
| **Non-signer caller** | Panics with `Unauthorized` (caller auth + signer check). |

## `MAX_BATCH_SIZE`

Defined as `32`. This bound prevents unbounded loop iterations and storage
churn in a single contract invocation, keeping gas costs predictable.
