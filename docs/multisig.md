# Multisig Module

## Overview

The **multisig** module (`src/multisig.rs`) implements a proposal–approve–execute governance pattern for critical StellarLend protocol parameters. It is a thin, focused layer on top of `governance.rs` that adds admin-set management (`ms_set_admins`) and a clean public API for the multisig flow.

For a detailed reference on the two-phase change lifecycle, timelock cooldowns, and cancellation mechanics, see the [Change Lifecycle Guide](../stellar-lend/contracts/multisig/docs/CHANGE_LIFECYCLE.md).

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
| `GovernanceDataKey::Proposal(id)` | `Proposal` | Proposal data, including `expires_at_ledger: u32` |
| `GovernanceDataKey::ProposalApprovals(id)` | `Vec<Address>` | Per-proposal approvals; removed with its proposal during expiry cleanup |
| `DataKey::PendingSignersChange` | `SignersChange` | Queued replacement signer set plus `eta_ledger`; absent when idle |

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

Rotation guidance:

- Use `ms_set_admins(...)` to replace the full signer set in one governance action when rotating
  the humans or devices behind a multisig-controlled admin role.
- Avoid "remove one signer now, add the replacement later" workflows for governance signers. The
  multisig contract already supports atomic replacement, which is safer and easier to audit.
- If this multisig is the stored upgrade `admin`, finish multisig signer rotation here before
  rotating any separate upgrade approver keys in `docs/UPGRADE_AUTHORIZATION.md`.

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

Executes the proposal after the approval threshold is met **and** the execution timelock has elapsed. Execution also checks `env.ledger().sequence() <= proposal.expires_at_ledger`; stale proposals whose expiry ledger is in the past are rejected and must be recreated so approvals reflect current signer intent.

**Errors:** `Unauthorized`, `InsufficientApprovals`, `ProposalNotReady`, `ProposalAlreadyExecuted`, `ProposalExpired`

**Events:** `proposal_executed(proposal_id, executor)`

---

### `cleanup_expired(env, ids)`

> **Auth:** Multisig admin / cleanup administrator

Removes expired, unexecuted proposal records and their approval vectors from contract storage. The call is deliberately batched by explicit proposal IDs so operators can bound transaction cost and review exactly which records will be deleted. Fresh proposals and executed proposals are retained for auditability.

| Param | Type | Constraint |
|-------|------|-----------|
| `ids` | `Vec<u64>` | Proposal IDs to inspect and clean |

**Returns:** number of proposals removed.

**Errors:** `Unauthorized`, `NotInitialized`

---

## View Functions

| Function | Returns | Description |
|----------|---------|-------------|
| `get_ms_admins(env)` | `Option<Vec<Address>>` | Current admin list |
| `get_ms_threshold(env)` | `u32` | Approval threshold (default `1`) |
| `get_ms_proposal(env, id)` | `Option<Proposal>` | Proposal by ID |
| `get_ms_approvals(env, id)` | `Option<Vec<Address>>` | Approvals for a proposal |
| `get_default_expiry_ledgers(env)` | `u32` | Default proposal lifetime used by the multisig crate |

---

## Security Model

| Threat | Mitigation |
|--------|-----------|
| Single admin key compromise | t-of-n threshold before any parameter changes |
| Replay of executed proposals | `ProposalStatus::Executed` checked; `ProposalAlreadyExecuted` returned on second attempt |
| Old proposal ID reuse | Monotonic counter in `governance.rs` — IDs never decrease |
| Front-running a proposal | Proposer auto-approves in the same call, so no window between creation and first approval |
| Rushed execution | Execution timelock (default 2 days) gives time to detect malicious proposals |
| Stale approval execution | `expires_at_ledger` is stored on every proposal and `ms_execute` / `execute_proposal` rejects `current_ledger > expires_at_ledger` |
| Storage bloat from expired proposals | `cleanup_expired(ids)` removes expired unexecuted proposals and their approvals in bounded batches |
| **Signer-set instant takeover** | **`queue_signers_change` / `apply_signers_change` enforce the same ~7-day cooldown as threshold changes; `set_signers` is retained for direct admin changes** |

