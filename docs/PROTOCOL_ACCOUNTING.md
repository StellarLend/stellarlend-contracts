# Protocol-Level Accounting & Aggregate Limits

## Overview

The StellarLend protocol maintains aggregate counters for total debt and total deposits across all users. These counters enable protocol-level risk management through configurable debt ceilings and deposit caps.

## Accounting Model

### Per-User State

Each user maintains:
- **Collateral Balance**: `DataKey::Collateral(user)` → i128
  - Incremented on `deposit()`
  - Decremented on `withdraw()`
  - Represents deposited assets available as collateral

- **Debt Position**: `DataKey::Debt(user)` → DebtPosition { principal, last_update }
  - Principal incremented on `borrow()`
  - Principal decremented on `repay()`
  - Interest accrues based on elapsed time and APR (5% default)
  - Last update timestamp tracks when interest was last settled

### Protocol-Level Aggregates

The protocol maintains two aggregate counters:

- **Total Debt**: `DataKey::TotalDebt` → i128
  - Sum of all user principal balances (excluding accrued but unsettled interest)
  - Incremented by borrow amount when `borrow()` succeeds
  - Decremented by repay amount when `repay()` succeeds
  - Used to enforce debt ceiling

- **Total Deposits**: `DataKey::TotalDeposits` → i128
  - Sum of all user collateral balances
  - Incremented by deposit amount when `deposit()` succeeds
  - Decremented by withdrawal amount when `withdraw()` succeeds
  - Used to enforce deposit cap

## Accounting Invariants

### Invariant 1: Total Debt Consistency
```
TotalDebt == sum(user.debt.principal for all users)
```

**Enforcement:**
- Incremented atomically with per-user principal increase in `borrow()`
- Decremented atomically with per-user principal decrease in `repay()`
- Checked arithmetic prevents overflow/underflow

**Why it matters:**
- Enables accurate debt ceiling enforcement
- Allows protocol to track aggregate exposure
- Supports risk monitoring and reporting

### Invariant 2: Total Deposits Consistency
```
TotalDeposits == sum(user.collateral for all users)
```

**Enforcement:**
- Incremented atomically with per-user collateral increase in `deposit()`
- Decremented atomically with per-user collateral decrease in `withdraw()`
- Checked arithmetic prevents overflow/underflow

**Why it matters:**
- Enables accurate deposit cap enforcement
- Allows protocol to track total collateral locked
- Supports liquidity management

### Invariant 3: Non-Negative Balances
```
For all users:
  user.collateral >= 0
  user.debt.principal >= 0
  TotalDebt >= 0
  TotalDeposits >= 0
```

**Enforcement:**
- `withdraw()` checks `amount <= current_collateral` before decrementing
- `repay()` uses checked subtraction; fails if amount > principal
- Aggregate totals only decremented after per-user checks pass

### Invariant 4: Ceiling & Cap Enforcement
```
TotalDebt <= DebtCeiling
TotalDeposits <= DepositCap
```

**Enforcement:**
- `borrow()` checks `TotalDebt + amount <= DebtCeiling` before updating
- `deposit()` checks `TotalDeposits + amount <= DepositCap` before updating
- Both checks use checked arithmetic to prevent overflow

## Operation Semantics

### Deposit

```rust
pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError>
```

**Preconditions:**
- Emergency state is Normal
- No active flash loan
- User authorizes transaction
- `amount > 0` (implicit via checked_add)

**Atomicity:**
1. Check `TotalDeposits + amount <= DepositCap`
2. Update `user.collateral += amount`
3. Update `TotalDeposits += amount`

**Postconditions:**
- `user.collateral` increased by `amount`
- `TotalDeposits` increased by `amount`
- Invariant 2 maintained

**Failure modes:**
- `DepositCapExceeded`: Would exceed deposit cap
- `Overflow`: Arithmetic overflow in checked_add

### Withdraw

```rust
pub fn withdraw(env: Env, user: Address, amount: i128) -> Result<i128, LendingError>
```

**Preconditions:**
- Emergency state is not Shutdown
- No active flash loan
- User authorizes transaction
- `amount <= user.collateral`

**Atomicity:**
1. Check `amount <= user.collateral`
2. Update `user.collateral -= amount`
3. Update `TotalDeposits -= amount`

**Postconditions:**
- `user.collateral` decreased by `amount`
- `TotalDeposits` decreased by `amount`
- Invariant 2 maintained

**Failure modes:**
- Insufficient collateral
- `Overflow`: Arithmetic underflow in checked_sub

### Borrow

```rust
pub fn borrow(env: Env, user: Address, amount: i128) -> Result<i128, LendingError>
```

**Preconditions:**
- Emergency state is Normal
- User authorizes transaction
- `amount >= MinBorrow`
- `amount > 0` (implicit via checked_add)

**Atomicity:**
1. Check `TotalDebt + amount <= DebtCeiling`
2. Settle accrual on `user.debt` (add interest to principal)
3. Update `user.debt.principal += amount`
4. Update `TotalDebt += amount`

**Postconditions:**
- `user.debt.principal` increased by `amount` (plus accrued interest)
- `TotalDebt` increased by `amount`
- Invariant 1 maintained

