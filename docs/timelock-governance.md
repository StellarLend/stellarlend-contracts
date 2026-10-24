# Timelock Governance Operations Guide

## Overview

This document describes the timelocked WASM upgrade governance built into the
lending contract (`stellar-lend/contracts/lending/src/upgrade.rs`). All
governance for WASM upgrades goes through a three-step propose → approve →
execute flow enforced entirely inside the lending contract itself — there is
no separate "Timelock" contract.

For the multisig two-phase threshold-change lifecycle (state diagrams, ETA
formulas, event schema, and cancellation rules) see the
[Multisig Change Lifecycle Guide](../stellar-lend/contracts/multisig/docs/CHANGE_LIFECYCLE.md).

## Architecture

The lending contract admin calls `upgrade_propose` to submit a new WASM hash.
A configurable set of approvers (seeded with the admin at `upgrade_init` time)
each call `upgrade_approve`. Once the required number of approvals is reached
**and** the timelock delay has elapsed, any approver may call `upgrade_execute`
to apply the upgrade atomically.

```
upgrade_propose  →  upgrade_approve (×N)  →  upgrade_execute
     │                     │                       │
     │  eta = now + 600 000 ledgers                │
     │  expires = now + 1 200 000 ledgers          │
     └─────────── proposal stored ────────────────►│
                                          deployer().update_current_contract_wasm
```

### Key Constants

| Constant | Value | Approximate wall-clock time |
|---|---|---|
| `MIN_THRESHOLD_DELAY_LEDGERS` | 600 000 | ~7 days at 5 s/ledger |
| `DEFAULT_PROPOSAL_EXPIRY_LEDGERS` | 1 200 000 | ~14 days at 5 s/ledger |
| `MAX_APPROVERS` | 32 | — |

## Upgrade Governance Entrypoints

All functions below live on the lending contract (not a separate contract).
Replace `$LENDING_CONTRACT` with the deployed lending contract ID and
`$ADMIN_KEY` / `$APPROVER_KEY` with the appropriate Stellar secret keys.

### Step 0 — Initialize upgrade governance (once, admin only)

```bash
stellar contract invoke \
  --id $LENDING_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- upgrade_init \
  --caller $ADMIN_ADDRESS \
  --current_wasm_hash $CURRENT_WASM_HASH \
  --required_approvals 2
```

Stores the current WASM hash, sets the approval threshold, and seeds the
approver list with the admin. Must be called exactly once; a second call
returns `AlreadyInitialized`.

### Step 1 — Propose a WASM upgrade (admin only)

```bash
stellar contract invoke \
  --id $LENDING_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- upgrade_propose \
  --caller $ADMIN_ADDRESS \
  --new_wasm_hash $NEW_WASM_HASH \
  --new_version 2
```

- `new_version` must be strictly greater than the current stored version.
- Returns a `proposal_id` (u64) used in subsequent steps.
- Sets `eta_ledger = current_ledger + 600 000` and
  `expires_at_ledger = current_ledger + 1 200 000`.
- Emits `UpgradeProposedEvent`.

### Step 2 — Approve (approver accounts, once each)

```bash
stellar contract invoke \
  --id $LENDING_CONTRACT \
  --source $APPROVER_KEY \
  --network testnet \
  -- upgrade_approve \
  --caller $APPROVER_ADDRESS \
  --proposal_id $PROPOSAL_ID
```

- Only addresses in the approver set may call this.
- Each address may approve at most once per proposal.
- Returns the running `approval_count`.
- Emits `UpgradeApprovedEvent`.

### Step 3 — Execute after timelock elapses (any approver)

```bash
stellar contract invoke \
  --id $LENDING_CONTRACT \
  --source $APPROVER_KEY \
  --network testnet \
  -- upgrade_execute \
  --caller $APPROVER_ADDRESS \
  --proposal_id $PROPOSAL_ID
```