---

## Signer-Set Change Timelock

For detailed state diagrams and transition guides regarding signer-set rotations, see the [Change Lifecycle Guide](../stellar-lend/contracts/multisig/docs/CHANGE_LIFECYCLE.md).

### Motivation

`set_signers` can still be called directly by the admin for immediate changes when that is intentional (e.g. emergency key rotation). However, a new **queued, cooldown-gated path** — mirroring the existing `queue_threshold_change` / `apply_threshold_change` pattern — protects against an attacker who briefly controls quorum from replacing the entire signer set in a single ledger and locking out the legitimate owners.

### Cooldown Window

```
MIN_SIGNERS_DELAY_LEDGERS = MIN_THRESHOLD_DELAY_LEDGERS = 600 000 ledgers ≈ 7 days
```

Both governance levers (`threshold` and `signer set`) share the same cooldown so there is no weaker lever an attacker can exploit.

### API

```rust
/// Queue a signer-set replacement. Returns SignersDelayNotElapsed until the
/// cooldown elapses. Overwrites any previously queued change and resets the
/// cooldown window to the new queue ledger.
pub fn queue_signers_change(env: Env, new_signers: Vec<Address>) -> Result<(), MultisigError>

/// Apply the queued change once current_ledger >= eta_ledger.
pub fn apply_signers_change(env: Env) -> Result<(), MultisigError>

/// Cancel a queued change before it is applied. Emergency escape hatch.
pub fn cancel_signers_change(env: Env) -> Result<(), MultisigError>

/// Inspect the pending change (returns None when no change is queued).
pub fn get_pending_signers_change(env: Env) -> Option<SignersChange>

/// The cooldown constant (equals MIN_THRESHOLD_DELAY_LEDGERS).
pub fn get_min_signers_delay_ledgers(env: Env) -> u32
```

### Storage

| Key | Type | Description |
|-----|------|-------------|
| `DataKey::PendingSignersChange` | `SignersChange` | Queued replacement signer set plus `eta_ledger` |

```rust
pub struct SignersChange {
    pub new_signers: Vec<Address>,
    pub eta_ledger: u32,        // queue_ledger + MIN_SIGNERS_DELAY_LEDGERS
}
```

### Events

| Event | Fields | Emitted by |
|-------|--------|-----------|
| `SignersChangeQueuedEvent` | `admin`, `eta_ledger` | `queue_signers_change` |
| `SignersChangeAppliedEvent` | `admin`, `ledger` | `apply_signers_change` |
| `SignersChangeCancelledEvent` | `admin`, `ledger` | `cancel_signers_change` |

### Error Variants

| Code | Variant | Meaning |
|------|---------|---------|
| 1015 | `NoQueuedSignersChange` | `apply_signers_change` or `cancel_signers_change` called with nothing queued |
| 1016 | `SignersDelayNotElapsed` | `apply_signers_change` called before `eta_ledger` |

### Worked Example

**Scenario A — Legitimate signer rotation (7-day review)**

```
Ledger L:           Admin queues a 3-signer replacement set.
                    → SignersChangeQueuedEvent emitted; eta_ledger = L + 600 000
                    → Live signer set UNCHANGED
                    → get_pending_signers_change() returns the queued set

Ledger L + 300 000: Community reviews the proposed new signers.
                    → apply_signers_change() → SignersDelayNotElapsed

Ledger L + 600 000: Delay elapsed.
                    → apply_signers_change() succeeds
                    → Live signer set updated to the queued set
                    → SignersChangeAppliedEvent emitted
                    → get_pending_signers_change() returns None
```

**Formula:** `eta_ledger = queue_ledger + 600 000`  
**Application window:** `current_ledger >= eta_ledger` (no upper bound)

**Scenario B — Malicious quorum attempts a takeover**

```
Ledger L:       Attacker briefly controls quorum; queues a replacement signer
                set with their own addresses.
                → eta_ledger = L + 600 000; live set is still the original

Ledger L + 1:   Legitimate admin detects the queued change via
                get_pending_signers_change().
                → Calls cancel_signers_change()
                → Pending change removed; SignersChangeCancelledEvent emitted
                → Attacker's replacement set is never applied
```

