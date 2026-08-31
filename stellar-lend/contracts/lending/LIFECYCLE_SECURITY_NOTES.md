# Lifecycle Security Notes — Pause & Emergency States

## Overview

The lending protocol implements a layered defence model for halting and
recovering from incidents. Two independent mechanisms can restrict operations:

1. **Granular pause flags** — per-operation toggles set by the admin at any time.
2. **Emergency state machine** — protocol-wide lifecycle (Normal → Shutdown → Recovery → Normal).

Both mechanisms are composable: granular pause flags are checked first, then the
emergency state is checked on top. Every state-mutating entry point in `lib.rs`
calls `check_pause_status` before `check_emergency_status`.

---

## Operation Permission Matrix

| Operation          | Normal | Granular-paused | Shutdown | Recovery | Normal (post) |
|--------------------|--------|-----------------|----------|----------|---------------|
| `deposit`          | ✓      | ✗ (if paused)   | ✗        | ✗        | ✓             |
| `borrow`           | ✓      | ✗ (if paused)   | ✗        | ✗        | ✓             |
| `repay`            | ✓      | ✗ (if paused)   | ✗        | **✓**    | ✓             |
| `withdraw`         | ✓      | ✗ (if paused)   | ✗        | **✓**    | ✓             |
| `liquidate`        | ✓      | ✗ (if paused)   | ✗        | **✓**    | ✓             |
| `flash_loan`       | ✓      | —               | ✗        | ✗        | ✓             |

---

## Emergency State Machine

```
 ┌─────────┐  guardian/admin       ┌──────────┐  admin only  ┌──────────┐
 │ Normal  │ ─────────────────────▶│ Shutdown │ ────────────▶│ Recovery │
 └─────────┘  emergency_shutdown   └──────────┘ start_recovery└──────────┘
      ▲                                                             │
      └─────────────────────────────────────────────────────────────┘
                             complete_recovery (admin only)
```

### Forbidden transitions
- `Normal → Recovery` directly: **blocked** (`ProtocolPaused` error)
- `Normal → complete_recovery`: **blocked**
- `Shutdown → Normal` directly: **blocked**
- `Recovery → shutdown` via `emergency_shutdown`: **allowed** (override for re-escalation)

---

## Incident Response Runbook

### Step 1 — Detect & Halt
```
client.emergency_shutdown(&guardian);   // or admin
```
Effect: All operations immediately denied. State = **Shutdown**.

### Step 2 — Assess
Analyse the incident off-chain. Confirm that root cause is contained before proceeding.

### Step 3 — Open Controlled Unwind
```
client.start_recovery(&admin);
```
Effect: `repay` and `withdraw` re-enabled. All new-risk ops remain blocked. State = **Recovery**.

> **Tip**: Use granular pause flags during Recovery to temporarily restrict
> even repay/withdraw (e.g., to prevent a run on specific assets).
> ```
> client.set_pause(&admin, &PauseType::Withdraw, &true);
> // ... unwind specific positions manually ...
> client.set_pause(&admin, &PauseType::Withdraw, &false);
> ```

### Step 4 — Verify & Restore
Once all open positions have been resolved:
```
client.complete_recovery(&admin);
```
Effect: All operations re-enabled. State = **Normal**.

> ⚠️ **Do not call `complete_recovery` prematurely.** Re-enabling borrow and
> deposit before the root cause is fixed re-opens the vulnerability.

---

## Security Properties Validated by Tests

| Property | Test |
|----------|------|
| Granular pause denies specific operation, leaves others open | `test_deposit_borrow_granular_pause_mid_lifecycle` |
| Global `All` pause blocks all operations simultaneously | `test_deposit_borrow_global_pause_mid_lifecycle` |
| Shutdown blocks all 5 operation types atomically | `test_shutdown_mid_lifecycle_blocks_new_risk` |
| Recovery permits only repay + withdraw | `test_recovery_mode_allows_only_unwind` |
| `complete_recovery` fully restores all operations | `test_complete_recovery_re_enables_full_lifecycle` |
| Multi-cycle + granular pauses in recovery do not leak state | `test_multi_cycle_with_partial_pauses_in_recovery` |
| Emergency state transitions are atomic and idempotent | `test_emergency_transitions_idempotent` |
| Duplicate submissions cannot corrupt state | `test_duplicate_submissions_are_noops` |
| Stale responses are ignored | `test_stale_response_ignored` |
| Interrupted operations recover without repeating on-chain action | `test_interrupted_operation_recovery` |

