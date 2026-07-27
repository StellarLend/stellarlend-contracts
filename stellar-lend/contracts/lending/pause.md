# Protocol Pause Mechanism

The StellarLend lending contract exposes a **granular pause mechanism** and an
**emergency lifecycle state machine** to ensure safety during emergency
situations or maintenance windows. The two layers are independent and
complementary.

## Features

- **Granular Control**: Pause specific operations (`Deposit`, `Borrow`, `Repay`,
  `Withdraw`, `Liquidation`, `FlashLoan`) without affecting others.
- **Global Pause**: A master switch (`All`) that immediately halts every operation.
- **Admin & Guardian Managed**: The admin or the configured guardian can toggle
  individual pause flags and trigger an emergency shutdown.
- **Guardian Trigger**: A configured guardian (e.g., a security multisig) can
  trigger emergency shutdown without waiting for full governance latency.
- **Recovery Mode**: After a shutdown the admin can move the protocol into a
  controlled unwind mode by calling `set_emergency_state(EmergencyState::Recovery)`
  so users can repay debt and withdraw collateral.
- **Event Driven**: Every pause and emergency state change emits an
  `EmergencyStateChangedEvent` / `PauseStateChangedEvent` for transparent
  off-chain monitoring.
- **Auto-Expiry**: Each granular pause switch carries an `expires_at_ledger` and
  automatically clears when ledger sequence progresses past its expiry.
- **Read-Only Mode**: A separate incident-response switch that blocks all
  state-changing operations while keeping view functions available.

> **Note — recovery / extension API.** This contract does **not** expose
> `start_recovery`, `complete_recovery`, or `extend_pause` entrypoints. Recovery
> is performed via `set_emergency_state(EmergencyState::Recovery)` (admin only),
> exit-to-Normal is `set_emergency_state(EmergencyState::Normal)` (admin only),
> and pause expiry/extension is managed via the TTL parameter on `set_pause`.
> A separate `start_recovery` / `approve_recovery` / `execute_recovery` flow
> exists in the `hello-world` crate's `governance` module, but it is unrelated
> to this lending contract.

## Auto-Expiry Lifecycle

- Each granular pause is stored as a struct with `paused: bool` and
  `expires_at_ledger: u32`.
- A pause is considered active only while
  `env.ledger().sequence() < expires_at_ledger`.
- When ledger sequence exceeds `expires_at_ledger`, the paused operation is
  treated as unpaused without any storage rewrite.
- Operators can either re-issue a pause with `set_pause(operation, paused=true, ttl_ledgers=N)`
  or call `set_pause(operation, paused=false, ttl_ledgers=0)` to explicitly
  clear an active pause.

## Operation Types

| Enum Value    | Description                                                         |
| ------------- | ------------------------------------------------------------------- |
| `All`         | Global pause that supersedes all individual flags.                  |
| `Deposit`     | Prevents new collateral deposits (`deposit`, `deposit_collateral`). |
| `Borrow`      | Prevents new loan originations.                                     |
| `Repay`       | Prevents loan repayments (use with caution).                        |
| `Withdraw`    | Prevents collateral withdrawals.                                    |
| `Liquidation` | Prevents liquidations.                                              |
| `FlashLoan`   | Prevents flash loan issuance and repayment (`flash_loan`, `repay_flash_loan`). |

(`ReadOnly` is a separate protocol-level mode; see
[`emergency_shutdown.md`](./emergency_shutdown.md).)

## Liquidation-Pause Policy

The protocol follows an explicit liquidation policy that balances **solvency protection** with **market health** during different pause and emergency states.

### Policy Matrix

| State/Emergency                      | Liquidation Paused | Liquidation Behavior | Rationale                                                                                                                        |
| ------------------------------------ | ------------------ | -------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| **Normal** + Liquidation Pause       | **Yes**            | **BLOCKED**          | **Solvency Protection**: Prevents potentially solvent positions from being liquidated during oracle issues or market volatility. |
| **Normal** + Other Operations Paused | **No**             | **ALLOWED**          | **Market Health**: Allows the market to self-correct unhealthy positions while preventing new risk.                              |
| **Normal** + Global Pause (`All`)    | **Yes**            | **BLOCKED**          | **Protocol Halt**: All operations including liquidations are stopped.                                                            |
| **Shutdown**                         | **Yes**            | **BLOCKED**          | **Emergency Stop**: Hard stop for all operations to prevent cascading failures.                                                  |
| **Recovery**                         | **Yes**            | **BLOCKED**          | **Unwind-Only Mode**: Only repay/withdraw allowed to safely close positions.                                                     |
| **ReadOnly**                         | **Yes**            | **BLOCKED**          | **Incident Freeze**: All state changes frozen for investigation.                                                                |

### Trade-offs and Decision Framework