### Monitoring

Add the following event watchers alongside the existing threshold-change monitors:

```javascript
contract.events.filter({ topics: ["multisig", "SignersChangeQueuedEvent"] })
  .on('data', (event) => {
    console.log('⚠ Signer-set change queued', {
      admin:      event.admin,
      eta_ledger: event.eta_ledger,
      eta_time:   new Date(event.eta_ledger * 5 * 1000).toISOString(),
    });
    // Alert governance participants; begin 7-day review period.
  });

contract.events.filter({ topics: ["multisig", "SignersChangeAppliedEvent"] })
  .on('data', (event) => {
    console.log('Signer-set change applied', { admin: event.admin, ledger: event.ledger });
    // Update UI; notify signers of new set.
  });

contract.events.filter({ topics: ["multisig", "SignersChangeCancelledEvent"] })
  .on('data', (event) => {
    console.log('Signer-set change cancelled', { admin: event.admin, ledger: event.ledger });
  });
```

### Interaction with `set_signers`

`set_signers` is **not** removed. It remains available for cases where the admin intentionally wants an immediate change (e.g. emergency key compromise response). The queued path is an additional security option, not a replacement.

| Path | Delay | Use case |
|------|-------|---------|
| `set_signers` | None (immediate) | Emergency key rotation, initial bootstrap |
| `queue_signers_change` → `apply_signers_change` | ~7 days | Routine signer rotation with community review |

---

## Quorum-Counting Rules

The multisig contract enforces strict quorum-integrity guarantees at execution time to ensure that only current, valid approvals contribute to meeting the threshold:

- **Deduplication:** A signer can only approve a proposal once; subsequent approvals from the same signer are rejected with `AlreadyApproved`. At execution time, the contract deduplicates the approval list so that each signer address counts at most once.
- **Live Signer Set Evaluation:** Only addresses currently present in the registered signer set contribute to the quorum. If a signer approved a proposal but is subsequently removed from the signer set before the proposal is executed, their approval is excluded and no longer counts.
- **Live Threshold Evaluation:** The quorum threshold is read fresh from storage at the time of execution, not at the time of proposal creation. If the threshold is raised during the proposal's lifecycle, the proposal requires additional approvals to meet the new threshold before it can be executed.
- **Fallback Signer:** When no signer set is configured, the administrator address is treated as the sole implicit signer (requiring a threshold of 1).

These rules are fully covered and verified by the tests in `quorum_edge_test.rs`.

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

---

## Safe Threshold and Signer-Set Change Workflow

Changing the multisig threshold or signer set is a high-risk operation. An
incorrect sequence can create a window where protocol actions are executable
with weaker security than intended, or leave governance permanently deadlocked.

### Recommended Sequences

#### Raising security (adding signers or increasing threshold)

Always use `ms_set_admins` to atomically replace both the signer list and the
threshold in a single call. This eliminates any intermediate state.

```
# Safe: atomic replace — new threshold applies to the new set immediately
ms_set_admins([A1, A2, A3], threshold=2)
```

If you must use two steps, raise the threshold **before** adding the new signer:

```
# Step 1: raise threshold while signer count is still the same
ms_set_admins([A1, A2], threshold=2)   # was threshold=1

# Step 2: add A3 — threshold is already at the desired level
ms_set_admins([A1, A2, A3], threshold=2)
```

#### Lowering security (removing signers or decreasing threshold)

Lowering the threshold or removing a signer should be done with extra caution.
Prefer the atomic form:

```
# Safe: atomic replace
ms_set_admins([A1, A2], threshold=2)   # removes A3, keeps threshold
```

If you must lower the threshold separately, do it **after** removing the signer:

```
# Step 1: remove A3 first (threshold stays at 2-of-2, still valid)
ms_set_admins([A1, A2], threshold=2)

# Step 2: lower threshold only if intentional
set_ms_threshold(threshold=1)
```

Never lower the threshold before removing a signer — this creates a window
where fewer approvals than intended can execute proposals.

#### Replacing the entire signer set

