# Reserve Invariant Checking

## Overview

The reserve invariant system ensures that token reserves held by the contract **exactly match** the internal balance ledger accounting state at all times. This critical safety mechanism detects and prevents balance drift that could result from:

- Accounting bugs
- Reentrancy attacks
- External token manipulation
- Rounding errors that accumulate
- Logic errors in state transitions

## Core Invariant

```
actual_balance == expected_balance
```

Where:
- `actual_balance = token_client.balance(&env.current_contract_address())`
- `expected_balance = compute_expected_reserve(&env, &asset)`

The expected balance is computed from internal accounting ledgers:

```rust
expected_balance = total_deposits 
                 + cross_asset_collateral 
                 + protocol_reserves
                 - bad_debt
```

## Implementation

### Module Location

`contracts/lending/src/invariants.rs`

### Public API

```rust
pub fn check_invariant_before(env: &Env, asset: &Address)
pub fn check_invariant_after(env: &Env, asset: &Address)
pub fn compute_expected_reserve(env: &Env, asset: &Address) -> i128
```

### Macro Helper

```rust
with_invariant_check!(env, asset, {
    // ... state-changing operation ...
});
```

## Integration Points

The invariant is checked at the **start and end** of every state-changing operation:

### Single-Asset Operations

| Operation | Invariant Checks |
|-----------|------------------|
| `deposit(user, amount, asset)` | Before & After |
| `withdraw(user, amount, asset)` | Before & After |
| `borrow(user, amount, asset)` | Before & After |
| `repay(user, amount, asset)` | Before & After |

### Cross-Asset Operations

| Operation | Invariant Checks |
|-----------|------------------|
| `borrow_against_collateral(user, amount, collateral_asset)` | Before & After (collateral_asset) |
| `repay_against_collateral(user, amount, collateral_asset)` | Before & After (collateral_asset) |

### Multi-Asset Operations

| Operation | Invariant Checks |
|-----------|------------------|
| `liquidate(liquidator, borrower, debt_asset, collateral_asset, amount)` | Before & After (both assets) |

### Flash Loans

Flash loans are **excluded** from invariant checking during the callback phase because:
1. The balance temporarily changes during the loan
2. The invariant is restored upon repayment
3. The `FlashActive` guard prevents nested operations

## Panic Behavior

When drift is detected, the transaction **panics immediately** with:

```
RESERVE INVARIANT VIOLATION [BEFORE/AFTER]: 
  asset=<address>, 
  actual_balance=<value>, 
  expected_balance=<value>, 
  drift=<difference>
```

This ensures:
- No partial state updates survive
- The contract remains in a consistent state
- Drift is caught at the exact operation that caused it
- Debugging information is preserved in the panic message

## Accounting Components

### 1. Total Deposits (Single-Asset)

```rust
DataKey::TotalDeposits -> i128
```

Represents the sum of all user collateral deposits in single-asset mode.

### 2. Cross-Asset Collateral

```rust
DataKey::CollateralAsset(user, asset) -> i128
```

Per-user per-asset collateral balances in cross-asset mode.

### 3. Bad Debt

```rust
DataKey::BadDebt -> i128
```

Accumulated unrecoverable debt that reduces effective reserves.

### 4. Protocol Reserves

```rust
DataKey::Treasury(asset) -> i128
DepositDataKey::ProtocolReserve(asset) -> i128
```

Accumulated protocol fees from:
- Interest accrual (reserve factor split)
- Flash loan fees

## Testing Strategy

### Unit Tests

Located in `src/invariants.rs`:
- `test_invariant_passes_when_balanced` - Verify invariant passes with matching balances
- `test_invariant_panics_on_drift` - Verify panic on mismatch

### Integration Tests

Located in `src/invariant_integration_test.rs`:
- Verify all operations have before/after checks
- Test drift detection across operation sequences
- Verify multi-asset liquidation checks both assets
- Test bad debt accounting in expected balance computation

### Property-Based Tests

Extend `src/property_invariants_test.rs` to include:
- Random operation sequences maintain invariant
- No operation can create drift
- Liquidations preserve total system reserves

## Performance Considerations

### Computational Cost

Each invariant check performs:
1. One token balance query (external contract call)
2. Multiple storage reads for accounting state
3. Arithmetic to compute expected balance

**Estimated gas cost per check:** ~5,000-10,000 gas units

### Optimization Strategies