- Requires `current_ledger >= eta_ledger` (7-day minimum delay).
- Requires `approval_count >= required_approvals`.
- Calls `env.deployer().update_current_contract_wasm` atomically.
- Updates `CurrentVersion` and `CurrentWasmHash` in storage.
- Emits `UpgradeExecutedEvent`.
- Each proposal may execute at most once (`ProposalAlreadyExecuted` on retry).

## Approver Management

### Add an approver (admin only)

```bash
stellar contract invoke \
  --id $LENDING_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- upgrade_add_approver \
  --caller $ADMIN_ADDRESS \
  --approver $NEW_APPROVER_ADDRESS
```

### Remove an approver (admin only)

Removing is rejected if it would leave `approver_count <= required_approvals`
or bring the set below one member.

```bash
stellar contract invoke \
  --id $LENDING_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- upgrade_remove_approver \
  --caller $ADMIN_ADDRESS \
  --approver $APPROVER_ADDRESS
```

### Change the required approval count (admin only)

```bash
stellar contract invoke \
  --id $LENDING_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- upgrade_set_required_approvals \
  --caller $ADMIN_ADDRESS \
  --required_approvals 3
```

In-flight proposals keep the threshold that was snapshotted at propose time;
this call only affects future proposals.

## Read-Only Queries

```bash
# Current stored version
stellar contract invoke --id $LENDING_CONTRACT --network testnet \
  -- current_version

# Proposal status (Pending / Executed / Expired) + approval count
stellar contract invoke --id $LENDING_CONTRACT --network testnet \
  -- upgrade_status --proposal_id $PROPOSAL_ID

# Who has approved so far
stellar contract invoke --id $LENDING_CONTRACT --network testnet \
  -- get_proposal_approvals --proposal_id $PROPOSAL_ID

# Full approver list
stellar contract invoke --id $LENDING_CONTRACT --network testnet \
  -- get_upgrade_approvers

# Required approvals threshold
stellar contract invoke --id $LENDING_CONTRACT --network testnet \
  -- get_required_approvals

# Minimum delay constant (always 600 000 ledgers)
stellar contract invoke --id $LENDING_CONTRACT --network testnet \
  -- get_min_upgrade_delay_ledgers
```

## Events

| Event | Emitted by | Key fields |
|---|---|---|
| `UpgradeProposedEvent` | `upgrade_propose` | `proposer`, `proposal_id`, `new_wasm_hash`, `new_version`, `eta_ledger`, `expires_at_ledger` |
| `UpgradeApprovedEvent` | `upgrade_approve` | `approver`, `proposal_id`, `approval_count` |
| `UpgradeExecutedEvent` | `upgrade_execute` | `executor`, `proposal_id`, `new_version`, `new_wasm_hash`, `ledger` |
| `UpgradeApproverAddedEvent` | `upgrade_add_approver` | `admin`, `approver` |
| `UpgradeApproverRemovedEvent` | `upgrade_remove_approver` | `admin`, `approver` |

## Error Reference

| Error | Cause |
|---|---|
| `UpgradeNotInitialized` | `upgrade_init` has not been called yet |
| `AlreadyInitialized` | `upgrade_init` called a second time |
| `InvalidUpgradeVersion` | `new_version <= current_version` |
| `InvalidUpgradeConfig` | `required_approvals` is 0, exceeds approver count, or removal would break quorum |
| `ProposalNotFound` | Unknown `proposal_id` |
| `ProposalAlreadyExecuted` | Proposal was already executed |
| `ProposalExpired` | `current_ledger > expires_at_ledger` |
| `ProposalNotReady` | `current_ledger < eta_ledger` (timelock not elapsed) |
| `InsufficientUpgradeApprovals` | Not enough approvals collected yet |
| `AlreadyApproved` | This approver already approved this proposal |
| `ApproverNotFound` | Address is not in the approver set |
| `MaxApproversReached` | Approver set is at the 32-address limit |
| `Unauthorized` | Caller is not an approver (for `upgrade_approve` / `upgrade_execute`) |