Use a single `ms_set_admins` call. The old set is replaced atomically; there
is no window where the old threshold applies to the new set or vice versa.

```
ms_set_admins([NewA1, NewA2, NewA3], threshold=2)
```

---

## Security Notes: Preventing Downgrade Attacks

### Threshold is captured at proposal creation time

When a proposal is created via `ms_propose_set_min_cr` (or any propose
function), the **current multisig threshold is stored on the proposal** in the
`multisig_threshold` field. This stored value is the binding quorum for that
proposal — it cannot be retroactively changed.

This prevents the following attack:

1. Attacker creates a proposal when threshold = 3 (needs 3 approvals).
2. Attacker lowers threshold to 1.
3. Attacker tries to execute with only 1 approval.

Step 3 fails because `ms_execute` checks `proposal.multisig_threshold` (= 3),
not the current global threshold (= 1).

### Constraints enforced on every threshold/signer change

| Constraint | Enforced by | Error |
|---|---|---|
| Threshold ≥ 1 | `ms_set_admins`, `set_ms_threshold` | `InvalidMultisigConfig` / `InvalidThreshold` |
| Threshold ≤ signer count | `ms_set_admins`, `set_ms_threshold` | `InvalidMultisigConfig` / `InvalidThreshold` |
| No duplicate signers | `ms_set_admins` | `InvalidMultisigConfig` |
| Non-empty signer set | `ms_set_admins` | `InvalidMultisigConfig` |
| Caller must be existing admin | `ms_set_admins` (post-bootstrap), `set_ms_threshold` | `Unauthorized` |

### Execution timelock

`ms_execute` enforces a 24-hour delay from proposal creation before any
proposal can be executed, regardless of how many approvals it has. This gives
the remaining admins time to detect and respond to a malicious proposal before
it takes effect.

### Expiry window

Each proposal stores an explicit `expires_at_ledger: u32`. Proposal creation must set this value far enough in the future to cover the execution timelock, review period, and expected operational delay. The multisig crate default is 14 days of ledgers.

Execution is valid through the exact expiry ledger (`current_ledger == expires_at_ledger`) and rejected once the ledger sequence advances past it (`current_ledger > expires_at_ledger`). An expired proposal cannot be executed; a new proposal must be created and approved so the quorum reflects current intent and current protocol state.

### Operational cleanup cadence

Run `cleanup_expired(ids)` as part of normal governance operations, ideally after each governance meeting and at least weekly for active deployments. Suggested procedure:

1. Enumerate pending proposal IDs from governance monitoring.
2. Filter to proposals where `env.ledger().sequence() > expires_at_ledger` and `status != Executed`.
3. Submit bounded cleanup batches sized to fit Soroban transaction limits.
4. Archive proposal metadata off-chain before cleanup if long-term audit records are required.

Cleanup is not a substitute for the execution guard. Even if cleanup is delayed, expired proposals still cannot execute because execution checks `expires_at_ledger` on-chain.

### Bootstrap vs. post-bootstrap

`ms_set_admins` accepts any caller during the initial bootstrap (when no
multisig config exists yet). After bootstrap, only an existing admin can call
it. Ensure the bootstrap call is made in the same transaction as contract
initialization to avoid a front-running window.

---

## Failure Recovery

### Scenario: threshold accidentally set too high (governance deadlocked)

If the threshold is set higher than the number of available signers (e.g. a
key is lost), governance is deadlocked. Recovery options:

1. **Social recovery** — if guardians are configured, use `start_recovery` /
   `approve_recovery` / `execute_recovery` to rotate the admin key, then
   reconfigure the multisig.
2. **Key recovery** — recover the lost signing key from secure backup.

Prevention: always keep at least one more signer than the threshold (n-of-m
where m > n) so a single key loss does not deadlock governance.

### Scenario: malicious proposal approved before detection

If a malicious proposal reaches the approval threshold:

1. The 24-hour execution timelock gives a response window.
2. Any existing admin can call `cancel_proposal` before execution.
3. After cancellation, rotate the compromised key via `ms_set_admins`.

### Scenario: signer key compromised