**Failure modes:**
- `DebtCeilingExceeded`: Would exceed debt ceiling
- `BelowMinimumBorrow`: Amount below minimum
- `Overflow`: Arithmetic overflow

### Repay

```rust
pub fn repay(env: Env, user: Address, amount: i128) -> Result<i128, LendingError>
```

**Preconditions:**
- Emergency state is not Shutdown
- No active flash loan
- User authorizes transaction
- `amount > 0`

**Atomicity:**
1. Settle accrual on `user.debt` (add interest to principal)
2. Update `user.debt.principal -= min(amount, current debt)`
3. Update `TotalDebt -= min(amount, current debt)`

**Postconditions:**
- `user.debt.principal` is decreased by the repaid amount, clamped at zero
- `TotalDebt` is decreased by the same clamped amount
- Remaining debt principal after repay is returned
- Invariant 1 maintained

**Failure modes:**
- `InvalidAmount`: `amount <= 0`
- `Overflow`: Arithmetic overflow during debt accrual

## Interest Accrual & Accounting

**Important:** Interest accrual does NOT automatically update aggregate totals.

- Interest is calculated on-demand when `borrow()` or `repay()` is called
- Interest is added to `user.debt.principal` before the operation
- The aggregate `TotalDebt` is updated by the operation amount, not the interest

**Example:**
```
User borrows 100 at 5% APR
- TotalDebt += 100
- user.debt.principal = 100

After 1 year with no operations:
- user.debt.principal (effective) = 105 (100 + 5 interest)
- TotalDebt = 100 (unchanged, interest not yet settled)

User repays 50:
- Interest settled: user.debt.principal = 105
- After repay: user.debt.principal = 55
- TotalDebt -= 50 (only the repay amount, not interest)
```

**Implication:** `TotalDebt` represents settled principal, not effective debt including accrued interest. This is intentional to avoid expensive aggregate interest calculations.

## Admin Functions

### Set Debt Ceiling

```rust
pub fn set_debt_ceiling(env: Env, ceiling: i128) -> Result<(), LendingError>
```

- Admin-only
- Must be > 0
- Takes effect immediately
- Does not retroactively enforce on existing debt

### Set Deposit Cap

```rust
pub fn set_deposit_cap(env: Env, cap: i128) -> Result<(), LendingError>
```

- Admin-only
- Must be > 0
- Takes effect immediately
- Does not retroactively enforce on existing deposits

### Query Functions

```rust
pub fn get_total_debt(env: Env) -> i128
pub fn get_total_deposits(env: Env) -> i128
pub fn get_debt_ceiling(env: Env) -> i128
pub fn get_deposit_cap(env: Env) -> i128
```

All are read-only and return current values.

## Testing & Verification

### Test Coverage

The test suite includes:

1. **Ceiling Enforcement Tests**
   - `test_borrow_blocked_at_debt_ceiling`: Verifies borrow fails when ceiling would be exceeded
   - `test_total_debt_tracking`: Verifies TotalDebt increments correctly
   - `test_repay_decrements_total_debt`: Verifies TotalDebt decrements on repay

2. **Cap Enforcement Tests**
   - `test_deposit_blocked_at_cap`: Verifies deposit fails when cap would be exceeded
   - `test_total_deposits_tracking`: Verifies TotalDeposits increments correctly
   - `test_withdraw_decrements_total_deposits`: Verifies TotalDeposits decrements on withdraw

3. **Invariant Tests**
   - `test_accounting_invariant_after_operations`: Multi-user scenario verifying totals
   - `test_multiple_users_respect_ceiling`: Ceiling enforcement across users
   - `test_multiple_users_respect_deposit_cap`: Cap enforcement across users

### Verification Checklist

- [ ] All tests pass with 95%+ coverage
- [ ] Debt ceiling prevents unbounded borrowing
- [ ] Deposit cap prevents unbounded deposits
- [ ] Aggregate totals remain consistent after all operations
- [ ] Overflow/underflow is prevented via checked arithmetic
- [ ] Admin can adjust ceilings/caps
- [ ] Interest accrual does not break accounting invariants

## Security Considerations

### Blast Radius Mitigation

The debt ceiling and deposit cap provide critical protection against:

1. **Oracle Failures**: If price oracle is compromised, limits prevent unbounded liquidation cascades
2. **Smart Contract Bugs**: Limits cap exposure even if a bug allows incorrect borrowing
3. **Market Shocks**: Limits prevent protocol from over-leveraging during volatility

### Recommended Defaults

- **Debt Ceiling**: Start conservative (e.g., 10M USD equivalent), increase as protocol matures
- **Deposit Cap**: Match debt ceiling or set higher to allow collateral accumulation
- **Monitoring**: Admin should track utilization and adjust caps as protocol scales

### Overflow Protection

All arithmetic uses Rust's checked operations:
- `checked_add()`: Returns error on overflow
- `checked_sub()`: Returns error on underflow
- Operations fail atomically; no partial state updates

## Future Enhancements

Potential improvements to the accounting model:

1. **Per-Asset Tracking**: Separate ceilings/caps per asset type
2. **Time-Weighted Averages**: Track average debt/deposits over time
3. **Interest Accrual Snapshots**: Periodically settle interest to update aggregates
4. **Liquidation Accounting**: Track liquidated amounts separately
5. **Reserve Accounting**: Separate protocol reserves from user deposits
