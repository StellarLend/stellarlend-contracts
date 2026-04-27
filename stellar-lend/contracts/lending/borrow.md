# Borrow Function Documentation

## Overview

The borrow function allows users to borrow assets from the StellarLend protocol by providing collateral. The system enforces minimum collateral ratios, tracks interest accrual, and respects protocol-level constraints such as debt ceilings and pause states.

## Function Signature

```rust
pub fn borrow(
    env: Env,
    user: Address,
    asset: Address,
    amount: i128,
    collateral_asset: Address,
    collateral_amount: i128,
) -> Result<(), BorrowError>
```

## Parameters

- `env`: The contract environment
- `user`: The borrower's address (must authorize the transaction)
- `asset`: The address of the asset to borrow
- `amount`: The amount to borrow (must be positive and above minimum)
- `collateral_asset`: The address of the collateral asset
- `collateral_amount`: The amount of collateral to deposit (must be positive)

## Returns

- `Ok(())` on successful borrow
- `Err(BorrowError)` on failure

## Error Types

| Error                    | Description                                           |
| ------------------------ | ----------------------------------------------------- |
| `InsufficientCollateral` | Collateral ratio is below the minimum required (150%) |
| `DebtCeilingReached`     | Protocol's total debt ceiling would be exceeded       |
| `ProtocolPaused`         | Borrow operations are currently paused                |
| `InvalidAmount`          | Amount or collateral is zero or negative              |
| `BelowMinimumBorrow`     | Borrow amount is below the minimum threshold          |
| `Overflow`               | Arithmetic overflow occurred during calculation       |
| `Unauthorized`           | User did not authorize the transaction                |
| `AssetNotSupported`      | The specified asset is not supported                  |

## Security Assumptions

### Collateral Ratio

- **Minimum Ratio**: 150% (15000 basis points)
- Users must provide collateral worth at least 1.5x the borrowed amount
- Ratio is calculated as: `(collateral_amount * 10000) / borrow_amount`
- Prevents under-collateralized positions that could lead to protocol insolvency

### Interest Calculation

- **Annual Rate**: 5% (500 basis points)
- Interest accrues continuously based on time elapsed
- Formula: `borrowed_amount * interest_rate * time_elapsed / (10000 * seconds_per_year)`
- Uses saturating arithmetic to prevent overflow

### Overflow Protection

- All arithmetic operations use checked methods (`checked_add`, `checked_mul`, etc.)
- Returns `BorrowError::Overflow` if any calculation would overflow
- Prevents integer overflow attacks and ensures data integrity

### Debt Ceiling

- Protocol enforces a maximum total debt limit
- Each borrow checks if new total debt would exceed ceiling
- Protects protocol from excessive leverage

### Pause Mechanism

- Admin can pause all borrow operations
- Useful for emergency situations or upgrades
- Does not affect existing positions, only new borrows

## Usage Examples

### Basic Borrow

```rust
let user = Address::from_string("GUSER...");
let usdc = Address::from_string("GUSDC...");
let xlm = Address::from_string("GXLM...");

// Borrow 10,000 USDC with 20,000 XLM collateral (200% ratio)
contract.borrow(
    user.clone(),
    usdc,
    10_000,
    xlm,
    20_000
)?;
```

### Check User Position

```rust
// Get current debt including accrued interest
let debt = contract.get_user_debt(user.clone());
println!("Borrowed: {}", debt.borrowed_amount);
println!("Interest: {}", debt.interest_accrued);

// Get collateral position
let collateral = contract.get_user_collateral(user.clone());
println!("Collateral: {}", collateral.amount);
```

### Initialize Protocol

```rust
// Set admin, debt ceiling to 1 billion, and minimum borrow to 1,000
contract.initialize(&admin, 1_000_000_000, 1_000)?;
```

### Pause/Unpause (Granular)

```rust
// Pause borrowing specifically
contract.set_pause(&admin, PauseType::Borrow, true)?;

// Resume borrowing
contract.set_pause(&admin, PauseType::Borrow, false)?;
```

## Data Structures

### DebtPosition

```rust
pub struct DebtPosition {
    pub borrowed_amount: i128,    // Total borrowed
    pub interest_accrued: i128,   // Accrued interest
    pub last_update: u64,         // Last update timestamp
    pub asset: Address,           // Borrowed asset
}
```

### CollateralPosition

```rust
pub struct CollateralPosition {
    pub amount: i128,      // Collateral amount
    pub asset: Address,    // Collateral asset
}
```

### BorrowEvent

```rust
pub struct BorrowEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub collateral: i128,
    pub timestamp: u64,
}
```

## Events

The borrow function emits a `BorrowEvent` on successful execution:

```rust
env.events().publish((Symbol::new(env, "borrow"),), event);
```

This event can be monitored off-chain for indexing and analytics.

## Storage

The contract uses persistent storage for:

- `UserDebt(Address)`: Individual user debt positions
- `UserCollateral(Address)`: Individual user collateral positions
- `TotalDebt`: Protocol-wide total debt
- `DebtCeiling`: Maximum allowed total debt
- `MinBorrowAmount`: Minimum borrow amount
- `Paused`: Protocol pause state

## Best Practices

1. **Always check collateral ratio**: Ensure collateral is at least 150% of borrow amount
2. **Monitor interest accrual**: Interest compounds over time, check positions regularly
3. **Respect debt ceiling**: Large borrows may fail if they exceed protocol limits
4. **Handle pause state**: Implement retry logic for paused protocol scenarios
5. **Use appropriate amounts**: Ensure amounts are above minimum thresholds

## Testing

Comprehensive tests cover:

- ✅ Successful borrow with valid collateral
- ✅ Insufficient collateral rejection
- ✅ Protocol pause enforcement
- ✅ Invalid amount validation
- ✅ Below minimum borrow rejection
- ✅ Debt ceiling enforcement
- ✅ Multiple borrows accumulation
- ✅ Interest accrual over time
- ✅ Collateral ratio validation
- ✅ Pause/unpause functionality
- ✅ Overflow protection

Run tests with:

```bash
cargo test
```

## Security Considerations

1. **Authorization**: User must authorize the transaction via `require_auth()`
2. **Collateral Validation**: Strict enforcement of 150% minimum ratio
3. **Overflow Protection**: All arithmetic uses checked operations
4. **Debt Ceiling**: Prevents protocol over-leverage
5. **Pause Mechanism**: Emergency stop functionality
6. **Interest Calculation**: Uses saturating arithmetic to prevent overflow
7. **Storage Isolation**: User positions stored separately to prevent cross-contamination

## Future Enhancements

- Multi-asset collateral support
- Dynamic interest rates based on utilization
- Liquidation mechanism for under-collateralized positions
- Oracle integration for accurate asset pricing
- Variable collateral ratios per asset type
- Governance-controlled parameter updates
