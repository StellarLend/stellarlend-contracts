# Incident Response & Pause Mechanisms

The StellarLend protocol provides several layers of protection to handle security incidents, market volatility, or technical issues. These mechanisms allow the protocol administrators to halt or restrict operations to protect user funds.

## 0. TL;DR — Decision Tree

| Symptom | First action | Why |
|---|---|---|
| **Suspected validator-set compromise on a bridge** | `freeze_bridge` (guardian) | Halts all outbound withdrawals within one transaction. |
| Single bad oracle / specific borrow spike | `set_emergency_pause` | Halts every mutating op; safe to lift after assessment. |
| Unknown, broad, or exploit in progress | `set_read_only_mode` | Snapshot the chain exactly; even admin cannot clobber evidence. |
| Maintenance only | `set_pause_switch` for the affected op | Smallest blast-radius. |

The freeze described below is **not** "one of the pause mechanisms" in the
rest of this table — it is an independent, break-glass control that lives
next to the bridge validator set and is operated by a separate `Guardian`
role. It is deliberately faster, smaller in scope, and reserved for the
incidents above where every minute of exposure matters.

## 1. Pause Mechanisms Overview

| Mechanism | Scope | Impact | Recommended Use Case |
|-----------|-------|--------|----------------------|
| **Per-Operation Pause** | Specific function (e.g., Deposit) | Only the specific operation is disabled. Others remain active. | Minor issues in specific modules, maintenance. |
| **Emergency Pause** | Global | ALL mutating operations are disabled. View functions remain available. | Major security breach, critical bug discovery. |
| **Read-Only Mode** | Global (Highest Precedence) | ALL state-changing operations (including admin config) are disabled. View functions remain available. | Investigation of complex incidents where even admin state changes might be risky. |

## 2. Read-Only Mode

Read-Only Mode is the most restrictive state of the protocol. When enabled, it ensures that no state transitions can occur within the contract, providing a "frozen" snapshot for investigation.

### Impact of Read-Only Mode
- **Mutating Operations Disabled:** `deposit`, `withdraw`, `borrow`, `repay`, `liquidate`, and `flash_loan` will all fail with a `ReadOnlyMode` error.
- **Admin Operations Disabled:** `set_risk_params`, `update_interest_rate_config`, and other configuration updates are blocked.
- **View Functions Available:** All `get_*` functions and analytics reporting remain fully functional.
- **Exceptions:** Only `set_read_only_mode` itself can be called by the admin to toggle the mode.

### Precedence Matrix
If multiple pause mechanisms are active simultaneously, the most restrictive one takes precedence:
1. **Read-Only Mode** (Overrides everything)
2. **Emergency Pause** (Overrides per-operation switches)
3. **Per-Operation Pause** (Lowest precedence)

## 3. Incident Response Guidance

### Minor Bug or Maintenance
If a bug is identified in a specific operation (e.g., a display error in deposits), use **Per-Operation Pause** for that specific function:
```sh
soroban contract invoke --id <ID> --fn set_pause_switch --arg caller=<ADMIN> --arg operation=pause_deposit --arg paused=true
```

### Suspected Security Breach
If a security breach is suspected but its extent is unknown, immediately trigger the **Emergency Pause**:
```sh
soroban contract invoke --id <ID> --fn set_emergency_pause --arg caller=<ADMIN> --arg paused=true
```

### Critical Incident / Forensic Investigation
If a critical exploit has occurred or the protocol state must be preserved exactly for forensic analysis, enable **Read-Only Mode**:
```sh
soroban contract invoke --id <ID> --fn set_read_only_mode --arg caller=<ADMIN> --arg enabled=true
```

## 4. Bridge Freeze (Incident-Response Break-Glass)

In addition to the global pause mechanisms above, the bridge surface
(`bridge_withdraw`, `bridge_deposit`, `register_bridge`, `set_bridge_fee`,
`set_bridge_guardian`) carries its own **freeze** control that can be
tripped instantly by a single `Guardian` address — no multisig, no
governance vote, no validator-set rotation.

The freeze is **independent** of the validator-set rotation that lives in
`contracts/bridge/` (the off-chain validator/quorum layer). It exists so
that during a suspected validator-set compromise, the guardian can stop
outbound withdrawals **now** while the slower rotation discussion runs in
parallel.

### 4.1 State formula

Let `F` denote the freeze flag stored in *instance* storage at
`BridgeDataKey::IsFrozen` (boolean). Then the steady-state semantics are:

```
F = false   →   bridge_withdraw is permitted for any registered network
F = true    →   bridge_withdraw returns BridgeError::Frozen (mutates NOTHING)
bridge_deposit is permitted REGARDLESS of F
register_bridge / set_bridge_fee / set_bridge_guardian are NOT affected by F
```

The transition `F: false → F: true` (and the reverse) emits exactly one
event on the topic `("bridge", "v1", "freeze")` with payload
`BridgeFreezeEvent { schema_version, is_frozen, guardian, timestamp }`. A
redundant call (freeze when already frozen, or unfreeze when already
unfrozen) is a **no-op** and does **not** emit a duplicate event.

### 4.2 Authorization