## Multisig Threshold Changes (7-day timelock)

> See also: [Multisig Change Lifecycle Guide](../stellar-lend/contracts/multisig/docs/CHANGE_LIFECYCLE.md)
> for state diagrams, full event schema, signer-change flow, and cancellation rules.

The multisig contract (`stellarlend-multisig`) enforces its own independent
timelock on threshold adjustments via `queue_threshold_change` →
`apply_threshold_change`. The minimum delay is also 600 000 ledgers (~7 days).

```bash
# Queue a new threshold (admin only)
stellar contract invoke \
  --id $MULTISIG_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- queue_threshold_change \
  --new_threshold 2

# Apply after 7 days
stellar contract invoke \
  --id $MULTISIG_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- apply_threshold_change
```

**Implementation signatures**:

```rust
pub fn queue_threshold_change(env: Env, new_threshold: u32) -> Result<(), MultisigError>
pub fn apply_threshold_change(env: Env) -> Result<(), MultisigError>
pub fn get_pending_threshold_change(env: Env) -> Option<ThresholdChange>
pub fn get_min_threshold_delay_ledgers(env: Env) -> u32
```

## Multisig Proposal Cancel State Machine

The multisig contract's `cancel_proposal` entrypoint transitions proposals from `Active` to `Cancelled`. A cancelled proposal is permanently dead — it cannot be approved, executed, or batch-executed.

### Cancel State Machine

```
                    ┌─────────────────────────────────────┐
                    │                                     │
                    │   create_proposal                   │
                    │         │                           │
                    │         v                           │
                    │   ┌──────────┐    cancel_proposal   │
                    │   │  Active  │ ──────────────────►  │
                    │   └──────────┘    ┌───────────┐    │
                    │         │         │ Cancelled │    │
                    │         │         └───────────┘    │
                    │         │              │           │
                    │         v              │           │
                    │   ┌──────────┐         │           │
                    │   │  Passed  │         │           │
                    │   └──────────┘         │           │
                    │         │              │           │
                    │         v              │           │
                    │   ┌──────────┐         │           │
                    │   │ Executed │         │           │
                    │   └──────────┘         │           │
                    │         │              │           │
                    │         v              │           │
                    │   ┌──────────┐         │           │
                    │   │ Expired  │         │           │
                    │   └──────────┘         │           │
                    │                                     │
                    └─────────────────────────────────────┘
```

### Cancel Guards

| Gate | Reverts with | Triggered when |
|---|---|---|
| Non-signer | `Unauthorized` | Caller is not a registered signer |
| Already executed | `AlreadyExecuted` | `proposal.status == Executed` |
| Already cancelled | `AlreadyCancelled` | `proposal.status == Cancelled` |
| Expired | `ProposalExpired` | `ledger.sequence() > proposal.expires_at` |

### Cancel Effects

- The `Proposal.status` field is set to `Cancelled` in persistent storage.
- `approve_proposal`, `execute_proposal`, and `batch_execute` all check `proposal.status == Cancelled` **before** any other processing and return `AlreadyCancelled`.
- A cancelled proposal can never be revived or re-executed — the status is permanent.

### Cancel Authorization

- **Any registered signer** may call `cancel_proposal`, not only the proposer.
- `caller.require_auth()` is enforced so the caller must authorize the cancel invocation.
- `Self::require_signer` verifies the caller is in the registered signer set.

---

## Timelock Test Coverage

### Upgrade governance (`contracts/lending/src/upgrade.rs`)

| Test | Scenario |
|---|---|
| Happy-path propose → approve → execute | Proposal executes after 600 000 ledgers with sufficient approvals |
| Execute before ETA | Returns `ProposalNotReady` |
| Execute with insufficient approvals | Returns `InsufficientUpgradeApprovals` |
| Double-execute | Returns `ProposalAlreadyExecuted` |
| Execute expired proposal | Returns `ProposalExpired` |
| Duplicate approval | Returns `AlreadyApproved` |
| Non-approver calling approve/execute | Returns `Unauthorized` |
| `new_version <= current_version` | Returns `InvalidUpgradeVersion` |