#### When to Pause Liquidations (Solvency Protection)

- **Oracle Issues**: Price feed staleness, manipulation, or extreme volatility
- **Market Stress**: Flash crashes, extreme volatility events
- **Technical Issues**: Contract bugs, security vulnerabilities
- **Regulatory Concerns**: Compliance requirements or legal restrictions

#### When to Allow Liquidations (Market Health)

- **Isolated Asset Issues**: Single asset problems while other markets function
- **Gradual Market Corrections**: Allow natural liquidation of unhealthy positions
- **Liquidity Events**: Market-wide liquidity crunches where liquidations provide relief
- **Risk Management**: Prevent systemic risk buildup from unhealthy positions

### Operational Guidelines

#### Incident Response Scenarios

1. **Oracle Staleness Detected**

   ```
   Action: Pause Liquidation + Pause Borrow/Deposit
   Reason: Protect potentially solvent positions from incorrect liquidations
   Recovery: Fix oracle, then unpause in reverse order
   ```

2. **Market Volatility Event**

   ```
   Action: Pause Borrow/Deposit only (keep liquidations active)
   Reason: Allow market self-correction while preventing new risk
   Recovery: Monitor volatility, gradually unpause when stable
   ```

3. **Security Vulnerability**

   ```
   Action: Global Pause or ReadOnly Mode
   Reason: Complete halt while investigating
   Recovery: Patch vulnerability, test, then controlled unpause
   ```

4. **Liquidity Crisis**
   ```
   Action: Keep liquidations active, pause new borrowing
   Reason: Liquidations provide much-needed liquidity
   Recovery: Monitor system health, adjust as needed
   ```

5. **Flash Loan Exploit / Reentrancy Risk**
   ```
   Action: Pause FlashLoan immediately (PauseType::FlashLoan)
   Reason: Flash loans are the primary vector for price manipulation,
           reentrancy, and governance attacks during incidents
   Recovery: Fix vulnerability, audit, then unpause FlashLoan last
   ```

### Flash Loan Pause Policy

Flash loans are high-risk operations that are frequently used as attack vectors in DeFi exploits.
Both `flash_loan` and `repay_flash_loan` are gated by the pause / emergency checks
identically to the deposit/borrow/repay/withdraw entrypoints.

| Condition                       | `flash_loan` | `repay_flash_loan` |
| ------------------------------- | ------------ | ------------------ |
| Normal + No pause               | ✅ ALLOWED   | ✅ ALLOWED         |
| `PauseType::FlashLoan` active   | ❌ BLOCKED   | ❌ BLOCKED         |
| `PauseType::All` active         | ❌ BLOCKED   | ❌ BLOCKED         |
| `EmergencyState::Shutdown`      | ❌ BLOCKED   | ❌ BLOCKED         |
| `EmergencyState::Recovery`      | ❌ BLOCKED   | ❌ BLOCKED         |
| Pause expired                   | ✅ ALLOWED   | ✅ ALLOWED         |

### Security Considerations

- **Precedence Rules**: Emergency states take precedence over granular pause flags.
- **Atomic Operations**: Pause checks happen before any state changes.
- **Event Transparency**: All pause changes emit events for off-chain monitoring.
- **Role Separation**: Only the admin can set granular pauses; the guardian can
  additionally trigger `EmergencyState::Shutdown` but cannot compute with the
  lifecycle beyond that.

## Contract Interface

The pause mechanism is governed by the following public entrypoints (all
`pub fn`, accepting `Env`):

### Admin / Guardian Functions

#### `set_pause(pause_type: PauseType, paused: bool, ttl_ledgers: u32)`

Sets or clears a granular pause flag with a time-to-live expressed in ledger count.

- **Parameters**:
  - `pause_type` — The `PauseType` variant to pause or unpause (e.g. `Deposit`, `Borrow`, `All`).
  - `paused` — `true` to activate the pause, `false` to clear it. Setting `paused = false` is a
    valid unpause call regardless of TTL.
  - `ttl_ledgers` — Number of ledgers from the current sequence until the pause expires. The
    contract computes `expires_at_ledger = env.ledger().sequence() + ttl_ledgers` internally.
- **TTL Semantics**:
  - `ttl_ledgers = 0` means the pause expires immediately (at the current ledger sequence). Since
    `pause_is_active` checks `ledger < expires_at_ledger`, a TTL of 0 means `pause_is_active`
    returns `false` right away.
  - `ttl_ledgers = N` means the pause remains active for the next `N` ledgers (including the
    current one), then auto-expires.
- **Authorization**: Admin or guardian (mirrors `set_emergency_state` Shutdown auth — if a guardian
  is configured, the guardian is the expected caller; otherwise admin is required).
- **Emits**: `PauseStateChangedEvent` with `old_state` and `new_state`.

