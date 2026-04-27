# Protocol Pause Mechanism

The StellarLend protocol includes a **granular pause mechanism** to ensure safety during emergency
situations or maintenance windows.

## Features

- **Granular Control**: Pause specific operations (`Deposit`, `Borrow`, `Repay`, `Withdraw`,
  `Liquidation`) without affecting others.
- **Global Pause**: A master switch (`All`) that immediately halts every operation.
- **Admin Managed**: Only the protocol admin can toggle individual pause flags.
- **Guardian Trigger**: A configured guardian (e.g., a security multisig) can trigger emergency
  shutdown without waiting for full governance latency.
- **Recovery Mode**: After a shutdown the admin can move the protocol into a controlled unwind mode
  so users can repay debt and withdraw collateral.
- **Event Driven**: Every pause state change emits a `pause_event` for transparent off-chain
  monitoring.

## Operation Types

| Enum Value    | Description                                                         |
| ------------- | ------------------------------------------------------------------- |
| `All`         | Global pause that supersedes all individual flags.                  |
| `Deposit`     | Prevents new collateral deposits (`deposit`, `deposit_collateral`). |
| `Borrow`      | Prevents new loan originations.                                     |
| `Repay`       | Prevents loan repayments (use with caution).                        |
| `Withdraw`    | Prevents collateral withdrawals.                                    |
| `Liquidation` | Prevents liquidations.                                              |

## Contract Interface

### Admin Functions

#### `set_pause(admin: Address, pause_type: PauseType, paused: bool) -> Result<(), BorrowError>`

Toggles the pause state for a specific operation or the entire protocol.

- **Requires Authorization**: Yes (by `admin`).
- **Emits**: `pause_event`.

#### `set_deposit_paused(paused: bool) -> Result<(), DepositError>`

Convenience wrapper for `set_pause(…, PauseType::Deposit, paused)`.

- **Requires Authorization**: Yes (admin derived from storage).
- **Emits**: `pause_event`.

#### `set_withdraw_paused(paused: bool) -> Result<(), WithdrawError>`

Convenience wrapper for `set_pause(…, PauseType::Withdraw, paused)`.

- **Requires Authorization**: Yes (admin derived from storage).
- **Emits**: `pause_event`.

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

Returns the current emergency lifecycle state:

| Value      | Meaning                                                               |
| ---------- | --------------------------------------------------------------------- |
| `Normal`   | Standard operation — all flags are honoured normally.                 |
| `Shutdown` | Hard stop — all high-risk operations blocked.                         |
| `Recovery` | Controlled unwind — `repay` and `withdraw` allowed; all others blocked. |

## Emergency Lifecycle

```
Normal ──(emergency_shutdown)──► Shutdown ──(start_recovery)──► Recovery ──(complete_recovery)──► Normal
                                     └──────────────(complete_recovery, fast-exit)────────────────►
```

During **Recovery**, the pause check for repay / withdraw explicitly allows these paths so users can
fully unwind positions. All other entry points remain blocked.

## Events

| Event                  | Topic                   | Emitted by                                              |
| ---------------------- | ----------------------- | ------------------------------------------------------- |
| `PauseEvent`           | `pause_event`           | `set_pause`, `set_deposit_paused`, `set_withdraw_paused` |
| `GuardianSetEvent`     | `guardian_set_event`    | `set_guardian`                                          |
| `EmergencyStateEvent`  | `emergency_state_event` | `emergency_shutdown`, `start_recovery`, `complete_recovery` |

## Security Assumptions

1. **Admin Trust**: The admin should be a multisig or DAO-governed address to avoid single-key
   centralization risk. Compromise of the admin key allows arbitrary pause/unpause.

2. **Guardian Scope**: The guardian can only trigger `emergency_shutdown`. It cannot set individual
   pause flags, rotate itself, or invoke recovery — those paths require the admin key. Configure the
   guardian as a lower-latency security multisig.

3. **Persistence**: All pause and emergency states are stored in persistent storage so they survive
   ledger upgrades and contract updates.

4. **No Bypass**: Every operation entry point in `lib.rs` and the inner module implementations
   enforce pause and emergency checks independently (defense in depth). There is no path that
   skips both layers.

5. **Global Overrides Local**: The `All` pause flag supersedes individual unpause flags. Setting
   `Deposit = false` while `All = true` still blocks deposit operations.

6. **Least-Risk Recovery**: During `Recovery`, only the unwind path (`repay`, `withdraw`) is
   available. Even in recovery, granular pause flags for `Repay` and `Withdraw` are still
   respected — the admin retains fine-grained control.

7. **Reentrancy**: Flash loan operations carry a dedicated reentrancy guard (separate from the
   pause mechanism). The pause check is performed before the guard is engaged.

## Usage Examples (Rust SDK)

```rust
// Pause borrowing in an emergency
client.set_pause(&admin, &PauseType::Borrow, &true);

// Re-enable borrowing
client.set_pause(&admin, &PauseType::Borrow, &false);

// Query pause state before presenting UI options
let borrow_paused = client.get_pause_state(&PauseType::Borrow);

// Configure a guardian (e.g., security multisig)
client.set_guardian(&admin, &security_multisig);

// Guardian (or admin) triggers emergency shutdown
client.emergency_shutdown(&security_multisig);

// Admin moves to controlled recovery so users can exit
client.start_recovery(&admin);

// After all positions are resolved, return to normal
client.complete_recovery(&admin);
```