### Multisig threshold timelock (`contracts/multisig`)

| Test | Ledger position | Expected outcome |
|---|---|---|
| `test_queue_threshold_change_success` | at queue ledger | change queued, eta = queue + 600 000 |
| `test_apply_threshold_change_before_delay` | queue + (MIN − 1) | `DelayNotElapsed` |
| `test_apply_at_exact_min_delay_boundary` | queue + MIN − 1 then queue + MIN | first `DelayNotElapsed`; second succeeds |
| `test_apply_threshold_change_after_delay` | queue + MIN | threshold updated, pending cleared |
| `test_same_ledger_protection` | same ledger as queue | `DelayNotElapsed` |

### Multisig cancel lifecycle (`contracts/multisig/src/cancel_proposal_test.rs`)

| Test | Scenario | Expected outcome |
|---|---|---|
| `test_cancel_proposal_status_is_cancelled` | Cancel an Active proposal | Status becomes `Cancelled` |
| `test_cancel_proposal_by_non_proposer_signer` | Non-proposer signer cancels | Status becomes `Cancelled` |
| `test_cancel_already_cancelled_returns_error` | Double-cancel | `AlreadyCancelled` |
| `test_cancel_executed_proposal_returns_error` | Cancel an executed proposal | `AlreadyExecuted` |
| `test_cancel_expired_proposal_returns_error` | Cancel an expired proposal | `ProposalExpired` |
| `test_cancel_proposal_non_signer_rejected` | Non-signer calls cancel | `Unauthorized` |
| `test_execute_cancelled_proposal_returns_error` | Execute after cancel | `AlreadyCancelled` |
| `test_batch_execute_cancelled_proposal_rejected` | Batch-execute after cancel | `AlreadyCancelled` |
| `test_approve_cancelled_proposal_returns_error` | Approve (vote) after cancel | `AlreadyCancelled` |

## Integration Checklist

### Pre-deployment

- [ ] Call `upgrade_init` with the deployed WASM hash and desired approval threshold
- [ ] Add all required approver addresses via `upgrade_add_approver`
- [ ] Verify approver list with `get_upgrade_approvers`
- [ ] Verify threshold with `get_required_approvals`
- [ ] Run a test proposal end-to-end on testnet
- [ ] Set up event monitoring for `UpgradeProposedEvent` and `UpgradeExecutedEvent`

### Upgrade procedure

- [ ] Build and upload the new WASM; note the resulting hash
- [ ] Increment the version number (must be > `current_version`)
- [ ] Call `upgrade_propose` and record the returned `proposal_id`
- [ ] Notify all approvers with the proposal ID and new WASM hash
- [ ] Collect approvals from `required_approvals` distinct approver accounts
- [ ] Wait until `current_ledger >= eta_ledger` (~7 days)
- [ ] Call `upgrade_execute` to apply the upgrade
- [ ] Verify new version with `current_version`

### Ongoing operations

- [ ] Monitor `UpgradeProposedEvent` for unexpected proposals
- [ ] Audit approver set periodically via `get_upgrade_approvers`
- [ ] Rotate approver keys by pairing `upgrade_add_approver` + `upgrade_remove_approver`

## Vesting Treasury Sink

When a vesting grant is revoked by the configured admin, any unvested tokens
are clawed back and deposited to the protocol treasury address. Operators
should note:

- **Revocation Authority**: Only the configured `admin` may call `revoke(grantee)`.
- **Cliff Behavior**: No tokens become claimable until `now >= start + cliff_seconds`.
- **Treasury Sink**: Unvested balance at the time of revoke is transferred to
  the protocol treasury address configured in the vesting contract.
- **Monitoring**: Watch vesting revoke events and treasury inflows to detect
  unexpected revocations.