---

## Transactional Invariants and Recovery

### Core Invariants

1. **State validity**: The emergency state is always one of `Normal`, `Shutdown`, or `Recovery`.
2. **Transition validity**: Only the transitions encoded in the state diagram are permitted; all others are rejected.
3. **Permission gating**: `complete_recovery` requires `admin`; `emergency_shutdown` requires `guardian` or `admin`; `start_recovery` requires `admin`.
4. **No partial application**: A transition either fully applies or fails; no intermediate state is ever persisted.
5. **Idempotency**: Re-applying the current state is a success with no state change.
6. **Composability**: Granular pauses are AND-ed with emergency state; neither can bypass the other.
7. **Reentrancy safety**: All state transitions run under the reentrancy guard; recovery-mode unwind operations are protected.

### State Transition Atomicity

Every emergency-state transition (`emergency_shutdown`, `start_recovery`,
`complete_recovery`) and granular pause update is atomic. The new state is
written in a single ledger entry; a failed or interrupted operation leaves the
previous state fully intact. No partial state is observable within a
transaction or across retries.

### Success, Rejection, Cancellation, and Retry Paths

- **Success**: Operation completes and the state transition is emitted as an
  event. The caller receives the new state.
- **Rejection**: An unauthorized or invalid transition is rejected before any
  state change. The rejection error is deterministic and contains the reason.
- **Cancellation**: A user-initiated cancellation (if supported by the caller
  SDK) is treated as a client-side discard; it does not invoke an on-chain
  state change.
- **Retry**: Every state-mutation call is idempotent when the target state is
  already active:
  - `emergency_shutdown` when already `Shutdown` → success, no state change.
  - `start_recovery` when already `Recovery` → success, no state change.
  - `complete_recovery` when already `Normal` → success, no state change.
  - `set_pause` to the same value → success, no state change.
  
  This ensures a duplicate submission (wallet retry, reorg, double-tap) cannot
  corrupt the state machine.

### Duplicate and Stale Response Prevention

Clients must include a unique `op_id` in each transaction and check that the
emitted event's `op_id` matches the request. Any response carrying an `op_id`
older than the latest confirmed operation is stale and MUST be ignored. The
contract itself does not need to track client nonces for pause/emergency
transitions because idempotency provides the same protection.

### Failure Recovery Preserving User Intent

For user operations that may be interrupted (e.g., deposit/repay after signing),
the SDK persists a `PendingIntent` containing the operation parameters and the
original transaction hash. On retry, the SDK first checks whether the original
transaction hash is already confirmed on-chain:
- **Confirmed** → do not resubmit. Surface the original success to the user.
- **Not confirmed** → resubmit the exact operation with a fresh `op_id`.

This prevents silently repeating an on-chain action while still recovering the
user's original intent.

### Invariant Enforcement

All invariants are enforced by the same code path that checks granular pause
flags and emergency state. The invariant layer is independent of the
permission layer: an operation can only proceed if the invariants hold, even if
the caller has admin/guardian rights.

---

## Threat Model Notes

- **Compromised guardian key**: Can trigger Shutdown, pausing all operations.
  Cannot start or complete recovery (admin-only). Impact is limited to a
  temporary halt. Rotate the guardian key and call `complete_recovery` after
  confirming Normal state.

- **Compromised admin key**: Can do everything, including calling
  `complete_recovery` prematurely. Protect the admin key with a multisig
  governance process.

- **Granular pause bypass**: Granular flags are applied in addition to
  emergency state, not instead of it. A Deposit-unpause during Shutdown still
  does not allow deposits. Each operation checks both layers.

- **Re-entrancy during Recovery**: The reentrancy guard remains active during
  recovery mode. `repay` and `withdraw` are guarded; a flash-loan callback
  cannot exploit recovery-mode unwind to drain the pool.
