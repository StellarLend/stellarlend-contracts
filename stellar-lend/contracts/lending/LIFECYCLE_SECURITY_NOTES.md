# Lifecycle Security Notes — Pause & Emergency States

## Overview

The lending protocol implements a layered defence model for halting and
recovering from incidents. Two independent mechanisms can restrict operations:

1. **Granular pause flags** — per-operation toggles set by the admin at any time.
2. **Emergency state machine** — protocol-wide lifecycle (Normal → Shutdown → Recovery → Normal).

Both mechanisms are composable: emergency state enforcement is applied first, then
granular pause flags layer on top.

---

## Operation Permission Matrix

| Operation          | Normal | Granular-paused | Shutdown | Recovery | Normal (post) |
|--------------------|--------|-----------------|----------|----------|---------------|
| `deposit`          | ✓      | ✗ (if paused)   | ✗        | ✗        | ✓             |
| `deposit_collateral` | ✓    | ✗ (if paused)   | ✗        | ✗        | ✓             |
| `borrow`           | ✓      | ✗ (if paused)   | ✗        | ✗        | ✓             |
| `repay`            | ✓      | ✗ (if paused)   | ✗        | **✓**    | ✓             |
| `withdraw`         | ✓      | ✗ (if paused)   | ✗        | **✓**    | ✓             |
| `liquidate`        | ✓      | ✗ (if paused)   | ✗        | ✗        | ✓             |
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