1. Immediately call `ms_set_admins` with the compromised key removed and a
   replacement key added, keeping the threshold the same or higher.
2. Review all pending proposals for approvals from the compromised key.
3. Cancel any proposals that were approved by the compromised key if their
   legitimacy is in doubt.


## Proposal Lifecycle Test Coverage

The `stellarlend-multisig` crate ships a `#[cfg(test)]` module covering every error variant and timing boundary in the proposal flow. Run with:

```bash
cargo test -p stellarlend-multisig
```

### Key scenarios

| Test | Error variant asserted |
|------|----------------------|
| `test_execute_proposal_double_execution` | `ProposalAlreadyExecuted` |
| `test_execute_proposal_not_found` | `ProposalNotFound` |
| `test_execute_proposal_before_eta_rejected` | `ProposalNotReady` |
| `test_execute_at_exact_eta_boundary` | `ProposalNotReady` (boundary −1); success (boundary) |
| `test_execute_expired_proposal_rejected` | `ProposalExpired` |
| `test_execute_at_expiry_boundary` | success at boundary; `ProposalExpired` at boundary +1 |
| `test_apply_threshold_change_before_delay` | `DelayNotElapsed` |
| `test_apply_at_exact_min_delay_boundary` | `DelayNotElapsed` (boundary −1); success (boundary) |
| `test_queue_threshold_change_zero_threshold` | `InvalidThreshold` |
| `test_create_proposal_invalid_threshold` | `InvalidThreshold` |
| `test_apply_threshold_change_no_queued_change` | `NoQueuedChange` |
| `test_initialize_already_initialized` | `AlreadyInitialized` |
| `test_queue_threshold_change_unauthorized` | host-level abort (`#[should_panic]`) |
| `test_apply_threshold_change_unauthorized` | host-level abort (`#[should_panic]`) |

### Signer-Set Cooldown Test Coverage (`signer_cooldown_test.rs`)

| Test | Invariant verified |
|------|--------------------|
| `test_queue_and_apply_signers_change_success` | Happy-path queue → apply updates live signer set |
| `test_apply_before_delay_rejected` | `SignersDelayNotElapsed` before cooldown elapses |
| `test_apply_at_exact_eta_boundary` | `SignersDelayNotElapsed` at eta−1; success at eta |
| `test_apply_after_eta_succeeds` | Apply at 2× cooldown succeeds (no upper bound) |
| `test_cancel_clears_pending_change` | `cancel_signers_change` removes pending; subsequent apply → `NoQueuedSignersChange` |
| `test_cancel_nothing_queued_returns_error` | `NoQueuedSignersChange` when idle |
| `test_overwrite_pending_change_resets_cooldown` | Second queue resets eta; first eta no longer sufficient |
| `test_queue_signers_change_unauthorized` | `require_auth` aborts (`#[should_panic]`) |
| `test_apply_signers_change_unauthorized` | `require_auth` aborts (`#[should_panic]`) |
| `test_cancel_signers_change_unauthorized` | `require_auth` aborts (`#[should_panic]`) |
| `test_queue_empty_signers_rejected` | `InvalidThreshold` on empty signer list |
| `test_get_pending_signers_change_reflects_queue` | `eta_ledger = queue_ledger + MIN_SIGNERS_DELAY_LEDGERS` |
| `test_get_min_signers_delay_ledgers_returns_constant` | Returns `MIN_SIGNERS_DELAY_LEDGERS` |
| `test_same_ledger_queue_and_apply_rejected` | Same-ledger protection (`SignersDelayNotElapsed`) |
| `test_set_signers_still_immediate` | `set_signers` remains instant; queued path is additive |
| `test_apply_clears_pending_change` | `get_pending_signers_change` → `None` after apply |
| `test_apply_with_nothing_queued_returns_error` | `NoQueuedSignersChange` |
| `test_multi_signer_set_replacement_preserved` | 5-signer set preserved exactly through queue → apply |
| `test_entrypoints_return_not_initialized` | All 3 new entrypoints → `NotInitialized` before init |
| `test_signers_delay_equals_threshold_delay` | `MIN_SIGNERS_DELAY_LEDGERS == MIN_THRESHOLD_DELAY_LEDGERS` |
| `test_queued_change_does_not_mutate_live_set` | Live set unchanged while change is only queued |
| `test_queue_cancel_requeue_apply` | Cancel + re-queue + apply with different set succeeds |
| `test_single_signer_set_accepted` | Single-address signer set is valid |
| `test_apply_at_exact_min_delay_boundary` | Boundary −1 → `SignersDelayNotElapsed`; boundary → success |