> The function supports effective extension simply by re-issuing `set_pause` with a later TTL
> — the contract does **not** expose a separate `extend_pause` entrypoint.

#### `set_guardian(guardian: Address)`

Sets or rotates the guardian authorized to trigger emergency shutdown.

- **Requires Authorization**: Yes (admin).
- **Emits**: `GuardianSetEvent`.

#### `set_emergency_state(new_state: EmergencyState)`

Transitions the protocol to a new emergency lifecycle state.

- **Authorization**:
  - `EmergencyState::Shutdown` → admin **or** guardian.
  - `EmergencyState::Recovery` → admin only.
  - `EmergencyState::Normal`   → admin only.
- **Emits**: `EmergencyStateChangedEvent { old_state, new_state }`.
- **See**: [`emergency_shutdown.md`](./emergency_shutdown.md) for the full lifecycle.

This single entrypoint replaces the `start_recovery` / `complete_recovery` pair: passing
`Recovery` enters unwind-only mode and passing `Normal` returns to full operation.

#### `emergency_shutdown(caller: Address)`

Convenience entrypoint that triggers `set_emergency_state(EmergencyState::Shutdown)`.
Accepts either the configured guardian or the admin.

#### `set_read_only(read_only: bool)`

Toggles the protocol-level read-only mode.

- **Requires Authorization**: Admin.
- **Precedence**: Blocks all user-facing mutations even if granular pause flags are off.

### Public (Read-Only) Functions

#### `get_pause_state(pause_type: PauseType) -> bool`

Returns `true` if the specified operation is currently paused — either by its own granular flag or
by the global `All` flag. No authorization required. Frontends should call this before presenting
an operation to users so they can surface a clear "paused" message instead of a failed transaction.

#### `get_admin() -> Option<Address>`

Returns the current protocol admin address.

#### `get_guardian() -> Option<Address>`

Returns the currently configured guardian, or `None` if none has been set.

#### `get_emergency_state() -> EmergencyState`

Returns the current emergency lifecycle state.

#### `is_read_only() -> bool`

Returns `true` if the protocol is currently in read-only mode. No authorization required.

| Value      | Meaning                                                                 |
| ---------- | ----------------------------------------------------------------------- |
| `Normal`   | Standard operation — all flags are honoured normally.                   |
| `Shutdown` | Hard stop — all high-risk operations blocked.                           |
| `Recovery` | Controlled unwind — `repay` and `withdraw` allowed; all others blocked. |

`ReadOnly` is a separate flag and can be toggled in any state (`Normal`, `Shutdown`, `Recovery`).

## Pause Precedence Matrix

When multiple pause flags or emergency states are active, the protocol follows a deterministic
precedence order to determine if an operation is allowed. The **Global** flag and **ReadOnly**
mode act as master overrides.

| Global Pause (`All`) | Granular Pause (e.g. `Borrow`) | Result for Operation | Rationale                                    |
| -------------------- | ------------------------------ | -------------------- | -------------------------------------------- |
| `False`              | `False`                        | **ALLOWED**          | Standard operating condition.                |
| `False`              | `True`                         | **PAUSED**           | Specific risk mitigated via granular switch. |
| `True`               | `False`                        | **PAUSED**           | Global halt supersedes granular unpause.     |
| `True`               | `True`                         | **PAUSED**           | Protocol-wide defense in depth.              |

### Emergency State Precedence

Emergency lifecycle states (`Shutdown`, `Recovery`) provide a secondary layer of protection for
high-risk entry points.

Core user entry points evaluate granular/global pause flags first, then emergency lifecycle state.
This keeps the `All` flag and operation-specific flags available as immediate circuit breakers,
including for `Recovery` unwind paths that would otherwise be allowed.

| Emergency State | Granular Pause | High-Risk Op (e.g. `Borrow`) | Unwind Op (e.g. `Repay`) | Flash Loan |
| --------------- | -------------- | ---------------------------- | ------------------------ | ---------- |
| `Normal`        | `False`        | Allowed                      | Allowed                  | Allowed    |
| `Shutdown`      | `False`        | **PAUSED**                   | **PAUSED**               | **PAUSED** |
| `Recovery`      | `False`        | **PAUSED**                   | Allowed                  | **PAUSED** |
| `Recovery`      | `True`         | **PAUSED**                   | **PAUSED**               | **PAUSED** |

### Read-Only Mode

The `ReadOnly` switch is the highest precedence master switch. When active, it blocks **ALL**
state-mutating operations, regardless of the status of any other pause flags or emergency states.

## Emergency Lifecycle

```
Normal ──(set_emergency_state(Shutdown) — admin|guardian)──► Shutdown
                                                                  │
                                                                  ▼
Recovery ◀──(set_emergency_state(Recovery) — admin)── Shutdown
   │
   └──(set_emergency_state(Normal) — admin)──► Normal
```

