# State Machine Invariants for StellarLend Lending Protocol

## Overview

This document defines the explicit state machine invariants for all protocol operations, covering normal, adversarial, retry, and cancellation paths. Each operation's invariants specify pre-conditions, post-conditions, state transitions, and failure recovery strategies.

---

## Table of Contents

1. [Deposit Invariants](#deposit-invariants)
2. [Withdraw Invariants](#withdraw-invariants)
3. [Borrow Invariants](#borrow-invariants)
4. [Repay Invariants](#repay-invariants)
5. [Liquidate Invariants](#liquidate-invariants)
6. [Flash Loan Invariants](#flash-loan-invariants)
7. [Cross-Cutting Invariants](#cross-cutting-invariants)
8. [Failure Recovery Strategies](#failure-recovery-strategies)

---

## Deposit Invariants

### Operation Signature
```rust
pub fn deposit(
    env: Env, 
    user: Address, 
    amount: i128,
    operation_id: Option<BytesN<32>>,
    expected_sequence: Option<u64>,
) -> Result<i128, LendingError>
```

### Pre-Conditions (MUST hold before execution)

| ID | Invariant | Validation |
|----|-----------|------------|
| D.PC.1 | `amount > 0` | `if amount <= 0 { return Err(InvalidAmount) }` |
| D.PC.2 | Protocol initialized | `require_initialized(&env)?` |
| D.PC.3 | Protocol not paused for deposits | `check_pause_status(&env, ProtocolAction::Deposit)` |
| D.PC.4 | Protocol not in emergency shutdown | `check_emergency_status(&env, ProtocolAction::Deposit)` |
| D.PC.5 | No active flash loan | `require_no_active_flash_loan(&env)` (FlashActive == false) |
| D.PC.6 | User authorized | `user.require_auth()` |
| D.PC.7 | Sequence number valid (if provided) | `operation_tracker::validate_sequence(&env, &user, expected_sequence)?` |
| D.PC.8 | Operation not already completed | `operation_tracker::validate_operation_preconditions(&env, &user, operation_id.clone(), expected_sequence)?` |

### State Mutations (Atomic within transaction)

| Order | Key | Old Value | New Value | Formula |
|-------|-----|-----------|-----------|---------|
| 1 | `DataKey::Collateral(user)` | `current` | `new_balance` | `current + amount` |
| 2 | `DataKey::TotalDeposits` | `total` | `new_total` | `total + amount` |
| 3 | `OperationRecord(operation_id)` | `None` or `Pending` | `Completed(Deposit(new_balance))` | If operation_id provided |
| 4 | `UserOperationSequence(user)` | `seq` | `seq + 1` | Only on success |

### Post-Conditions (MUST hold after execution)

| ID | Invariant | Verification |
|----|-----------|--------------|
| D.POST.1 | `Collateral(user)_new = Collateral(user)_old + amount` | Balance increased exactly by amount |
| D.POST.2 | `TotalDeposits_new = TotalDeposits_old + amount` | Protocol total increased |
| D.POST.3 | `new_total <= deposit_cap` | Deposit cap not exceeded |
| D.POST.4 | `new_balance == Collateral(user)_new` | Return value matches stored state |
| D.POST.5 | Operation record status == `Completed` | If operation_id provided |
| D.POST.6 | User sequence incremented by 1 | `UserOperationSequence(user) == old_seq + 1` |
| D.POST.7 | `DepositEvent` emitted | Event log contains deposit record |
| D.POST.8 | Reserve invariant preserved | `actual_balance(asset) == expected_balance(asset)` (see RESERVE_INVARIANT_CHECKING.md) |

### Failure Paths

#### DepositCapExceeded

**Trigger:** `new_total > deposit_cap`

**State Impact:** No state mutations persist (atomic rollback)

**Recovery:** Wait for withdrawals to free capacity OR admin increases deposit_cap

#### DuplicateOperation

**Trigger:** `operation_id` already exists with status `Completed`

**State Impact:** None (early return)

**Recovery:** Return cached result from `OperationRecord.result`

**Idempotency:** ✅ **Safe** - same operation repeated returns same result without double-deposit

#### SequenceMismatch

**Trigger:** `expected_sequence != stored_sequence`

**State Impact:** None

**Recovery:** Client must query current sequence via `get_user_sequence()` and resubmit with correct value

### Retry Scenarios

#### Network Timeout (Uncertain Result)

**Client Perspective:** Transaction submitted, no response received

**Protocol State:** Transaction either succeeded (state mutated, sequence++), failed (no state change), or pending

**Recovery Strategy:**
1. Query `get_operation_record(operation_id)` if operation_id was provided
2. If `Completed`: Use cached result (idempotent)
3. If `Pending` or `Not Found`: Query `get_user_sequence()` 
4. If sequence incremented: Deposit succeeded
5. If sequence unchanged: Safe to retry with same or new operation_id

#### Explicit Failure (Error Returned)

**Protocol State:** All state rolled back atomically

**Recovery Strategy:** Fix error condition (e.g., increase deposit_cap), retry with same/new operation_id

### Adversarial Scenarios

#### Double Submission Attack

**Attack:** Submit same deposit twice rapidly

**Defense:**
- With `operation_id`: Second submission returns `OperationInProgress` or `OperationAlreadyCompleted`
- With `expected_sequence`: Second submission fails with `SequenceMismatch` (sequence already incremented)
- Without both: **VULNERABLE** - both deposits execute

**Mitigation:** Clients MUST use either operation_id OR expected_sequence

#### Replay Attack

**Attack:** Replay old valid transaction

**Defense:** Stellar/Soroban platform sequence numbers prevent replay at transaction level

**Protocol Layer:** Additional defense via operation_id TTL expiry (stale operation IDs rejected)

#### Frontrunning

**Attack:** Observe pending deposit, submit own deposit to hit cap first

**Impact:** Original deposit fails with `DepositCapExceeded`

**Mitigation:** Unavoidable in public mempool. User must retry.

---

## Withdraw Invariants

### Operation Signature
```rust
pub fn withdraw(
    env: Env,
    user: Address,
    amount: i128,
    operation_id: Option<BytesN<32>>,
    expected_sequence: Option<u64>,
) -> Result<i128, LendingError>
```

### Pre-Conditions

| ID | Invariant | Validation |
|----|-----------|------------|
| W.PC.1 | `amount > 0` | `if amount <= 0 { return Err(InvalidAmount) }` |
| W.PC.2 | Protocol initialized | `require_initialized(&env)?` |
| W.PC.3 | Protocol not paused for withdrawals | `check_pause_status(&env, ProtocolAction::Withdraw)` |
| W.PC.4 | No active flash loan | `require_no_active_flash_loan(&env)` |
| W.PC.5 | User authorized | `user.require_auth()` |
| W.PC.6 | `amount <= current_balance` | `if amount > current { return Err(InvalidAmount) }` |
| W.PC.7 | Sequence valid (if provided) | `validate_sequence(&env, &user, expected_sequence)?` |
| W.PC.8 | Operation not duplicate | `validate_operation_preconditions(...)` |
| W.PC.9 | **Single-asset:** No debt constraint OR health factor maintained | See below |
| W.PC.10 | **Cross-asset:** `health_factor_after >= HEALTH_FACTOR_SCALE` (10000 = 1.0) | Must remain solvent |

### State Mutations

| Order | Key | Old Value | New Value | Constraint |
|-------|-----|-----------|-----------|------------|
| 1 | `Collateral(user)` | `current` | `new_balance = current - amount` | `new_balance >= 0` |
| 2 | `TotalDeposits` | `total` | `new_total = total - amount` | `new_total >= 0` |
| 3 | `UserCollateralAssets(user)` | `list` | `list \ {asset}` | Only if `new_balance == 0` (cross-asset) |
| 4 | `OperationRecord(op_id)` | - | `Completed(Withdraw(new_balance))` | If provided |
| 5 | `UserOperationSequence(user)` | `seq` | `seq + 1` | On success |

### Post-Conditions

| ID | Invariant | Verification |
|----|-----------|--------------|
| W.POST.1 | `Collateral(user)_new = Collateral(user)_old - amount` | Balance reduced exactly |
| W.POST.2 | `TotalDeposits_new = TotalDeposits_old - amount` | Protocol total reduced |
| W.POST.3 | **Cross-asset:** `health_factor >= 1.0` | Position remains solvent |
| W.POST.4 | Return value == stored balance | `new_balance == Collateral(user)_new` |
| W.POST.5 | Sequence incremented | `UserOperationSequence(user)++` |
| W.POST.6 | `WithdrawEvent` emitted | Event logged |
| W.POST.7 | Reserve invariant preserved | `actual_balance == expected_balance` |

### Failure Paths

#### HealthFactorTooLow (Cross-Asset Only)

**Trigger:** After withdrawal, `(weighted_collateral / total_debt) < HEALTH_FACTOR_SCALE`

**State Impact:** **Explicit Rollback Required**

```rust
// Implementation must restore collateral and asset list:
save_collateral_asset(env, user, asset, current); // Restore old balance
if current > 0 {
    add_to_user_collateral_list(env, user, asset); // Restore list
}
return Err(LendingError::HealthFactorTooLow);
```

**Critical Invariant:** After rollback, `Collateral(user) == Collateral(user)_before_withdrawal`

**Recovery:** User must first repay debt to improve health factor, then retry withdrawal

#### Insufficient Balance

**Trigger:** `amount > current_balance`

**State Impact:** None (pre-mutation check)

**Recovery:** Reduce withdrawal amount or deposit more first

### Retry Scenarios

#### Post-Health-Check Interruption

**Scenario:** Health check passed, state written, but response lost

**Detection:**
1. Query `OperationRecord(operation_id)` → status == `Completed`
2. Return cached `result` (withdrawal succeeded)

**Idempotency:** ✅ Cached result prevents double-withdrawal

#### Pre-Health-Check Failure

**Scenario:** Collateral written, health check fails, rollback executed, but client uncertain

**Detection:**
1. Query `UserOperationSequence(user)` → if unchanged, withdrawal did not complete
2. Query `Collateral(user)` → if unchanged, safe to retry

**Recovery:** Fix health issue (repay debt) and retry

### Adversarial Scenarios

#### Race to Withdraw Before Liquidation

**Attack:** Borrower's health factor drops, attempts to withdraw before liquidator acts

**Defense:** Health factor check rejects withdrawal if `HF < 1.0`

**Outcome:** Withdrawal fails, liquidation proceeds

#### Concurrent Withdrawal + Liquidation

**Scenario:** Withdraw and liquidate transactions both submitted, race condition

**Outcome:**
- If withdraw executes first and passes health check: Succeeds, liquidation may fail with `PositionHealthy`
- If liquidation executes first: Collateral seized, withdrawal fails with `InvalidAmount` (insufficient balance)

**Atomicity:** Soroban's serialization ensures only one executes per ledger state

---

## Borrow Invariants

### Operation Signature
```rust
pub fn borrow(
    env: Env,
    user: Address,
    amount: i128,
    operation_id: Option<BytesN<32>>,
    expected_sequence: Option<u64>,
) -> Result<i128, LendingError>
```

### Pre-Conditions

| ID | Invariant | Validation |
|----|-----------|------------|
| B.PC.1 | `amount > 0` | Early check |
| B.PC.2 | `amount >= min_borrow` | Prevents dust borrows |
| B.PC.3 | Protocol initialized | `require_initialized(&env)?` |
| B.PC.4 | Not paused | `check_pause_status(&env, ProtocolAction::Borrow)` |
| B.PC.5 | No flash loan active | `require_no_active_flash_loan(&env)` |
| B.PC.6 | User authorized | `user.require_auth()` |
| B.PC.7 | **Cross-asset:** All collateral/debt prices fresh | `ensure_position_prices_fresh(&env, &user, &asset)?` (fail-closed) |
| B.PC.8 | **Single-asset:** Collateral price available | `get_collateral_price_internal(&env)?` |
| B.PC.9 | Sequence valid (if provided) | `validate_sequence(...)` |
| B.PC.10 | Operation not duplicate | `validate_operation_preconditions(...)` |

### State Mutations (Two-Phase for Cross-Asset)

#### Phase 1: Debt Position Update

| Order | Key | Old | New | Formula |
|-------|-----|-----|-----|---------|
| 1 | `BorrowIndex` | `index_old` | `index_new` | `accrue_index(index_old, elapsed, rate)` |
| 2 | `LastIndexUpdate` | `ts_old` | `now` | Current timestamp |
| 3 | `Debt(user)` or `DebtAsset(user, asset)` | `position_old` | `position_new` | `settle_position() + amount` |
| 4 | `UserDebtAssets(user)` | `list` | `list ∪ {asset}` | Add if new asset (cross-asset) |

#### Phase 2: Health Factor Validation (Cross-Asset)

**Computation:** 
```
weighted_collateral = Σ(collateral_i × price_i × ltv_i)
total_debt_value = Σ(debt_j × price_j)
health_factor = weighted_collateral / total_debt_value
```

**Validation:** `health_factor >= HEALTH_FACTOR_SCALE (10000)`

**On Failure:** **Explicit Rollback**

```rust
// Restore old debt position
save_debt_asset(env, user, asset, &DebtPosition {
    principal: prev_principal,
    borrow_index_snapshot: 0,
    last_update: now,
});

// Remove from debt list if was new position
if prev_principal == 0 {
    remove_from_user_debt_list(env, user, asset);
}

return Err(LendingError::HealthFactorTooLow);
```

#### Phase 3: Debt Ceiling Validation

**Check:** `new_total_debt <= asset_params.debt_ceiling`

**On Failure:** Same rollback as Phase 2

#### Phase 4: Finalization (If All Checks Pass)

| Order | Key | Old | New |
|-------|-----|-----|-----|
| 1 | `TotalDebtAsset(asset)` | `total_asset` | `total_asset + delta` |
| 2 | `TotalDebt` | `total_protocol` | `total_protocol + delta` |
| 3 | `IsolationDebt(collateral_asset)` | `iso_debt` | `iso_debt + delta` (if isolated) |
| 4 | `InsuranceFund` | `fund` | `fund + interest_share` (if interest accrued) |
| 5 | `OperationRecord(op_id)` | - | `Completed(Borrow(new_principal))` |
| 6 | `UserOperationSequence(user)` | `seq` | `seq + 1` |

### Post-Conditions

| ID | Invariant | Verification |
|----|-----------|--------------|
| B.POST.1 | `Debt(user).principal == prev_principal + amount` | Debt increased exactly |
| B.POST.2 | `Debt(user).borrow_index_snapshot == current_index` | Snapshot updated |
| B.POST.3 | `Debt(user).last_update == now` | Timestamp refreshed |
| B.POST.4 | `health_factor >= 1.0` | Solvency maintained |
| B.POST.5 | `TotalDebt increased by delta` | Protocol total updated |
| B.POST.6 | `new_total_debt <= debt_ceiling` | Ceiling respected |
| B.POST.7 | Sequence incremented | `UserOperationSequence++` |
| B.POST.8 | `BorrowEvent` emitted | Event logged |
| B.POST.9 | Reserve invariant preserved | `actual_balance == expected_balance` |

### Failure Paths

#### HealthFactorTooLow

**Trigger:** Post-borrow health factor < 1.0

**State Impact:** Debt position restored to old value, asset list cleaned

**Critical Invariant:** `Debt(user).principal == prev_principal` after rollback

**Recovery:** 
1. Deposit more collateral
2. Reduce borrow amount
3. Wait for collateral price increase

#### DebtCeilingExceeded

**Trigger:** `new_total_debt > asset_params.debt_ceiling`

**State Impact:** Same rollback as HealthFactorTooLow

**Recovery:** Wait for repayments OR admin increases ceiling

#### StaleOracleTimestamp (Cross-Asset)

**Trigger:** Any collateral/debt price older than `DEFAULT_ORACLE_MAX_AGE_SECS` (3600s = 1 hour)

**State Impact:** None (pre-mutation check)

**Fail-Closed Policy:** Reject borrow on ANY stale price to prevent under-collateralized loans

**Recovery:** Wait for oracle update, then retry

**Contrast:** Repay is **fail-open** (not gated by staleness) to always allow risk reduction

### Retry Scenarios

#### Health Factor Calculation Timeout

**Scenario:** Borrow submitted, health calc times out, client uncertain

**Detection:**
1. Query `OperationRecord(operation_id)`:
   - `Completed`: Borrow succeeded, use cached result
   - `Pending` or `Not Found`: Uncertain
2. Query `Debt(user).principal`:
   - If changed: Borrow executed
   - If unchanged: Safe to retry

**Idempotency:** With operation_id, cached result prevents double-borrow

#### Concurrent Borrow Attempts

**Scenario:** User submits two borrow requests simultaneously (network glitch)

**With operation_id:**
- Both use same ID → Second returns `OperationInProgress` or cached result
- Different IDs → Both attempt execution; second may fail health check

**With expected_sequence:**
- First increments sequence → Second fails with `SequenceMismatch`

**Without either:** **Both execute** (up to health limit)

### Adversarial Scenarios

#### Maximum Leverage Attack

**Attack:** Borrow maximum amount to push health factor to exactly 1.0

**Defense:** Health check requires `HF >= HEALTH_FACTOR_SCALE`

**Risk:** Even small price movement triggers liquidation

**Mitigation:** Protocol enforces minimum; users should maintain buffer

#### Oracle Manipulation

**Attack:** Manipulate oracle to report inflated collateral price, borrow excess

**Defense:**
1. Partial staleness check (fail-closed on ANY stale price)
2. Oracle signature validation
3. Price bounds (min/max sanity checks)

**Recovery:** Liquidation when true price reflected

#### Debt Ceiling Race

**Attack:** Multiple users race to borrow when ceiling nearly reached

**Outcome:** First to execute succeeds; others fail with `DebtCeilingExceeded`

**Fairness:** First-come-first-served (Stellar transaction ordering)

---

## Repay Invariants

### Operation Signature
```rust
pub fn repay(
    env: Env,
    user: Address,
    amount: i128,
    operation_id: Option<BytesN<32>>,
    expected_sequence: Option<u64>,
) -> Result<i128, LendingError>
```

### Pre-Conditions

| ID | Invariant | Validation |
|----|-----------|------------|
| R.PC.1 | `amount > 0` | Early check |
| R.PC.2 | Protocol initialized | `require_initialized(&env)?` |
| R.PC.3 | Not paused for repays | `check_pause_status(&env, ProtocolAction::Repay)` |
| R.PC.4 | **Allowed in Emergency Recovery mode** | Repay is fail-open for risk reduction |
| R.PC.5 | No flash loan active | `require_no_active_flash_loan(&env)` |
| R.PC.6 | User authorized | `user.require_auth()` |
| R.PC.7 | **Single-asset:** `amount <= effective_debt(position, now, rate)` | Prevent overpayment |
| R.PC.8 | **Cross-asset:** Silently clamp to outstanding balance | `clamped_amount = amount.min(settled_position.principal)` |
| R.PC.9 | Sequence valid (if provided) | `validate_sequence(...)` |
| R.PC.10 | Operation not duplicate | `validate_operation_preconditions(...)` |

### State Mutations

| Order | Key | Old | New | Formula |
|-------|-----|-----|-----|---------|
| 1 | `BorrowIndex` | `index_old` | `index_new` | Accrue interest |
| 2 | `LastIndexUpdate` | `ts_old` | `now` | Update timestamp |
| 3 | `Debt(user)` | `position_old` | `position_new` | `settle_position() - amount` |
| 4 | `TotalDebt` | `total_protocol` | `total_protocol - repaid` | Decrement |
| 5 | `TotalDebtAsset(asset)` | `total_asset` | `total_asset - repaid` | Cross-asset |
| 6 | `UserDebtAssets(user)` | `list` | `list \ {asset}` | If `new_principal == 0` |
| 7 | `IsolationDebt(collateral_asset)` | `iso_debt` | `iso_debt - repaid` | If isolated |
| 8 | `InsuranceFund` | `fund` | `fund + interest_share` | If interest accrued |
| 9 | `FirstUnhealthyTimestamp(user)` | `ts` | `None` | Clear if health restored |
| 10 | `OperationRecord(op_id)` | - | `Completed(Repay(new_principal))` | If provided |
| 11 | `UserOperationSequence(user)` | `seq` | `seq + 1` | On success |

### Post-Conditions

| ID | Invariant | Verification |
|----|-----------|--------------|
| R.POST.1 | `Debt(user).principal == prev_principal - repaid_amount` | Debt reduced |
| R.POST.2 | `repaid_amount <= prev_principal + accrued_interest` | No overpayment |
| R.POST.3 | `TotalDebt decreased by repaid_amount` | Protocol total updated |
| R.POST.4 | If `new_principal == 0`, removed from debt asset list | Clean up |
| R.POST.5 | Isolation debt decremented correctly | Ceiling freed |
| R.POST.6 | Sequence incremented | `UserOperationSequence++` |
| R.POST.7 | `RepayEvent` emitted | Event logged |
| R.POST.8 | Reserve invariant preserved | `actual_balance == expected_balance` |

### Semantic Differences: Single-Asset vs Cross-Asset

| Aspect | Single-Asset | Cross-Asset |
|--------|--------------|-------------|
| Overpayment | **Rejected** with `RepayAmountTooHigh` | **Silently clamped** to outstanding balance |
| Rationale | Exact balance tracking | User convenience (repay all with max amount) |
| Client Responsibility | Must query exact debt first | Can safely pass large amount |

### Failure Paths

#### RepayAmountTooHigh (Single-Asset Only)

**Trigger:** `amount > effective_debt(position, now, rate)`

**State Impact:** None (pre-mutation check)

**Recovery:** Query `get_debt_position()` or `compute_debt_view()`, resubmit with exact/lower amount

#### Zero Debt Edge Case (Cross-Asset)

**Scenario:** User attempts to repay but debt already zero (previous repayment succeeded but response lost)

**Behavior:** `clamped_amount == 0`, function returns early with `Ok(0)`

**State Impact:** None (early return, sequence NOT incremented)

**Idempotency:** ✅ Safe - repeated calls on zero debt are no-ops

### Retry Scenarios

#### Uncertain Repayment Status

**Detection Flow:**
1. Query `OperationRecord(operation_id)`:
   - `Completed`: Return cached `result` (debt fully repaid, idempotent)
   - `Not Found`: Continue to Step 2
2. Query `Debt(user).principal`:
   - If `== 0`: Repayment succeeded
   - If `unchanged`: Repayment did not execute, safe to retry
   - If `reduced but > 0`: Partial repayment succeeded (should not happen unless amount < total)

### Adversarial Scenarios

#### Repay-to-Unbind-Collateral Attack

**Attack:** Repay debt, immediately withdraw all collateral, then attempt to re-borrow before oracle updates

**Defense:**
1. Withdrawal health check prevents leaving insufficient collateral
2. Borrow requires fresh oracle prices
3. Sequence numbers prevent transaction reordering

**Outcome:** Attack fails at borrow step

#### Repay During Price Manipulation

**Scenario:** Oracle shows temporarily low debt asset price, user repays to fully clear debt for less value than owed

**Defense:** Protocol does not gate repay on oracle freshness (fail-open policy)

**Justification:** Reducing risk must always be allowed, even with stale data

---

## Liquidate Invariants

### Operation Signature
```rust
pub fn liquidate(
    env: Env,
    liquidator: Address,
    borrower: Address,
    debt_asset: Address,
    collateral_asset: Address,
    amount: i128,
    operation_id: Option<BytesN<32>>,
    expected_sequence: Option<u64>,
) -> Result<i128, LendingError>
```

### Pre-Conditions

| ID | Invariant | Validation |
|----|-----------|------------|
| L.PC.1 | `liquidator != borrower` | Prevent self-liquidation |
| L.PC.2 | Protocol initialized | `require_initialized(&env)?` |
| L.PC.3 | Not paused for liquidations | `check_pause_status(&env, ProtocolAction::Liquidate)` |
| L.PC.4 | **Allowed in Emergency modes** | Liquidation helps protocol recover |
| L.PC.5 | Liquidator authorized | `liquidator.require_auth()` |
| L.PC.6 | All oracle prices fresh | `require_fresh_valuation_prices(&env)?` |
| L.PC.7 | No flash loan active | `require_no_active_flash_loan(&env)` |
| L.PC.8 | Reentrancy lock acquired | `with_reentrancy_lock(&env, || ...)` |
| L.PC.9 | `borrower_health_factor < LIQUIDATION_THRESHOLD_BPS` (8000 = 80%) | Position must be underwater |
| L.PC.10 | `borrower_debt > 0` | Must have debt to liquidate |
| L.PC.11 | Grace period elapsed (if configured) | `now > FirstUnhealthyTimestamp + grace_period` |
| L.PC.12 | Sequence valid (if provided) | `validate_sequence(&env, &liquidator, expected_sequence)?` |
| L.PC.13 | Operation not duplicate | `validate_operation_preconditions(...)` |

### State Mutations (Atomic within reentrancy lock)

| Order | Key | Old | New | Formula |
|-------|-----|-----|-----|---------|
| 1 | `Debt(borrower)` | `position_old` | `position_settled` | Settle accrued interest |
| 2 | **(saved immediately)** | - | - | Interest settlement persisted |
| 3 | `Debt(borrower).principal` | `debt` | `debt - actual_repay` | Reduce debt |
| 4 | `Collateral(borrower)` | `collateral` | `collateral - seized` | Seize collateral |
| 5 | `BadDebt` | `bad_debt` | `bad_debt + shortfall` | If `seized < required` |
| 6 | `InsuranceFund` | `fund` | `fund - insurance_drawn` | Cover shortfall |
| 7 | `TotalDebt` | `total` | `total - actual_repay` | Decrement protocol total |
| 8 | `IsolationDebt(collateral_asset)` | `iso_debt` | `iso_debt - actual_repay` | Release ceiling |
| 9 | `FirstUnhealthyTimestamp(borrower)` | `ts` | `None` | Clear if health restored >= 1.0 |
| 10 | `OperationRecord(op_id)` | - | `Completed(Liquidate(actual_repay))` | If provided |
| 11 | `UserOperationSequence(liquidator)` | `seq` | `seq + 1` | On success |

**Token Transfers (After State Updates):**
```
debt_token.transfer(liquidator → contract, actual_repay)
collateral_token.transfer(contract → liquidator, seized)
```

### Post-Conditions

| ID | Invariant | Verification |
|----|-----------|--------------|
| L.POST.1 | `seized = min(actual_repay × (1 + incentive_bps) / 10000, available_collateral)` | Bonus calculated |
| L.POST.2 | `actual_repay <= debt × close_factor_bps / 10000` | Close factor cap respected |
| L.POST.3 | `Debt(borrower)_new = Debt(borrower)_old - actual_repay` | Debt reduced |
| L.POST.4 | `Collateral(borrower)_new = Collateral(borrower)_old - seized` | Collateral seized |
| L.POST.5 | If `seized < required`: `BadDebt += shortfall - insurance_drawn` | Shortfall recorded |
| L.POST.6 | Insurance drawn before bad debt recorded | `shortfall covered by insurance first` |
| L.POST.7 | `LiquidationEventV1` emitted | Event contains HF_before, repaid, seized, shortfall |
| L.POST.8 | Reentrancy lock released | `Symbol("reent_l")` cleared |
| L.POST.9 | Sequence incremented | `UserOperationSequence(liquidator)++` |
| L.POST.10 | Reserve invariant preserved (both assets) | `actual_balance == expected_balance` for debt_asset AND collateral_asset |

### Close Factor and Liquidation Incentive

**Close Factor:** Maximum share of debt repayable in single liquidation
- **Default:** 5000 bps = 50%
- **Max:** 7500 bps = 75%
- **Purpose:** Prevent full debt clearance in one transaction, give borrower opportunity to self-remedy

**Liquidation Incentive:** Bonus collateral given to liquidator
- **Default:** 1000 bps = 10%
- **Max:** 5000 bps = 50%
- **Purpose:** Incentivize liquidators while protecting borrowers from excessive seizure

**Formula:**
```
max_repay = debt × close_factor_bps / 10_000  // Default: 50% of debt
actual_repay = min(amount, max_repay)
seized = actual_repay × (10_000 + incentive_bps) / 10_000  // Default: 110% of repay value
```

### Failure Paths

#### PositionHealthy

**Trigger:** `health_factor >= LIQUIDATION_THRESHOLD_BPS` (80%)

**State Impact:** None (pre-mutation check, after interest settlement)

**Recovery:** Wait for health factor to drop below threshold

**Edge Case:** Between submission and execution, borrower repaid debt → health restored → liquidation fails

#### SelfLiquidation

**Trigger:** `liquidator == borrower`

**Rationale:** Prevent gaming liquidation incentive on own position

**State Impact:** None (pre-mutation check)

**Recovery:** Use different account as liquidator

#### InsufficientCollateral (Shortfall Handling)

**Trigger:** `seized_required > available_collateral`

**Behavior:** Partial liquidation with bad debt recording

**State Mutations:**
1. `seized = available_collateral` (seize all available)
2. `shortfall = seized_required - seized`
3. Draw from `InsuranceFund` up to available amount
4. Residual → `BadDebt += (shortfall - insurance_drawn)`

**Critical Invariant:** Transaction does not fail; protocol absorbs loss

### Retry Scenarios

#### Concurrent Liquidation Attempts

**Scenario:** Multiple liquidators submit liquidate for same borrower

**Outcome:**
- First liquidation executes, reduces debt
- If close_factor < 100%, borrower may still be underwater
- Second liquidation may succeed (partial liquidation)
- If borrower's HF restored, second liquidation fails with `PositionHealthy`

**Idempotency:** With operation_id, duplicate liquidations return cached result

#### Health Factor Races

**Scenario:** Borrower repays while liquidator submits liquidation

**Possible Outcomes:**
1. Repay executes first → Health restored → Liquidation fails
2. Liquidation executes first → Debt reduced → Repay succeeds on remaining debt
3. Both target same debt amount → One succeeds, one fails

**Atomicity:** Soroban ensures serialization; no partial state

### Adversarial Scenarios

#### Liquidation Sniping

**Attack:** Front-run borrower's repay transaction with liquidation

**Outcome:** Liquidation executes first if prioritized in mempool

**Defense:** None at protocol level (MEV inherent to public blockchains)

**Mitigation:** Borrowers should maintain health buffer

#### Griefing via Dust Liquidations

**Attack:** Liquidate tiny amounts repeatedly to annoy borrower

**Defense:**
1. Minimum borrow amount prevents dust debt
2. Close factor ensures meaningful liquidation size
3. Gas costs disincentivize dust liquidations

#### Bad Debt Accumulation Attack

**Attack:** Create many underwater positions with insufficient collateral

**Protocol Response:**
1. Liquidators seize all available collateral
2. Shortfall absorbed by insurance fund
3. Excess → bad debt (socialized loss)

**Recovery:** Governance calls `write_off_bad_debt()` to clear accumulated bad debt

---

## Flash Loan Invariants

### Operation Signature
```rust
pub fn flash_loan(
    env: Env,
    initiator: Address,
    receiver: Address,
    asset: Address,
    amount: i128,
    params: Bytes,
) -> (no operation_id support - atomically executes)
```

### Pre-Conditions

| ID | Invariant | Validation |
|----|-----------|------------|
| FL.PC.1 | `amount > 0` | Implicit validation |
| FL.PC.2 | Protocol initialized | `require_initialized(&env)?` |
| FL.PC.3 | Not paused for flash loans | `check_pause_status(&env, ProtocolAction::FlashLoan)` |
| FL.PC.4 | Not in emergency shutdown | `check_emergency_status(&env, ProtocolAction::FlashLoan)` |
| FL.PC.5 | `FlashActive == false` | No nested flash loans |
| FL.PC.6 | Initiator authorized | `initiator.require_auth()` |
| FL.PC.7 | `amount <= Treasury(asset)` | Sufficient liquidity |
| FL.PC.8 | `amount <= Treasury(asset) × max_flash_bps / 10000` | Utilization cap (default 100%) |

### State Transitions (Sequential within Single Transaction)

```
[START]
  ↓
[VALIDATE: Guards, amount, liquidity]
  ↓
[COMPUTE: fee = amount × fee_bps / 10000]
  ↓
[WRITE: Treasury(asset) -= amount]
[WRITE: Balance(receiver) += amount]
  ↓
[EMIT: FlashLoanEvent]
  ↓
[WRITE: FlashActive = TRUE] ← **CRITICAL GUARD POINT**
  ↓
[INVOKE: receiver.on_flash_loan(...)]
  │
  ├─ User executes arbitrary logic
  │  └─ MUST call repay_flash_loan:
  │       [READ/WRITE: Balance(payer) -= repay_amount]
  │       [READ/WRITE: Treasury(asset) += repay_amount]
  │       [EMIT: FlashLoanRepaidEvent]
  │
  ↓ (callback returns)
[WRITE: FlashActive = FALSE] ← **ALWAYS CLEARED**
  ↓
[VERIFY: Treasury(asset) >= original_balance + fee]
  ↓ (insufficient) → PANIC: InsufficientRepayment → **ENTIRE TX ROLLS BACK**
  ↓ (sufficient)
[SUCCESS]
```

### Post-Conditions

| ID | Invariant | Verification |
|----|-----------|--------------|
| FL.POST.1 | `Treasury(asset)_final >= Treasury(asset)_initial + fee` | Net profit to protocol |
| FL.POST.2 | `FlashActive == false` | Guard cleared |
| FL.POST.3 | `FlashLoanEvent` emitted before callback | Ordering preserved |
| FL.POST.4 | `FlashLoanRepaidEvent` emitted during callback | Repayment logged |
| FL.POST.5 | All state changes rolled back if callback panics | Soroban atomicity |
| FL.POST.6 | No sequence increment (flash loan is atomic, not user operation) | Sequence unchanged |

### Reentrancy Protection

#### FlashActive Guard Semantics

**Purpose:** Prevent nested flash loans within same transaction

**Storage:** Instance storage (transaction-scoped)

**Lifecycle:**
```rust
// Entry
require_no_active_flash_loan(&env); // Check == false
env.storage().instance().set(&DataKey::FlashActive, &true);

// Callback
// → If callback calls deposit/borrow/repay/flash_loan:
//   → require_no_active_flash_loan() panics: "FlashLoanReentrancy"

// Exit (always executed, even on panic via Soroban rollback)
env.storage().instance().set(&DataKey::FlashActive, &false);
```

**Protected Operations:**
- `deposit`, `withdraw`, `borrow`, `repay`, `liquidate`, `flash_loan`

**Critical Property:** Cannot be "stuck true" due to Soroban's automatic rollback on panic

### Failure Paths

#### FlashLoanReentrancy

**Trigger:** Callback attempts to call deposit/borrow/repay/liquidate/flash_loan

**State Impact:** Panic → entire transaction rolls back, FlashActive restored to false

**Defense:** Prevents:
1. Nested flash loans (flash loan funding another flash loan)
2. Flash loan funding own borrow (circular credit)
3. State manipulation during flash loan execution

#### InsufficientRepayment

**Trigger:** `Treasury(asset)_final < Treasury(asset)_initial + fee`

**State Impact:** Panic → entire transaction rolls back

**Causes:**
1. User forgot to call `repay_flash_loan`
2. User repaid less than `amount + fee`
3. Bug in repayment logic

**Critical Invariant:** Protocol never loses funds from flash loan

#### InsufficientLiquidity

**Trigger:** `amount > Treasury(asset)`

**State Impact:** None (pre-mutation check)

**Recovery:** Wait for deposits to increase treasury OR reduce flash loan amount

### Adversarial Scenarios

#### Circular Flash Loan Attack

**Attack:** Use flash loan to fund borrow, use borrowed funds to repay flash loan

**Defense:** `FlashActive` guard prevents borrow during flash loan callback

**Outcome:** Borrow call panics with "FlashLoanReentrancy"

#### Flash Loan Price Manipulation

**Attack:** 
1. Flash loan large amount
2. Manipulate external DEX prices
3. Borrow at manipulated prices
4. Repay flash loan

**Defense:**
1. All operations during flash loan blocked by `FlashActive` guard
2. Oracle uses time-weighted average or signed prices
3. Price bounds (min/max sanity checks)

**Outcome:** Attack fails at step 3 (borrow blocked)

#### Callback Reentrancy

**Attack:** Callback invokes liquidate on another user during flash loan

**Defense:** `require_no_active_flash_loan` in liquidate checks `FlashActive`

**Outcome:** Liquidate call panics

### Callback Safety Requirements

User-implemented `on_flash_loan` callback MUST:

1. ✅ Call `repay_flash_loan(env, payer, asset, amount + fee)`
2. ✅ Ensure payer has sufficient `Balance(asset, payer)` or tokens approved
3. ❌ NOT call any state-mutating protocol functions (deposit/borrow/repay/liquidate/flash_loan)
4. ❌ NOT manipulate oracle prices
5. ❌ NOT exceed gas limits

**Failure to comply:** Transaction panics and rolls back

---

## Cross-Cutting Invariants

### Global System Invariants (MUST hold at all times)

| ID | Invariant | Description |
|----|-----------|-------------|
| G.INV.1 | **Reserve Matching** | For each asset: `token_client.balance(&contract) == Σ(Collateral(user_i)) + Treasury + protocol_reserves` |
| G.INV.2 | **Debt Consistency** | `TotalDebt == Σ(Debt(user_i).principal)` (excluding accrued interest) |
| G.INV.3 | **Deposit Consistency** | `TotalDeposits == Σ(Collateral(user_i))` |
| G.INV.4 | **Isolation Debt Tracking** | For isolated assets: `IsolationDebt(asset) == Σ(Debt(user_i) where collateral includes asset)` |
| G.INV.5 | **Sequence Monotonicity** | `UserOperationSequence(user)` only increases, never decreases |
| G.INV.6 | **Operation Record Consistency** | If `OperationRecord(op_id).status == Completed`, result is immutable |
| G.INV.7 | **Flash Loan Atomicity** | `FlashActive` is never true at transaction boundary (start/end) |
| G.INV.8 | **Health Factor Lower Bound** | For all users with debt: `health_factor >= 0` (negative health impossible) |
| G.INV.9 | **Borrow Index Monotonicity** | `BorrowIndex` only increases (or stays same), never decreases |
| G.INV.10 | **Bad Debt Non-Negative** | `BadDebt >= 0` at all times |

### Operation Atomicity Guarantees

All operations are **atomic at Soroban transaction level**:

```
Transaction {
  [Pre-Conditions Checked]
  ↓
  [State Mutations Applied]
  ↓
  [Post-Conditions Validated]
  ↓ (any failure) → [ROLLBACK: All state restored to pre-transaction]
  ↓ (success) → [COMMIT: All state persisted, events emitted]
}
```

**Critical Property:** No partial state changes observable between transactions

### Sequence Number Ordering Guarantees

**Invariant:** Operations with sequence `n` can only execute after operations with sequence `< n` have completed

**Enforcement:**
```rust
// Each operation checks:
let current_seq = get_user_sequence(&env, &user);
if expected_sequence != current_seq {
    return Err(OperationTrackerError::SequenceMismatch);
}

// On success, increment:
increment_user_sequence(&env, &user); // current_seq + 1
```

**Consequence:** Client can enforce strict ordering by providing `expected_sequence` on each call

**Trade-off:** Flexibility vs Safety
- Provide sequence → Strict ordering enforced
- Omit sequence → Operations can execute in any order (parallel submissions possible)

### Operation ID Deduplication Guarantees

**Invariant:** Operation ID can only execute once within TTL window

**Enforcement:**
```rust
if let Some(record) = get_operation_record(&env, &operation_id) {
    match record.status {
        OperationStatus::Completed => {
            return Ok(cached_result); // Idempotent return
        }
        OperationStatus::Pending | OperationStatus::Executing => {
            return Err(OperationInProgress);
        }
        OperationStatus::Failed | OperationStatus::Cancelled => {
            // Allow retry
        }
    }
}
```

**TTL Behavior:**
- Operation records expire after `OPERATION_RECORD_TTL` ledgers (~30 days)
- After expiry, same operation_id can be reused
- Sequence number provides longer-term ordering guarantee

---

## Failure Recovery Strategies

### Strategy 1: Operation Status Query

**Use Case:** Transaction submitted, response lost/delayed

**Recovery Flow:**
```
1. Query get_operation_record(operation_id)
   ├─ Status::Completed
   │  └─ Return cached result (operation succeeded, idempotent)
   ├─ Status::Pending
   │  └─ Operation registered but not executed
   │      → Query sequence number or balance to confirm
   ├─ Status::Failed
   │  └─ Safe to retry with same or new operation_id
   └─ Not Found
      └─ Operation never registered OR expired
          → Query state (balance/debt) to confirm
```

### Strategy 2: Sequence Number Reconciliation

**Use Case:** Uncertain which operations completed

**Recovery Flow:**
```
1. Query current_sequence = get_user_sequence(user)
2. Compare with locally tracked sequence
   ├─ current == local
   │  └─ All submitted operations completed
   ├─ current > local
   │  └─ current - local operations succeeded without confirmation
   └─ current < local
      └─ IMPOSSIBLE (sequence only increases)
```

### Strategy 3: State Inspection

**Use Case:** No operation_id was provided, need to verify execution

**Recovery Flow:**
```
1. For Deposit/Withdraw:
   current_balance = get_collateral(user)
   Compare with expected_balance_before_operation
   
2. For Borrow/Repay:
   current_debt = get_debt_position(user).principal
   Compare with expected_debt_before_operation
   
3. For Liquidate:
   Check LiquidationEventV1 in event logs for matching parameters
```

### Strategy 4: Failed Operation Retry

**Retry Decision Matrix:**

| Original Error | Safe to Retry | Conditions |
|----------------|---------------|------------|
| `DepositCapExceeded` | ✅ Yes | After capacity frees OR cap increased |
| `HealthFactorTooLow` | ✅ Yes | After improving health (deposit/repay) |
| `DebtCeilingExceeded` | ✅ Yes | After repayments OR ceiling increased |
| `SequenceMismatch` | ✅ Yes | With corrected sequence number |
| `OperationAlreadyCompleted` | ⚠️ Return cached result | Idempotent - no retry needed |
| `InvalidAmount` | ❌ No | Fix parameters first |
| `SelfLiquidation` | ❌ No | Use different liquidator |
| `StaleOracleTimestamp` | ✅ Yes | After oracle update |

### Strategy 5: Cancelled Operation Recovery

**Use Case:** Registered operation but want to cancel before execution

**Recovery Flow:**
```
1. Call cancel_operation(operation_id, user)
   └─ Sets status to Cancelled
   
2. Submit new operation with different operation_id
   └─ Cancelled operations don't block new submissions
```

**Limitation:** Can only cancel `Pending` operations, not `Executing` or `Completed`

### Strategy 6: Two-Phase Commit (Advanced)

**Use Case:** Critical operations requiring explicit confirmation

**Pattern:**
```
Phase 1: Register Intent
  operation_id = generate_operation_id(...)
  register_operation(&env, &operation_id, &user, ttl_seconds)
  → Returns Pending
  
Phase 2: Execute
  result = deposit/borrow/repay(env, user, amount, Some(operation_id), ...)
  → Transitions Pending → Executing → Completed
  
Cancel (Optional):
  cancel_operation(&env, &operation_id, &user)
  → Transitions Pending → Cancelled
```

**Benefit:** User can review operation parameters before execution

**Caveat:** Not enforced by protocol - purely client-side pattern

---

## Testing Strategy for Invariants

### Unit Tests (Per Operation)

**Coverage:**
- ✅ All pre-conditions validated
- ✅ State mutations applied correctly
- ✅ Post-conditions verified
- ✅ Each failure path tested
- ✅ Rollback correctness verified

**Example Test Structure:**
```rust
#[test]
fn test_borrow_health_factor_rollback() {
    // Setup: User with collateral, attempt under-collateralized borrow
    // Expected: Borrow fails, debt position unchanged, sequence unchanged
    // Verify: Debt == debt_before, UserDebtAssets unchanged
}
```

### Integration Tests (Operation Sequences)

**Scenarios:**
- Deposit → Borrow → Repay → Withdraw (happy path)
- Borrow (fail) → Deposit → Borrow (succeed)
- Concurrent borrow attempts (with/without sequence)
- Liquidation during active borrow
- Flash loan callback reentrancy attempts

### Property-Based Tests (Invariant Preservation)

**Properties to Test:**
```rust
// For any sequence of valid operations:
// 1. TotalDeposits == Σ(Collateral(user_i))
// 2. TotalDebt <= DebtCeiling
// 3. FlashActive == false at transaction boundaries
// 4. OperationSequence(user) is monotonic
// 5. Reserve invariant preserved
```

### Adversarial Tests (Attack Scenarios)

**Attacks to Simulate:**
- Double submission (with/without operation_id)
- Sequence number manipulation
- Replay attacks (simulate transaction resubmission)
- Reentrancy attempts
- Race conditions (liquidation vs repay)

---

## Migration and Compatibility

### Adding Operation Tracking to Existing Deployments

**Challenge:** Existing users have no `UserOperationSequence` entry

**Solution:** Initialize to 0 on first operation

```rust
pub fn get_user_sequence(env: &Env, user: &Address) -> u64 {
    let key = OperationTrackerKey::UserSequence(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(0u64) // Default: 0 for new/existing users
}
```

**Backward Compatibility:**
- `operation_id` and `expected_sequence` are **optional parameters**
- Existing clients (not providing these) continue to work
- New clients can opt-in to stricter guarantees

### Event Schema Versioning

**All events include `schema_version` field:**
```rust
pub struct DepositEvent {
    pub schema_version: u32, // Current: 1
    pub user: Address,
    pub amount: i128,
    pub new_balance: i128,
    pub timestamp: u64,
}
```

**Indexer Compatibility:**
- Indexers check `schema_version` field
- Version 1 → Parse as DepositEvent
- Future versions → Parse accordingly or skip

**Migration Path:**
- Increment `EVENT_SCHEMA_VERSION` constant
- Add new event structs with higher version
- Maintain old struct definitions for backward compatibility
- Indexers handle both versions during transition period

---

## Summary

This document defines **production-quality state machine invariants** for the StellarLend lending protocol, covering:

✅ **Explicit pre-conditions and post-conditions** for all operations  
✅ **Detailed state transition flows** with atomic guarantees  
✅ **Failure paths and rollback correctness** verification  
✅ **Retry safety and idempotency** semantics  
✅ **Adversarial scenario defenses** and attack surface analysis  
✅ **Recovery strategies** for interrupted/uncertain operations  
✅ **Cross-cutting invariants** (reserve matching, debt consistency, sequence monotonicity)  
✅ **Operation tracking** (sequence numbers, operation IDs, TTL-based deduplication)  

**Key Guarantees:**

1. **Atomicity**: All operations are atomic at Soroban transaction level
2. **Idempotency**: Operations with `operation_id` can return cached results
3. **Ordering**: Operations with `expected_sequence` enforce strict ordering
4. **Reentrancy Protection**: Flash loan guard prevents nested operations
5. **Reserve Invariant**: Token balances always match accounting state
6. **Failure Recovery**: All failures are detectable and recoverable

**Client Responsibilities:**

- Use `operation_id` for idempotency OR `expected_sequence` for ordering
- Handle `SequenceMismatch` and `OperationAlreadyCompleted` errors
- Query operation status on uncertain results
- Maintain local operation state for recovery

**Testing Requirements:**

- Unit tests for each invariant
- Integration tests for operation sequences
- Property-based tests for invariant preservation
- Adversarial tests for attack scenarios

See `stellar-lend/contracts/lending/src/operation_tracker.rs` for implementation.