The freeze is gated by a single `Guardian` address stored in *instance*
storage at `BridgeDataKey::Guardian` — this address is **deliberately
disjoint** from the `Admin` address that controls the rest of the bridge.
The intent is that a key compromise on one role cannot unilaterally lift
the other's controls.

- `freeze_bridge(caller)` — succeeds iff `caller.require_auth()` matches
  the stored `Guardian`; otherwise returns `Unauthorized` (or
  `GuardianNotConfigured` if no guardian has been set yet).
- `unfreeze_bridge(caller)` — same rule; lifts the freeze and emits the
  transition event.
- `set_bridge_guardian(admin, new_guardian)` — admin-only; allows
  rotation if the guardian key is itself compromised.

### 4.3 What the freeze does NOT touch

- **Deposit leg is exempt.** `bridge_deposit` continues to function so
  that user funds are not stranded on the bridge. (See
  `bridge_fee_test::prop_deposit_withdraw_round_trip_no_extra_value` for
  the value-conservation invariant.)
- **Admin operations are exempt.** During an incident we still need to
  be able to rotate the guardian, update fees, or register a backup
  bridge.
- **Read functions continue to work.** Off-chain monitors can keep
  reading `is_bridge_frozen()`, `get_bridge_config()`, and `list_bridges()`
  to drive dashboards.

### 4.4 Worked example (incident timeline)

**Setup.** The admin sets:

- Admin = `G...ADMIN`
- Guardian = `G...GA` (the guardian whose key is offline / cold)
- Registered bridge on `network_id = 7` with `fee_bps = 30`

**t = 0** (steady state):
- `F = false`
- `is_bridge_frozen()` returns `false`
- Withdrawals and deposits both succeed.

**t = T** (validator-set compromise suspected; a withdrawal burst of
unusual size is in flight):

1. Off-chain monitor calls the guardian out-of-band ("freeze bridge 7").
2. Guardian hot-signs a single transaction:
   ```sh
   soroban contract invoke \
     --id <CONTRACT_ID> \
     --fn freeze_bridge \
     --arg caller=<GUARDIAN_ADDRESS>
   ```
3. The transaction succeeds (`Ok(())`), one `BridgeFreezeEvent {
   is_frozen: true, guardian: G...GA, ... }` is emitted, and `F` flips
   to `true`.
4. From this block onwards, every `bridge_withdraw` returns immediately
   with `BridgeError::Frozen`. `user.require_auth()` is still invoked
   (so that a frozen retry cannot be confused with a successful
   withdrawal) but no storage write or token transfer occurs.

**t = T + Δ** (investigation proceeds; coordination on validator rotation
happens off-chain).

**t = T'** (rotation is finalised):

1. Guardian signs the unfreeze:
   ```sh
   soroban contract invoke \
     --id <CONTRACT_ID> \
     --fn unfreeze_bridge \
     --arg caller=<GUARDIAN_ADDRESS>
   ```
2. One `BridgeFreezeEvent { is_frozen: false, ... }` is emitted. `F` flips
   back to `false`. Withdrawals resume.

### 4.5 Runbook checklist

When performing a bridge freeze:

- [ ] Confirm the threat model matches this runbook (validator-set
  compromise on the bridge, not a generic protocol exploit). For other
  threats, prefer §3 procedures.
- [ ] Verify caller is the configured guardian (`is_bridge_frozen`
  before-and-after check is enough).
- [ ] After the transaction, query `is_bridge_frozen()` and confirm
  `true`.
- [ ] Subscribe to `("bridge", "v1", "freeze")` for the transition
  event; you should see exactly one entry with `is_frozen: true`.
- [ ] Off-chain, pause any withdraw-queue items that referenced the
  bridge — the contract will reject them, but the indexer may still
  hold signed-but-unprocessed orders.
- [ ] Coordinate validator rotation *off*-chain while the freeze holds.
- [ ] When confident, unfreeze with the same guardian key. Document the
  unfreeze timestamp and pull the audit trail from the event stream.

### 4.6 Security notes & limitations

- The `Admin` and `Guardian` roles are disjoint by design. A compromise
  of one role cannot, by itself, lift a freeze set by the other.
- The freeze lives in *instance* storage: its lifetime is the
  contract's instance lifetime. It is not pruned across upgrades; if
  the contract is upgraded the freeze persists because the instance is
  migrated alongside the storage.
- Removing and updating the freeze requires guardian auth. There is no
  governance proposal path for the freeze control — that is intentional;
  break-glass controls work by being fast to trigger and slow to override.
- All freeze-related errors (`Frozen`, `Unauthorized`,
  `GuardianNotConfigured`) are deterministic and inspectable on the
  failed transaction — no off-chain signal is required.

## 5. Security Notes & Limitations (Global)

- **Authorization:** Only the designated `Admin` address can toggle these switches.
- **Persistence:** All pause states are stored in persistent storage and remain active across ledger updates until explicitly disabled.
- **View-Only Guarantee:** While state-changing operations are blocked, view functions continue to read from current storage. Note that if interest accrual is triggered by a view function (if any), it will not be persisted in read-only mode.
- **Off-Chain Indexers:** Indexers should monitor for `PauseStateChanged` and `ReadOnlyMode` events to update their UI/state accordingly.