During **Recovery**, repay / withdraw remain available only when their granular pause flag and the
global `All` flag are inactive. Deposit, borrow, and liquidation remain blocked by emergency state.

## Events

| Event                      | Topic                     | Emitted by                                |
| -------------------------- | ------------------------- | ----------------------------------------- |
| `PauseStateChangedEvent`   | `pause_state_changed_event` | `set_pause`                               |
| `GuardianSetEvent`         | `guardian_set_event`      | `set_guardian`                            |
| `EmergencyStateChangedEvent` | `emergency_state_event` | `set_emergency_state`                     |

## Security Assumptions

1. **Admin Trust**: The admin should be a multisig or DAO-governed address to avoid single-key
   centralization risk. Compromise of the admin key allows arbitrary pause/unpause and
   state lifecycle control.

2. **Guardian Scope**: The guardian can trigger `EmergencyState::Shutdown` and `set_pause`. It
   cannot exit the shutdown, enter Recovery, set ReadOnly, or rotate itself — those paths
   require the admin key. Configure the guardian as a lower-latency security multisig.

3. **Persistence**: All pause and emergency states are stored in persistent storage so they survive
   ledger upgrades and contract updates.

4. **No Bypass**: Every operation entry point enforces pause and emergency checks independently
   (defense in depth). This includes `flash_loan` and `repay_flash_loan`, which are gated
   identically to deposit/borrow/repay/withdraw. There is no mutating path that skips both layers.

5. **Global Overrides Local**: The `All` pause flag supersedes individual unpause flags. Setting
   `Deposit = false` while `All = true` still blocks deposit operations.

6. **Read-Only Mode Precedence**: Read-only mode blocks ALL user-facing mutations (deposit, borrow,
   repay, withdraw, liquidate) and most admin operations (including oracle updates). It is
   intended for rapid incident response where the state must be frozen. View functions remain
   functional.

7. **Least-Risk Recovery**: During `Recovery`, only the unwind path (`repay`, `withdraw`) is
   available. Even in recovery, granular pause flags for `Repay` and `Withdraw` are still
   respected — the admin retains fine-grained control.

8. **Reentrancy**: Flash loan operations carry a dedicated reentrancy guard (separate from the
   pause mechanism). The pause check is performed before the guard is engaged.

## Usage Examples (Rust SDK)

```rust
// Pause borrowing for 100 ledgers
client.set_pause(&PauseType::Borrow, &true, &100u32);

// Re-enable borrowing (paused=false is an unpause)
client.set_pause(&PauseType::Borrow, &false, &0u32);

// Query pause state before presenting UI options
let borrow_paused = client.get_pause_state(&PauseType::Borrow);

// Global pause for 500 ledgers
client.set_pause(&PauseType::All, &true, &500u32);

// Pause with immediate expiry (ttl=0 means pause_is_active returns false)
client.set_pause(&PauseType::Deposit, &true, &0u32);

// Extend an active pause by re-issuing with a later TTL
client.set_pause(&PauseType::Deposit, &true, &500u32);

// Configure a guardian (e.g., security multisig)
client.set_guardian(&security_multisig);

// Guardian (or admin) triggers emergency shutdown
client.set_emergency_state(&EmergencyState::Shutdown);

// Admin moves to controlled recovery so users can exit
client.set_emergency_state(&EmergencyState::Recovery);

// After all positions are resolved, return to normal
client.set_emergency_state(&EmergencyState::Normal);
```

## Security Notes: Operational Correctness

During an active incident, operators must follow these precedence rules to ensure predictable
protocol behavior:

1. **Predictable Halt**: If an unknown vulnerability is detected, activate `PauseType::All` or
   use `set_read_only(true)` immediately. These flags guarantee that NO operations can bypass
   the halt, even if other granular flags are later toggled by mistake.

2. **Deterministic Unpause**: To resume service, granular flags should be reviewed and set to
   `false` _before_ disabling the global `All` flag. This prevents an "accidental unpause" of a
   specific vulnerable path. To effectively extend a pause, simply re-issue `set_pause(..., true, new_ttl)`.

3. **Recovery Sequence**: Transitioning to `Recovery` mode is a one-way path to protocol unwind.
   Once in recovery, the protocol cannot return to `Normal` without resolving all outstanding
   liabilities or an admin `set_emergency_state(EmergencyState::Normal)` call. Granular pauses
   remain active in recovery to allow for "paused unwinds" if specific assets become volatile.

4. **Atomicity**: Pause checks are performed at the very beginning of every transaction. State
   reverts are atomic; a paused operation will never leave a partial state (e.g., tokens
   transferred but position not updated).
