# Deposit Collateral Function Documentation

## Overview

The deposit function allows users to deposit assets as collateral into the StellarLend protocol. The system enforces minimum deposit amounts, tracks total deposits against a protocol-wide cap, and supports pause functionality for emergency situations.

## Function Signature

```rust
pub fn deposit(
    env: Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<i128, DepositError>
```

## Parameters

- `env`: The contract environment
- `user`: The depositor's address (must authorize the transaction)
- `asset`: The address of the collateral asset (XLM, USDC, etc.)
- `amount`: The amount to deposit (must be positive and above minimum)

## Returns

- `Ok(i128)` — updated collateral balance for the user
- `Err(DepositError)` on failure

## Error Types

| Error | Description |
|-------|-------------|
| `InvalidAmount` | Amount is zero, negative, or below minimum deposit |
| `DepositPaused` | Deposit operations are currently paused |
| `Overflow` | Arithmetic overflow occurred during calculation |
| `AssetNotSupported` | The specified asset is not supported |
| `ExceedsDepositCap` | Protocol's total deposit cap would be exceeded |

## Security Assumptions

### Authorization
- User must authorize the transaction via `require_auth()`
- Prevents unauthorized deposits on behalf of other users

### Overflow Protection
- All arithmetic operations use `checked_add`
- Returns `DepositError::Overflow` if any calculation would overflow
- Prevents integer overflow attacks

### Deposit Cap
- Protocol enforces a maximum total deposit limit
- Each deposit checks if new total would exceed cap
- Protects protocol from excessive exposure

### Pause Mechanism
- Admin can pause all deposit operations
- Useful for emergency situations or upgrades
- Does not affect existing positions, only new deposits

## Usage Examples

### Basic Deposit

```rust
let user = Address::from_string("GUSER...");
let usdc = Address::from_string("GUSDC...");

// Deposit 10,000 USDC as collateral
let new_balance = contract.deposit(user.clone(), usdc, 10_000)?;
```

### Check Collateral Position

```rust
let position = contract.get_user_collateral_deposit(user.clone(), usdc.clone());
println!("Collateral: {}", position.amount);
println!("Last deposit: {}", position.last_deposit_time);
```

### Initialize Protocol

```rust
// Set deposit cap to 1 billion and minimum deposit to 100
contract.initialize_deposit_settings(1_000_000_000, 100)?;
```

### Pause/Unpause

```rust
// Pause deposits
contract.set_deposit_paused(true)?;

// Resume deposits
contract.set_deposit_paused(false)?;
```

## Data Structures

### CollateralPosition

```rust
pub struct CollateralPosition {
    pub amount: i128,             // Total collateral deposited
    pub asset: Address,           // Collateral asset
    pub last_deposit_time: u64,   // Last deposit timestamp
}
```

### DepositEvent

```rust
pub struct DepositEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub new_balance: i128,
    pub timestamp: u64,
}
```

## Events

The deposit function emits a `DepositEvent` on successful execution:

```rust
env.events().publish((Symbol::new(env, "deposit"),), event);
```

This event can be monitored off-chain for indexing and analytics.

## Storage

The contract uses persistent storage for:

- `UserCollateral(Address)`: Individual user collateral positions
- `TotalDeposits`: Protocol-wide total deposits
- `DepositCap`: Maximum allowed total deposits
- `MinDepositAmount`: Minimum deposit amount
- `Paused`: Deposit pause state

## Testing

Comprehensive tests cover:

- Successful deposit with valid amount
- Zero amount rejection
- Negative amount rejection
- Below minimum deposit rejection
- Deposit pause enforcement
- Deposit cap enforcement
- Multiple deposits accumulation
- Pause/unpause functionality
- Overflow protection
- Timestamp updates on deposit
- Separate user positions
- Deposit cap boundary (exact cap and cap+1)

Run tests with:
```bash
cargo test
```

## Security Considerations

1. **Authorization**: User must authorize via `require_auth()`
2. **Amount Validation**: Rejects zero, negative, and below-minimum amounts
3. **Overflow Protection**: All arithmetic uses checked operations
4. **Deposit Cap**: Prevents protocol over-exposure
5. **Pause Mechanism**: Emergency stop functionality
6. **Storage Isolation**: User positions stored separately
