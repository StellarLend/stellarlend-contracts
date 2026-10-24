# Invariant Checking Implementation

## Quick Start

### Basic Usage

Every state-changing operation is wrapped with invariant checks:

```rust
pub fn deposit(env: Env, user: Address, amount: i128, asset: Address) -> Result<i128, LendingError> {
    // Check invariant BEFORE state change
    invariants::check_invariant_before(&env, &asset);
    
    // ... perform deposit logic ...
    
    // Check invariant AFTER state change
    invariants::check_invariant_after(&env, &asset);
    
    Ok(new_balance)
}
```

### Using the Macro

For cleaner code, use the macro wrapper:

```rust
use crate::with_invariant_check;

pub fn some_operation(env: Env, asset: Address) -> Result<i128, Error> {
    with_invariant_check!(env, asset, {
        // ... state-changing logic ...
        Ok(result)
    })
}
```

## Operations with Invariant Checks

### ✅ Implemented

- [x] `deposit(user, amount, asset)` - Single asset check
- [x] `withdraw(user, amount, asset)` - Single asset check
- [x] `borrow(user, amount, asset)` - Single asset check
- [x] `repay(user, amount, asset)` - Single asset check
- [x] `borrow_against_collateral(user, amount, collateral_asset)` - Collateral asset check
- [x] `repay_against_collateral(user, amount, collateral_asset)` - Collateral asset check
- [x] `liquidate(liquidator, borrower, debt_asset, collateral_asset, amount)` - Both assets checked

### ⚠️ Excluded (By Design)

- `flash_loan` - Temporarily bypassed during callback (balance restored on repayment)
- View functions - Read-only, no state changes
- Admin setters - No token transfers

## Invariant Formula

```
actual_balance = token_client.balance(&contract_address)

expected_balance = total_deposits 
                 - bad_debt

// Accounting components:
// - total_deposits: DataKey::TotalDeposits
// - bad_debt: DataKey::BadDebt
```

## Testing

### Run Invariant Tests

```bash
# Unit tests
cargo test invariants::tests

# Integration tests  
cargo test invariant_integration_test

# All invariant-related tests
cargo test invariant
```

### Test Coverage

- ✅ Invariant passes with balanced state
- ✅ Invariant panics on drift detection
- ✅ All operations have before/after checks
- ✅ Multi-asset operations check all assets
- ✅ Bad debt correctly reduces expected balance

## Debugging

### Reading Panic Messages

When an invariant violation occurs:

```
thread 'test_name' panicked at 'RESERVE INVARIANT VIOLATION [AFTER]: 
  asset=GBXYZ..., 
  actual_balance=9950, 
  expected_balance=10000, 
  drift=-50'
```

**Interpretation:**
- `[AFTER]` - Violation detected after operation completed
- `drift=-50` - Actual balance is 50 stroops less than expected
- **Root cause**: Operation failed to update accounting or transferred too many tokens

### Common Causes

1. **Missing accounting update**
   ```rust
   // ❌ BAD: Transferred tokens but forgot to update TotalDeposits
   token_client.transfer(&user, &contract, &amount);
   // Missing: env.storage().set(&DataKey::TotalDeposits, &new_total);
   ```

2. **Rounding error accumulation**
   ```rust
   // ❌ BAD: Repeated division causes drift
   let fee = amount / 10000;  // Truncates
   let net = amount - fee;    // Drift accumulates
   ```

3. **Incorrect bad debt tracking**
   ```rust
   // ❌ BAD: Bad debt increased but not reflected in accounting
   env.storage().set(&DataKey::BadDebt, &new_bad_debt);
   // Missing: Expected balance computation should subtract bad_debt
   ```

## Disabling Invariants (Production)

If gas costs are prohibitive, use feature flags:

### 1. Add to Cargo.toml

```toml
[features]
default = ["invariant-checks"]
invariant-checks = []
```

### 2. Conditional Compilation

```rust
#[cfg(feature = "invariant-checks")]
invariants::check_invariant_before(&env, &asset);

// ... operation ...

#[cfg(feature = "invariant-checks")]
invariants::check_invariant_after(&env, &asset);
```

