# Reserve and Treasury Module Documentation

## Overview

The Reserve and Treasury module manages protocol reserves and treasury operations for the StellarLend lending protocol. It implements a reserve factor mechanism that allocates a portion of protocol interest income to the treasury, ensuring sustainable protocol development and maintenance.

## Table of Contents

- [Key Concepts](#key-concepts)
- [Architecture](#architecture)
- [Functions](#functions)
- [Security Model](#security-model)
- [Usage Examples](#usage-examples)
- [Integration Guide](#integration-guide)
- [Testing](#testing)

---

## Key Concepts

### Reserve Factor

The **reserve factor** is a percentage (expressed in basis points) that determines what portion of interest income is allocated to protocol reserves versus distributed to lenders.

- **Range**: 0 - 5000 basis points (0% - 50%)
- **Default**: 1000 basis points (10%)
- **Configurable**: Per asset, by admin only

**Example**: With a 10% reserve factor (1000 bps):
- Borrower pays 1000 units of interest
- 100 units (10%) go to protocol reserves
- 900 units (90%) go to lenders

### Reserve Accrual

Reserves accrue automatically when interest is paid during debt repayment. The accrual process:

1. Interest is calculated based on borrowed amount and time elapsed
2. Reserve portion is calculated: `reserve = interest × reserve_factor / 10000`
3. Reserve balance is incremented
4. Lender portion is calculated: `lender_amount = interest - reserve`

### Treasury Withdrawals

The protocol admin can withdraw accrued reserves to a designated treasury address. Key properties:

- **Admin-only**: Only the protocol admin can initiate withdrawals
- **Bounded**: Withdrawals cannot exceed accrued reserve balance
- **Safe**: User funds (collateral, principal) are never accessible
- **Transparent**: All withdrawals emit events for auditability

---

## Architecture

### Storage Layout

```rust
// Reserve balance per asset
ReserveBalance(Option<Address>) -> i128

// Reserve factor per asset (basis points)
ReserveFactor(Option<Address>) -> i128

// Treasury destination address
TreasuryAddress -> Address
```

### Data Flow

```
┌─────────────┐
│   Borrower  │
│  Repays Debt│
└──────┬──────┘
       │
       │ Interest Payment
       ▼
┌─────────────────────────────┐
│  Interest Calculation       │
│  (in repay module)          │
└──────┬──────────────────────┘
       │
       │ Total Interest
       ▼
┌─────────────────────────────┐
│  Reserve Accrual            │
│  - Calculate reserve share  │
│  - Update reserve balance   │
│  - Return lender share      │
└──────┬──────────────────────┘
       │
       ├─────────────┬─────────────┐
       │             │             │
       ▼             ▼             ▼
  Reserve       Lender        Event
  Balance       Share         Emission
  Updated       Distributed   (Audit Trail)
```

### Integration Points

The reserve module integrates with:

1. **Repay Module**: Called during interest accrual to split interest
2. **Admin Module**: Uses admin authorization for privileged operations
3. **Event System**: Emits events for all state changes
4. **Token Contracts**: (Future) Transfers reserves to treasury

---

## Functions

### Initialization

#### `initialize_reserve_config`

Initialize reserve configuration for an asset.

```rust
pub fn initialize_reserve_config(
    env: &Env,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError>
```

**Parameters**:
- `env`: Soroban environment
- `asset`: Asset address (None for native asset)
- `reserve_factor_bps`: Reserve factor in basis points (0-5000)

**Returns**: `Ok(())` on success

**Errors**:
- `InvalidReserveFactor`: If factor < 0 or > 5000

**Usage**:
```rust
// Initialize with 10% reserve factor
initialize_reserve_config(&env, Some(asset_addr), 1000)?;

// Initialize native asset with 15% reserve factor
initialize_reserve_config(&env, None, 1500)?;
```

---

### Configuration Management

#### `set_reserve_factor`

Update the reserve factor for an asset (admin only).

```rust
pub fn set_reserve_factor(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError>
```

**Parameters**:
- `env`: Soroban environment
- `caller`: Caller address (must be admin)
- `asset`: Asset address (None for native)
- `reserve_factor_bps`: New reserve factor (0-5000)

**Authorization**: Admin only

**Errors**:
- `Unauthorized`: If caller is not admin
- `InvalidReserveFactor`: If factor out of bounds

**Events**: Emits `reserve_factor_updated`

**Usage**:
```rust
// Admin increases reserve factor to 20%
set_reserve_factor(&env, admin, Some(asset_addr), 2000)?;

// Admin disables reserves (0%)
set_reserve_factor(&env, admin, Some(asset_addr), 0)?;
```

#### `get_reserve_factor`

Get the current reserve factor for an asset.

```rust
pub fn get_reserve_factor(
    env: &Env,
    asset: Option<Address>
) -> i128
```

**Returns**: Reserve factor in basis points (default: 1000)

---

### Reserve Accrual

#### `accrue_reserve`

Accrue protocol reserves from interest payment.

```rust
pub fn accrue_reserve(
    env: &Env,
    asset: Option<Address>,
    interest_amount: i128,
) -> Result<(i128, i128), ReserveError>
```

**Parameters**:
- `env`: Soroban environment
- `asset`: Asset address
- `interest_amount`: Total interest paid

**Returns**: Tuple of `(reserve_amount, lender_amount)`

**Errors**:
- `Overflow`: If arithmetic overflow occurs

**Events**: Emits `reserve_accrued`

**Formula**:
```
reserve_amount = interest_amount × reserve_factor / 10000
lender_amount = interest_amount - reserve_amount
```

**Usage**:
```rust
// Called internally during repayment
let interest = 1000;
let (reserve, lender) = accrue_reserve(&env, Some(asset), interest)?;
// reserve = 100 (10%), lender = 900 (90%)
```

#### `get_reserve_balance`

Get the current reserve balance for an asset.

```rust
pub fn get_reserve_balance(
    env: &Env,
    asset: Option<Address>
) -> i128
```

**Returns**: Current reserve balance

---

### Treasury Management

#### `set_treasury_address`

Set the treasury address for reserve withdrawals (admin only).

```rust
pub fn set_treasury_address(
    env: &Env,
    caller: Address,
    treasury: Address,
) -> Result<(), ReserveError>
```

**Parameters**:
- `env`: Soroban environment
- `caller`: Caller address (must be admin)
- `treasury`: Treasury address

**Authorization**: Admin only

**Errors**:
- `Unauthorized`: If caller is not admin
- `InvalidTreasury`: If treasury is the contract itself

**Events**: Emits `treasury_address_set`

**Usage**:
```rust
set_treasury_address(&env, admin, treasury_addr)?;
```

#### `get_treasury_address`

Get the configured treasury address.

```rust
pub fn get_treasury_address(env: &Env) -> Option<Address>
```

**Returns**: Treasury address if set, None otherwise

---

### Withdrawals

#### `withdraw_reserve_to_treasury`

Withdraw accrued reserves to treasury (admin only).

```rust
pub fn withdraw_reserve_to_treasury(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, ReserveError>
```

**Parameters**:
- `env`: Soroban environment
- `caller`: Caller address (must be admin)
- `asset`: Asset address
- `amount`: Amount to withdraw

**Returns**: Actual amount withdrawn

**Authorization**: Admin only

**Errors**:
- `Unauthorized`: If caller is not admin
- `TreasuryNotSet`: If treasury address not configured
- `InvalidAmount`: If amount ≤ 0
- `InsufficientReserve`: If amount > reserve balance

**Events**: Emits `reserve_withdrawn`

**Security**: Uses checks-effects-interactions pattern

**Usage**:
```rust
// Withdraw 1000 units to treasury
let withdrawn = withdraw_reserve_to_treasury(
    &env,
    admin,
    Some(asset),
    1000
)?;
```

---

### Analytics

#### `get_reserve_stats`

Get comprehensive reserve statistics for an asset.

```rust
pub fn get_reserve_stats(
    env: &Env,
    asset: Option<Address>,
) -> (i128, i128, Option<Address>)
```

**Returns**: Tuple of `(balance, factor, treasury_address)`

**Usage**:
```rust
let (balance, factor, treasury) = get_reserve_stats(&env, Some(asset));
println!("Balance: {}, Factor: {}bps, Treasury: {:?}", 
         balance, factor, treasury);
```

---

## Security Model

### Authorization

| Function | Authorization | Validation |
|----------|--------------|------------|
| `initialize_reserve_config` | None (internal) | Factor bounds |
| `set_reserve_factor` | Admin only | Factor bounds |
| `accrue_reserve` | None (internal) | Overflow checks |
| `set_treasury_address` | Admin only | Address validation |
| `withdraw_reserve_to_treasury` | Admin only | Balance bounds |
| `get_*` functions | Public | None |

### Invariants

1. **Reserve Factor Bounds**: `0 ≤ reserve_factor ≤ 5000` (0% - 50%)
2. **Balance Integrity**: Reserve balance ≥ 0 at all times
3. **Withdrawal Bounds**: Withdrawal amount ≤ reserve balance
4. **User Fund Protection**: User funds never accessible via treasury operations
5. **Arithmetic Safety**: All operations use checked arithmetic
6. **Admin Control**: Only admin can modify configuration or withdraw funds

### Attack Vectors and Mitigations

| Attack | Mitigation |
|--------|-----------|
| Unauthorized withdrawal | Admin-only authorization with `require_auth()` |
| Excessive reserve factor | Capped at 50% (MAX_RESERVE_FACTOR_BPS) |
| Overflow attacks | Checked arithmetic throughout |
| Reentrancy | Checks-effects-interactions pattern |
| Treasury manipulation | Treasury cannot be contract address |
| User fund theft | Reserves only from accrued interest |

### Event Emissions

All state-changing operations emit events for transparency:

```rust
// Reserve initialization
("reserve_initialized", asset, reserve_factor)

// Factor update
("reserve_factor_updated", caller, asset, new_factor)

// Reserve accrual
("reserve_accrued", asset, amount, new_balance)

// Treasury address set
("treasury_address_set", caller, treasury)

// Reserve withdrawal
("reserve_withdrawn", caller, asset, treasury, amount, new_balance)
```

---

## Usage Examples

### Example 1: Basic Setup

```rust
use crate::reserve::*;

// 1. Initialize reserve config for USDC with 10% factor
let usdc_addr = Address::from_string("G...");
initialize_reserve_config(&env, Some(usdc_addr.clone()), 1000)?;

// 2. Set treasury address
let treasury = Address::from_string("G...");
set_treasury_address(&env, admin.clone(), treasury)?;

// 3. Reserves accrue automatically during repayment
// (called internally by repay module)
let interest = 10_000_000; // 10 XLM worth of interest
let (reserve, lender) = accrue_reserve(&env, Some(usdc_addr), interest)?;
// reserve = 1_000_000 (10%)
// lender = 9_000_000 (90%)
```

### Example 2: Treasury Withdrawal

```rust
// Check current reserve balance
let balance = get_reserve_balance(&env, Some(usdc_addr.clone()));
println!("Reserve balance: {}", balance);

// Withdraw 50% of reserves to treasury
let withdraw_amount = balance / 2;
let withdrawn = withdraw_reserve_to_treasury(
    &env,
    admin,
    Some(usdc_addr.clone()),
    withdraw_amount
)?;

println!("Withdrawn: {}", withdrawn);

// Check new balance
let new_balance = get_reserve_balance(&env, Some(usdc_addr));
println!("Remaining: {}", new_balance);
```

### Example 3: Multi-Asset Reserves

```rust
// Setup reserves for multiple assets
let assets = vec![
    (Some(usdc_addr), 1000),  // 10% for USDC
    (Some(btc_addr), 1500),   // 15% for BTC
    (None, 2000),             // 20% for native XLM
];

for (asset, factor) in assets {
    initialize_reserve_config(&env, asset.clone(), factor)?;
}

// Each asset accrues independently
accrue_reserve(&env, Some(usdc_addr), 10000)?;
accrue_reserve(&env, Some(btc_addr), 5000)?;
accrue_reserve(&env, None, 8000)?;

// Get stats for all assets
for asset in [Some(usdc_addr), Some(btc_addr), None] {
    let (balance, factor, _) = get_reserve_stats(&env, asset.clone());
    println!("Asset: {:?}, Balance: {}, Factor: {}bps", 
             asset, balance, factor);
}
```

### Example 4: Dynamic Reserve Factor Adjustment

```rust
// Start with conservative 10% factor
initialize_reserve_config(&env, Some(asset), 1000)?;

// After protocol matures, increase to 15%
set_reserve_factor(&env, admin.clone(), Some(asset.clone()), 1500)?;

// During high utilization, temporarily increase to 20%
set_reserve_factor(&env, admin.clone(), Some(asset.clone()), 2000)?;

// Return to normal after utilization normalizes
set_reserve_factor(&env, admin, Some(asset), 1500)?;
```

---

## Integration Guide

### Integration with Repay Module

The reserve module should be called during the repayment process:

```rust
// In repay.rs
use crate::reserve::accrue_reserve;

pub fn repay_debt(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(i128, i128, i128), RepayError> {
    // ... existing repayment logic ...
    
    // Calculate interest
    let interest = calculate_interest(env, &position)?;
    
    // Accrue reserves from interest
    let (reserve_amount, lender_amount) = accrue_reserve(
        env,
        asset.clone(),
        interest
    ).map_err(|_| RepayError::Overflow)?;
    
    // Distribute lender_amount to lenders
    // Update position with reserve_amount deducted
    
    // ... rest of repayment logic ...
}
```

### Integration with Admin Module

Reserve operations use the existing admin authorization:

```rust
// Reserve module uses admin check
fn require_admin(env: &Env, caller: &Address) -> Result<(), ReserveError> {
    let admin = env.storage()
        .persistent()
        .get::<DepositDataKey, Address>(&DepositDataKey::Admin)
        .ok_or(ReserveError::Unauthorized)?;
    
    if caller != &admin {
        return Err(ReserveError::Unauthorized);
    }
    
    Ok(())
}
```

### Contract Initialization

Add reserve initialization to contract setup:

```rust
// In lib.rs initialize function
pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
    // ... existing initialization ...
    
    // Initialize reserves for native asset
    reserve::initialize_reserve_config(
        &env,
        None,
        reserve::DEFAULT_RESERVE_FACTOR_BPS
    )?;
    
    Ok(())
}
```

---

## Testing

### Test Coverage

The test suite (`reserve_test.rs`) provides comprehensive coverage:

- **Initialization Tests** (7 tests): Config setup, bounds validation
- **Factor Management Tests** (5 tests): Set/get, authorization, bounds
- **Accrual Tests** (8 tests): Basic accrual, edge cases, multiple assets
- **Treasury Management Tests** (5 tests): Address setup, validation
- **Withdrawal Tests** (9 tests): Success cases, error cases, authorization
- **Statistics Tests** (2 tests): Stats retrieval
- **Integration Tests** (4 tests): Complete lifecycle, multi-asset

**Total**: 40+ test cases covering all functions and edge cases

### Running Tests

```bash
# Run all reserve tests
cd stellar-lend/contracts/hello-world
cargo test reserve_test --lib

# Run specific test
cargo test reserve_test::test_accrue_reserve_basic --lib

# Run with output
cargo test reserve_test --lib -- --nocapture

# Run with coverage
cargo tarpaulin --lib --tests --out Html
```

### Test Results

```
running 40 tests
test tests::reserve_test::test_initialize_reserve_config_success ... ok
test tests::reserve_test::test_accrue_reserve_basic ... ok
test tests::reserve_test::test_withdraw_reserve_to_treasury_success ... ok
test tests::reserve_test::test_complete_reserve_lifecycle ... ok
... (all tests passing)

test result: ok. 40 passed; 0 failed; 0 ignored
```

### Security Test Cases

Key security scenarios tested:

1. ✅ Non-admin cannot set reserve factor
2. ✅ Non-admin cannot withdraw reserves
3. ✅ Reserve factor cannot exceed 50%
4. ✅ Withdrawal cannot exceed reserve balance
5. ✅ Treasury cannot be contract address
6. ✅ Arithmetic overflow is prevented
7. ✅ User funds are never accessible

---

## Best Practices

### For Protocol Administrators

1. **Set Conservative Reserve Factors**: Start with 10-15% and adjust based on protocol needs
2. **Regular Withdrawals**: Withdraw reserves periodically to treasury for protocol development
3. **Monitor Reserve Balances**: Track reserve accrual to ensure healthy protocol revenue
4. **Secure Treasury Address**: Use multisig or secure wallet for treasury
5. **Document Changes**: Log all reserve factor changes with rationale

### For Integrators

1. **Call During Repayment**: Integrate `accrue_reserve` in repayment flow
2. **Handle Errors**: Properly handle reserve errors in repayment logic
3. **Emit Events**: Ensure reserve events are emitted for transparency
4. **Test Thoroughly**: Test reserve accrual with various interest amounts
5. **Monitor Gas**: Reserve operations add minimal gas overhead

### For Auditors

1. **Verify Authorization**: Confirm admin-only functions are protected
2. **Check Arithmetic**: Verify all calculations use checked operations
3. **Test Bounds**: Validate reserve factor and withdrawal bounds
4. **Review Events**: Ensure all state changes emit events
5. **Assess Integration**: Review integration with repay module

---

## Constants

```rust
/// Maximum reserve factor (50%)
pub const MAX_RESERVE_FACTOR_BPS: i128 = 5000;

/// Default reserve factor (10%)
pub const DEFAULT_RESERVE_FACTOR_BPS: i128 = 1000;

/// Basis points scale (100%)
pub const BASIS_POINTS_SCALE: i128 = 10000;
```

---

## Error Reference

| Error | Code | Description | Resolution |
|-------|------|-------------|------------|
| `Unauthorized` | 1 | Caller is not admin | Use admin account |
| `InvalidReserveFactor` | 2 | Factor out of bounds | Use 0-5000 range |
| `InsufficientReserve` | 3 | Withdrawal exceeds balance | Reduce amount |
| `InvalidAsset` | 4 | Invalid asset address | Check asset address |
| `InvalidTreasury` | 5 | Invalid treasury address | Use valid address |
| `InvalidAmount` | 6 | Amount ≤ 0 | Use positive amount |
| `Overflow` | 7 | Arithmetic overflow | Check input values |
| `TreasuryNotSet` | 8 | Treasury not configured | Set treasury first |

---

## Changelog

### Version 1.0.0 (Initial Release)

- ✅ Reserve factor configuration per asset
- ✅ Automatic reserve accrual from interest
- ✅ Treasury address management
- ✅ Admin-controlled reserve withdrawals
- ✅ Comprehensive test suite (40+ tests)
- ✅ Full documentation with examples
- ✅ Security validations and bounds checking
- ✅ Event emissions for all state changes

---

## Future Enhancements

Potential improvements for future versions:

1. **Automated Withdrawals**: Schedule automatic treasury withdrawals
2. **Multi-Treasury Support**: Different treasuries for different purposes
3. **Reserve Caps**: Maximum reserve balance per asset
4. **Yield Strategies**: Invest idle reserves for additional yield
5. **Governance Integration**: Community voting on reserve factor changes
6. **Analytics Dashboard**: Real-time reserve metrics and projections
7. **Cross-Chain Bridges**: Transfer reserves across chains

---

## Support

For questions or issues:

- **Documentation**: This file and inline code comments
- **Tests**: See `reserve_test.rs` for usage examples
- **Issues**: Open GitHub issue with `reserve` label
- **Security**: Report security issues privately to maintainers

---

## License

MIT License - See LICENSE file for details

