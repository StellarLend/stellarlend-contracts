# Protocol Pause Mechanism

The StellarLend protocol includes a **granular pause mechanism** to ensure safety during emergency
situations or maintenance windows.

## Features

- **Granular Control**: Pause specific operations (`Deposit`, `Borrow`, `Repay`, `Withdraw`,
  `Liquidation`) without affecting others.
- **Global Pause**: A master switch (`All`) that immediately halts every operation.
- **Admin & Guardian Managed**: The admin or guardian can toggle individual pause flags.
- **Guardian Trigger**: A configured guardian (e.g., a security multisig) can trigger emergency
  shutdown without waiting for full governance latency.
- **Recovery Mode**: After a shutdown the admin can move the protocol into a controlled unwind mode
  so users can repay debt and withdraw collateral.
- **Event Driven**: Every pause state change emits a `PauseStateChangedEvent` for transparent off-chain
  monitoring.
- **Auto-Expiry**: Each pause switch carries an `expires_at_ledger` and automatically clears when
  ledger sequence progresses past its expiry.
- **Read-Only Mode**: A lightweight incident response switch that blocks all state-changing
  operations while keeping view functions available.

## Auto-Expiry Lifecycle

- Each granular pause is stored as a struct with `paused: bool` and `expires_at_ledger: u32`.
- A pause is considered active only while `env.ledger().sequence() < expires_at_ledger`.
- When ledger sequence exceeds `expires_at_ledger`, the paused operation is treated as unpaused
  without any storage rewrite.
- Operators should either explicitly extend an active pause with `extend_pause(operation, new_expiry)`
  or re-issue a new pause after expiry.

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
| `ReadOnly`    | Master switch that blocks ALL state changes (user and most admin).  |

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
| **ReadOnly**                         | **Yes**            | **BLOCKED**          | **Incident Freeze**: All state changes frozen for investigation.                                                                 |

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
Both `flash_loan` and `repay_flash_loan` are gated behind `check_pause_status` and
`check_emergency_status`, identical to the deposit/borrow/repay/withdraw entrypoints.

| Condition                       | `flash_loan` | `repay_flash_loan` |
| ------------------------------- | ------------ | ------------------ |
| Normal + No pause               | ✅ ALLOWED   | ✅ ALLOWED         |
| `PauseType::FlashLoan` active   | ❌ BLOCKED   | ❌ BLOCKED         |
| `PauseType::All` active         | ❌ BLOCKED   | ❌ BLOCKED         |
| `EmergencyState::Shutdown`      | ❌ BLOCKED   | ❌ BLOCKED         |
| `EmergencyState::Recovery`      | ❌ BLOCKED   | ❌ BLOCKED         |
| Pause expired                   | ✅ ALLOWED   | ✅ ALLOWED         |

> **Design decision**: `repay_flash_loan` is also gated because during an incident
> there should be no in-flight flash loans. If `flash_loan` is blocked, no new
> flash loan can start, so blocking repayment is safe and provides defense in depth.

### Security Considerations

- **Precedence Rules**: Emergency states and ReadOnly mode override granular pause flags
- **Atomic Operations**: Pause checks happen before any state changes
- **Event Transparency**: All pause changes emit events for off-chain monitoring
- **Role Separation**: Only admin can set granular pauses; guardian can trigger emergency shutdown

## Contract Interface

### Admin Functions

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

#### `extend_pause(admin: Address, operation: PauseType, new_expiry: u32) -> Result<(), LendingError>`

Extends an active pause state to a later ledger sequence.

- **Requires Authorization**: Yes (by `admin`).
- **Emits**: `PauseStateChangedEvent`.

#### `set_guardian(admin: Address, guardian: Address) -> Result<(), BorrowError>`

Sets or rotates the guardian authorized to trigger emergency shutdown.

- **Requires Authorization**: Yes (by `admin`).
- **Emits**: `guardian_set_event`.

#### `start_recovery(admin: Address) -> Result<(), BorrowError>`

Transitions the protocol from `Shutdown` to `Recovery`.

- **Requires Authorization**: Yes (by `admin`).
- **Precondition**: Emergency state must be `Shutdown`.
- **Emits**: `emergency_state_event`.

#### `complete_recovery(admin: Address) -> Result<(), BorrowError>`

Returns the protocol to `Normal` from any non-normal state.