### 3. Build Without Checks

```bash
cargo build --release --no-default-features
```

## Performance Impact

### Gas Cost Estimates

| Operation | Base Cost | With Invariants | Overhead |
|-----------|-----------|-----------------|----------|
| deposit   | 50k gas   | 60k gas         | +20%     |
| withdraw  | 45k gas   | 55k gas         | +22%     |
| borrow    | 70k gas   | 80k gas         | +14%     |
| repay     | 65k gas   | 75k gas         | +15%     |
| liquidate | 100k gas  | 120k gas        | +20%     |

**Note:** Actual costs depend on Stellar network parameters and contract complexity.

### Optimization Strategies

1. **Sample Checking** (Production)
   ```rust
   if env.ledger().sequence() % 100 == 0 {
       invariants::check_invariant_after(&env, &asset);
   }
   ```

2. **Critical Operations Only**
   ```rust
   // Always check high-risk operations
   match operation_type {
       ProtocolAction::Liquidate | ProtocolAction::FlashLoan => {
           invariants::check_invariant_after(&env, &asset);
       }
       _ => {} // Skip checks for low-risk operations
   }
   ```

## Contributing

### Adding Invariant Checks to New Operations

1. **Identify state-changing operations**
   - Any function that modifies `TotalDeposits`, `Collateral`, `Debt`, etc.
   - Any function that transfers tokens

2. **Add checks at boundaries**
   ```rust
   pub fn new_operation(env: Env, asset: Address) -> Result<i128, Error> {
       invariants::check_invariant_before(&env, &asset);
       
       // ... operation logic ...
       
       invariants::check_invariant_after(&env, &asset);
       Ok(result)
   }
   ```

3. **Write tests**
   ```rust
   #[test]
   fn test_new_operation_maintains_invariant() {
       // Setup
       // Execute operation
       // Verify no panic
   }
   ```

4. **Update documentation**
   - Add to "Operations with Invariant Checks" section
   - Update RESERVE_INVARIANT_CHECKING.md

### Extending Expected Balance Computation

To add new accounting components:

```rust
fn compute_expected_reserve(env: &Env, asset: &Address) -> i128 {
    let mut expected: i128 = 0;
    
    // Existing components
    let total_deposits = env.storage().get(&DataKey::TotalDeposits).unwrap_or(0);
    expected += total_deposits;
    
    // Add new component
    let new_component = env.storage().get(&DataKey::NewComponent).unwrap_or(0);
    expected += new_component;
    
    // Subtract liabilities
    let bad_debt = env.storage().get(&DataKey::BadDebt).unwrap_or(0);
    expected -= bad_debt;
    
    expected
}
```

## FAQ

**Q: Why do we check before AND after?**
A: Before-check ensures we start from a consistent state. After-check ensures the operation maintained consistency.

**Q: What happens if the invariant fails?**
A: The transaction panics and reverts completely. No state changes persist.

**Q: Can we recover from an invariant violation?**
A: No. Violations indicate critical bugs that must be fixed in the contract logic.

**Q: Why aren't flash loans checked during the callback?**
A: The balance temporarily changes during the loan by design. The invariant is restored upon repayment.

**Q: How do I add invariant checks to my custom operation?**
A: See "Contributing" section above. Basic pattern: check before, execute, check after.

**Q: What if expected balance computation is wrong?**
A: The invariant will false-positive. Review `compute_expected_reserve` logic and add missing components.

## See Also

- [RESERVE_INVARIANT_CHECKING.md](../../../docs/RESERVE_INVARIANT_CHECKING.md) - Detailed design doc
- [RESERVE_ACCOUNTING.md](../../../docs/RESERVE_ACCOUNTING.md) - Reserve factor accounting
- [PROTOCOL_ACCOUNTING.md](../../../docs/PROTOCOL_ACCOUNTING.md) - Overall accounting design
- [src/invariants.rs](src/invariants.rs) - Implementation code
- [src/invariant_integration_test.rs](src/invariant_integration_test.rs) - Integration tests
