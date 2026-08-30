# Reserve Invariant Checking - Quick Reference

## The Core Rule

```rust
actual_balance == expected_balance
```

**If this fails, transaction PANICS and REVERTS.**

## For Every State-Changing Operation

### Pattern 1: Manual Checks
```rust
pub fn your_operation(e: Env, asset: Address, ...) {
    // 1. CHECK BEFORE
    invariants::check_invariant_before(&e, &asset);
    
    // 2. DO YOUR OPERATION
    // ... transfer tokens ...
    // ... update accounting ...
    
    // 3. CHECK AFTER
    invariants::check_invariant_after(&e, &asset);
}
```

### Pattern 2: Using Macro
```rust
pub fn your_operation(e: Env, asset: Address, ...) {
    with_invariant_check!(e, asset, {
        // ... transfer tokens ...
        // ... update accounting ...
    });
}
```

### Pattern 3: Multi-Asset (e.g., Liquidation)
```rust
pub fn liquidate(e: Env, debt_asset: Address, collateral_asset: Address, ...) {
    // Check BOTH assets before
    invariants::check_invariant_before(&e, &debt_asset);
    invariants::check_invariant_before(&e, &collateral_asset);
    
    // ... do liquidation ...
    
    // Check BOTH assets after
    invariants::check_invariant_after(&e, &debt_asset);
    invariants::check_invariant_after(&e, &collateral_asset);
}
```

## What Gets Checked

```rust
actual_balance = token_client.balance(&contract_address)

expected_balance = TotalDeposits(asset) 
                 + Treasury(asset) 
                 - BadDebt(asset)
```

## When to Update Accounting

### ✅ After Token IN (deposit, repay)
```rust
// Transfer tokens IN
token_client.transfer(&user, &contract, &amount);

// UPDATE accounting to match
let total = get_total_deposits(asset);
set_total_deposits(asset, total + amount);
```

### ✅ Before Token OUT (withdraw, borrow)
```rust
// UPDATE accounting first
let total = get_total_deposits(asset);
set_total_deposits(asset, total - amount);

// Transfer tokens OUT
token_client.transfer(&contract, &user, &amount);
```

### ✅ Treasury Fees
```rust
// After collecting fee
let treasury = get_treasury(asset);
set_treasury(asset, treasury + fee_amount);
```

### ✅ Bad Debt
```rust
// When recording bad debt
let bad_debt = get_bad_debt(asset);
set_bad_debt(asset, bad_debt + loss_amount);
```

## Flash Loan Exception

```rust
pub fn flash_loan(e: Env, asset: Address, amount: i128) {
    // Check BEFORE loan
    invariants::check_invariant_before(&e, &asset);
    
    // Set guard (skips checks during callback)
    e.storage().temporary().set(&DataKey::FlashActive, &true);
    
    // Transfer loan
    token_client.transfer(&contract, &receiver, &amount);
    
    // Callback happens here (invariant temporarily violated)
    
    // Receive repayment + fee
    token_client.transfer(&receiver, &contract, &amount + fee);
    update_treasury(asset, fee);
    
    // Clear guard
    e.storage().temporary().remove(&DataKey::FlashActive);
    
    // Check AFTER repayment
    invariants::check_invariant_after(&e, &asset);
}
```

## Reading Panic Messages

```
RESERVE INVARIANT VIOLATION [BEFORE]: 
  asset=CA3D5KRYM..., 
  actual_balance=10000, 
  expected_balance=9950, 
  drift=+50
```

| Field | Meaning |
|-------|---------|
| `[BEFORE]` | Drift existed before operation (pre-existing bug) |
| `[AFTER]` | Drift created by this operation (bug in this op) |
| `drift > 0` | Contract has MORE tokens than accounting says |
| `drift < 0` | Contract has FEWER tokens than accounting says |

## Common Mistakes

### ❌ Forgot to update accounting
```rust
// WRONG
token_client.transfer(&user, &contract, &amount);
// Missing: update TotalDeposits
```

### ❌ Wrong amount in accounting
```rust
// WRONG
token_client.transfer(&user, &contract, &amount);
set_total_deposits(asset, total + (amount - 1)); // Off by one!
```

### ❌ Arithmetic overflow
```rust
// WRONG
let new_total = total + amount; // Can overflow!

// RIGHT
let new_total = total.checked_add(amount).expect("Overflow");
```

### ❌ Forgot treasury accounting
```rust
// WRONG
let fee = amount / 100;
// Fee collected but not added to Treasury storage
```

### ❌ Forgot bad debt deduction
```rust
// WRONG
// Recorded bad_debt but forgot it reduces expected_balance
```

## Debugging Checklist

When you get an invariant violation:

- [ ] Check all `token_client.transfer()` calls
- [ ] Check all `TotalDeposits` updates
- [ ] Check all `Treasury` updates
- [ ] Check all `BadDebt` updates
- [ ] Verify arithmetic (no overflow/underflow)
- [ ] Verify amounts match exactly
- [ ] Check for missing `checked_add`/`checked_sub`
- [ ] Review `compute_expected_reserve()` for missing components

## Testing Your Operation

```rust
#[test]
fn test_your_operation_maintains_invariant() {
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Set up initial state with matching balances
    setup_token_balance(&env, &asset, 1000);
    set_total_deposits(&env, &asset, 1000);
    
    // Call your operation
    your_operation(&env, &asset, ...);
    
    // Invariant should still pass
    invariants::check_invariant_after(&env, &asset);
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_your_operation_detects_drift() {
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Set up mismatched state
    setup_token_balance(&env, &asset, 1000);
    set_total_deposits(&env, &asset, 900); // Intentional mismatch
    
    // Should panic
    your_operation(&env, &asset, ...);
}
```

## Quick Decision Tree

```
Do I need to add invariant checks?
│
├─ Does this function transfer tokens? 
│  └─ YES → Add checks
│  └─ NO → Continue
│
├─ Does this function update TotalDeposits/Treasury/BadDebt?
│  └─ YES → Add checks
│  └─ NO → Continue
│
├─ Does this function call another function that does the above?
│  └─ YES → That function should have checks, not this one
│  └─ NO → No checks needed
│
└─ Is this a pure view/query function?
   └─ YES → No checks needed
```

## Remember

1. **Every token transfer** must have matching accounting update
2. **Every accounting update** must reflect real token movement
3. **Invariant checks** catch any mismatch immediately
4. **Transaction reverts** completely on violation (safe)
5. **Flash loans** are special (guard exempts callback)
6. **Multi-asset ops** check all assets involved

## Need Help?

- Full docs: `docs/RESERVE_INVARIANT_CHECKING.md`
- Implementation details: `IMPLEMENTATION_SUMMARY.md`
- Examples: `invariant_example.rs`
- Tests: `invariant_integration_test.rs`
- Source: `invariants.rs`