- **Requires Authorization**: Yes (by `admin`).
- **Emits**: `emergency_state_event`.

### Guardian / Admin Emergency Function

#### `emergency_shutdown(caller: Address) -> Result<(), BorrowError>`

Transitions the protocol to `Shutdown`.

- **Requires Authorization**: Yes — caller must be the admin **or** the configured guardian.
- **Emits**: `emergency_state_event`.

#### `set_read_only(admin: Address, read_only: bool) -> Result<(), BorrowError>`

Toggles the protocol-level read-only mode.

- **Requires Authorization**: Yes (by `admin`).
- **Emits**: `read_only_event`.
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

Returns the current emergency lifecycle state:

| Value      | Meaning                                                                 |
| ---------- | ----------------------------------------------------------------------- |
| `Normal`   | Standard operation — all flags are honoured normally.                   |
| `Shutdown` | Hard stop — all high-risk operations blocked.                           |
| `Recovery` | Controlled unwind — `repay` and `withdraw` allowed; all others blocked. |
| `ReadOnly` | Incident Response — ALL state changes blocked; view functions only.     |

Note: `ReadOnly` is a separate flag and can be toggled in any state (`Normal`, `Shutdown`, `Recovery`).

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
Normal ──(emergency_shutdown)──► Shutdown ──(start_recovery)──► Recovery ──(complete_recovery)──► Normal
                                     └──────────────(complete_recovery, fast-exit)────────────────►
```

During **Recovery**, repay / withdraw remain available only when their granular pause flag and the
global `All` flag are inactive. Deposit, borrow, and liquidation remain blocked by emergency state.

## Events

| Event                      | Topic                     | Emitted by                                |
| -------------------------- | ------------------------- | ----------------------------------------- |
| `PauseStateChangedEvent`   | `pause_state_changed_event` | `set_pause`, `extend_pause`               |
| `GuardianSetEvent`         | `guardian_set_event`      | `set_guardian`                            |
| `EmergencyStateEvent`      | `emergency_state_event`   | `emergency_shutdown`, `start_recovery`, `complete_recovery` |

## Security Assumptions

1. **Admin Trust**: The admin should be a multisig or DAO-governed address to avoid single-key
   centralization risk. Compromise of the admin key allows arbitrary pause/unpause.

2. **Guardian Scope**: The guardian can trigger `emergency_shutdown` and call `set_pause`. It
   cannot rotate itself or invoke recovery — those paths require the admin key. Configure the
   guardian as a lower-latency security multisig.

3. **Persistence**: All pause and emergency states are stored in persistent storage so they survive
   ledger upgrades and contract updates.

4. **No Bypass**: Every operation entry point in `lib.rs` and the inner module implementations
   enforce pause and emergency checks independently (defense in depth). This includes
   `flash_loan` and `repay_flash_loan`, which are gated identically to
   deposit/borrow/repay/withdraw. There is no mutating path that skips both layers.

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

// Configure a guardian (e.g., security multisig)
client.set_guardian(&admin, &security_multisig);

// Guardian (or admin) triggers emergency shutdown
client.emergency_shutdown(&security_multisig);

// Admin moves to controlled recovery so users can exit
client.start_recovery(&admin);

// After all positions are resolved, return to normal
client.complete_recovery(&admin);
```

## Security Notes: Operational Correctness

During an active incident, operators must follow these precedence rules to ensure predictable
protocol behavior:

1. **Predictable Halt**: If an unknown vulnerability is detected, activate `PauseType::All` or
   `ReadOnly` mode immediately. These flags guarantee that NO operations can bypass the halt,
   even if other granular flags are later toggled by mistake.

2. **Deterministic Unpause**: To resume service, granular flags should be reviewed and set to
   `False` _before_ disabling the global `All` flag. This prevents an "accidental unpause" of a
   specific vulnerable path.

3. **Recovery Sequence**: Transitioning to `Recovery` mode is a one-way path to protocol unwind.
   Once in recovery, the protocol cannot return to `Normal` without resolving all outstanding
   liabilities or an admin `complete_recovery` call. Granular pauses remain active in recovery
   to allow for "paused unwinds" if specific assets become volatile.

4. **Atomicity**: Pause checks are performed at the very beginning of every transaction. State
   reverts are atomic; a paused operation will never leave a partial state (e.g., tokens
   transferred but position not updated).