1. **Conditional Compilation**: Disable in production if gas costs are prohibitive
   ```rust
   #[cfg(feature = "invariant-checks")]
   invariants::check_invariant_before(&env, &asset);
   ```

2. **Sampling**: Check only N% of operations
   ```rust
   if env.ledger().sequence() % 10 == 0 {
       invariants::check_invariant_after(&env, &asset);
   }
   ```

3. **Critical Operations Only**: Check only high-risk operations (liquidate, flash_loan)

## Migration Path

### Phase 1: Add Checks (Current)
- ✅ Implement invariants module
- ✅ Add checks to all state-changing operations
- ✅ Write comprehensive tests

### Phase 2: Monitoring
- Deploy to testnet with invariant checks enabled
- Monitor for false positives
- Tune expected balance computation
- Measure gas impact

### Phase 3: Production
- Enable invariant checks on mainnet
- Set up alerting for invariant violations
- Plan emergency response procedures

### Phase 4: Optimization
- Profile gas usage
- Implement conditional compilation or sampling if needed
- Consider specialized checking for high-value operations

## Emergency Response

### If Invariant Violation Occurs in Production

1. **Immediate**: Transaction panics and reverts (no state change persists)
2. **Investigate**: Analyze the panic message to determine drift source
3. **Patch**: Fix the accounting bug or balance manipulation
4. **Re-deploy**: Upgrade contract with fix
5. **Post-Mortem**: Document root cause and prevention measures

### False Positive Scenario

If expected balance computation is incorrect:
1. The invariant will panic even though actual state is correct
2. Review `compute_expected_reserve` logic
3. Add missing accounting components
4. Update tests to cover edge case

## Security Considerations

### Attack Vectors Prevented

1. **Reentrancy**: Invariant catches unexpected balance changes during reentrant calls
2. **Flash Loan Attacks**: Detects manipulation of reserves during flash loan callbacks
3. **Accounting Bugs**: Catches off-by-one errors, rounding drift, overflow/underflow
4. **External Manipulation**: Detects direct token transfers that bypass accounting

### Limitations

1. **Performance**: Each check adds gas cost
2. **Granularity**: Checks at operation boundaries, not mid-operation
3. **Cross-Asset**: Current implementation focuses on single-asset mode
4. **Flash Loans**: Temporarily bypassed during callback (by design)

## Future Enhancements

### 1. Cross-Asset Mode Support

Aggregate collateral across all users for the asset:

```rust
fn compute_expected_reserve_cross_asset(env: &Env, asset: &Address) -> i128 {
    let mut total = 0i128;
    // Iterate UserCollateralAssets for each user
    // Sum CollateralAsset(user, asset) entries
    total
}
```

### 2. Reserve Factor Tracking

Include protocol reserves from interest accrual:

```rust
let reserve_balance: i128 = env
    .storage()
    .persistent()
    .get(&ReserveDataKey::ReserveBalance(asset))
    .unwrap_or(0);
expected = expected.checked_add(reserve_balance)?;
```

### 3. Detailed Drift Analysis

Expand panic message to show breakdown:

```
RESERVE INVARIANT VIOLATION [AFTER]:
  asset=<address>
  actual_balance=10000
  expected_balance=9950
  drift=+50
  
  Breakdown:
    total_deposits: 8000
    protocol_reserves: 2000
    bad_debt: -50
    expected: 9950
    
  Possible causes:
    - Unaccounted interest accrual
    - Missing reserve factor update
    - Rounding accumulation
```

### 4. Invariant Event Logging

Emit events on successful checks for monitoring:

```rust
env.events().publish(
    (symbol_short!("inv_ok"), asset),
    InvariantCheckEvent {
        checkpoint: "AFTER",
        operation: "deposit",
        actual: actual_balance,
        expected: expected_balance,
    }
);
```

## References

- `contracts/lending/src/invariants.rs` - Implementation
- `contracts/lending/src/invariant_integration_test.rs` - Integration tests
- `contracts/amm/src/lib.rs` - Similar pattern for constant product invariant
- `docs/RESERVE_ACCOUNTING.md` - Reserve factor and fee accounting
- `docs/PROTOCOL_ACCOUNTING.md` - Overall protocol accounting design

## Changelog

### 2026-08-26
- Initial implementation of reserve invariant checking
- Added checks to deposit, withdraw, borrow, repay operations
- Added checks to cross-asset borrow/repay operations
- Added dual-asset checks to liquidate operation
- Created comprehensive documentation