### Timing boundary invariants

- `DelayNotElapsed` until `current_ledger >= eta_ledger` (where `eta_ledger = queue_ledger + 600 000`).
- `ProposalNotReady` until `current_ledger >= proposal.eta_ledger`.
- `ProposalExpired` when `current_ledger > proposal.expires_at_ledger`; the boundary ledger itself is still valid.
- `ProposalAlreadyExecuted` on any second call to `execute_proposal` for an `executed = true` proposal — no amount of ledger advancement can re-enable execution.

### Snapshot Testing

## Overview

Soroban tests generate deterministic JSON snapshots in test_snapshots/ directories. These snapshots capture the ledger state at the end of each test and must be committed and kept in sync with the contract code. Drift between committed snapshots and freshly generated output indicates an unintended change in contract behavior.

## How Snapshots Work
The Soroban Rust SDK automatically writes snapshots to test_snapshots/<test_name>.<n>.json when tests use the Env. These files contain:
Ledger entries: Contract data, token balances, persistent storage
Contract events: Emitted events with topics and data
Authorization contexts: Auth trees and signatures
Budget/resource usage: CPU and memory metrics

Running Snapshot Checks Locally
# Check for drift (fails if snapshots differ from fresh runs)
SNAPSHOT_CHECK=1 ./scripts/check-snapshots.sh

# Warning only (does not fail)
SNAPSHOT_CHECK=0 ./scripts/check-snapshots.sh

Regenerating Snapshots (Intentional Changes)

When you modify contract logic that legitimately changes the snapshot output:
# 1. Regenerate all snapshots for both crates
./scripts/regenerate-snapshots.sh

# 2. Review the diff carefully
# Ensure every change is expected and explained by your code changes
git diff stellar-lend/contracts/*/test_snapshots/

# 3. Commit the updated snapshots
git add stellar-lend/contracts/*/test_snapshots/
git commit -m "chore: regenerate test snapshots for <describe change>"

## CI Failure Recovery
1. If CI fails with SNAPSHOT DRIFT DETECTED:
Check if drift is intentional: Did your PR change contract logic?
Yes: Run ./scripts/regenerate-snapshots.sh, review every diff line, commit.
No: Your change introduced unintended behavior. Fix the code, do not regenerate.
2. Never blindly regenerate: Always review the diff to ensure changes match your intent.
3. Common causes of unintended drift:
Changed Env setup in tests (timestamps, ledgers)
Modified contract state transitions
Updated SDK version changing snapshot format
Non-deterministic test data (random values, unmocked time)


## Snapshot File Format
test_snapshots/
└── test/
    └── test_name.1.json          # First snapshot assertion in test_name
    └── test_name.2.json          # Second snapshot assertion (if multiple)
    └── test_other.1.json
    Each .json file contains a complete ledger state dump. The filename pattern is:

    test_name — The Rust test function name
.1, .2 — Snapshot index (incremented per env assertion in the test)


## Troubleshooting
| Symptom                            | Cause                            | Fix                                                       |
| ---------------------------------- | -------------------------------- | --------------------------------------------------------- |
| `diff: No such file or directory`  | Missing `test_snapshots/` dir    | Run tests locally first to generate baseline, then commit |
| All snapshots show as "new"        | `.gitignore` excluding snapshots | Remove `test_snapshots/` from `.gitignore`                |
| Non-deterministic diffs            | Random data in tests             | Use fixed seeds, mock `Env` timestamps, avoid `rand`      |
| Large diffs across all tests       | SDK version change               | Regenerate once, commit alongside SDK upgrade PR          |
| CI passes locally but fails remote | Different Rust version           | Pin `rust-toolchain.toml` to exact version                |



