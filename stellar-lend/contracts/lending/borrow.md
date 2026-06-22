# Borrow Function Documentation

## Canonical contract tree

The live Soroban lending crate is `stellar-lend/contracts/lending`. Interest accrual is implemented in `src/rounding_strategy.rs` and `src/debt.rs`, with borrow and repay settling accrual in `src/lib.rs`.

The sibling path `contracts/lending/scr/` (misnamed for `src`) is a legacy reference implementation. New changes belong in `stellar-lend/contracts/lending` only.

## Interest accrual

| Item | Value |
| --- | --- |
| Rounding mode | Banker's (round half to even) |
| Annual rate | 500 basis points (5% APR) |
| Principal units | Asset smallest units (`i128`) |
| Rate units | Basis points per year (10_000 = 100%) |
| Time units | Ledger timestamp seconds (`u64`) |
| Storage | `DebtPosition { principal, last_update }` per user |

Accrual runs on `borrow` and `repay` before principal changes. `get_position` reports principal plus pending interest without persisting a view-time accrual.

Formula (scaled internally with `INTEREST_PRECISION = 1_000_000`):

```
interest = principal * elapsed_seconds * rate_bps / (SECONDS_PER_YEAR * 10_000)
```

`SECONDS_PER_YEAR = 31_536_000`.

### Interest Accrual Ordering on Repay

**Security Invariant**: Interest MUST be accrued BEFORE the repay amount is subtracted from the debt.

The order of operations on `repay` is:
1. **Accrue interest** based on elapsed time since `last_update`
2. **Apply repayment** to the accrued total (principal + interest)
3. **Update timestamp** to current ledger time

This ordering ensures users cannot avoid interest by timing their repayments. If the order were reversed (apply-then-accrue), users could repay before interest accrues, effectively getting interest-free loans.

**Test Coverage**: The ordering invariant is verified by comprehensive ledger-time-advancement tests in `src/interest_ordering_time_test.rs`, covering:
- Zero elapsed time (immediate repay)
- Exact one-year boundary
- Repay smaller than accrued interest
- Multiple borrows and repays with time gaps
- Adversarial timing attempts
- Timestamp boundary conditions

**Example**:
```rust
// User borrows 10,000 at t=0
borrow(user, 10_000);

// One year passes (t=31,536,000)
// Interest accrues: 10,000 * 5% = 500
// Total debt: 10,500

// User repays 1,000
repay(user, 1_000);

// Remaining debt: 10,500 - 1,000 = 9,500
// NOT: 10,000 - 1,000 + 500 = 9,500 (wrong order)
```

## Overview

The borrow function allows users to borrow from the StellarLend protocol against previously deposited collateral. The system enforces a minimum borrow amount, post-borrow health factor, accrued-interest settlement, and protocol-level constraints such as debt ceilings and emergency states.

## Function Signature

```rust
pub fn borrow(
    env: Env,
    user: Address,
    amount: i128,
) -> Result<i128, LendingError>
```

## Parameters

- `env`: The contract environment
- `user`: The borrower's address (must authorize the transaction)
- `amount`: The amount to borrow (must be positive and above minimum)

## Returns

- `Ok(updated_principal)` on successful borrow
- `Err(LendingError)` on failure

## Error Types

| Error | Description |
| --- | --- |
| `InsufficientCollateral` | Post-borrow health factor would fall below `10000` |
| `DebtCeilingExceeded` | Protocol's total debt ceiling would be exceeded |
| `InvalidAmount` | Amount is zero or negative |
| `BelowMinimumBorrow` | Borrow amount is below the minimum threshold |
| `Overflow` | Arithmetic overflow occurred during calculation |
| `Unauthorized` | User did not authorize the transaction |

## Security Assumptions

### Health Factor

- **Minimum Health Factor**: `10000` (1.0)
- Borrow uses the current borrower debt after interest accrual plus the requested amount.
- Health factor is evaluated as:

  ```
  collateral * LIQUIDATION_THRESHOLD_BPS / new_debt >= 10_000
  ```

  Equivalently (overflow-safe form used in the contract):

  ```
  collateral * LIQUIDATION_THRESHOLD_BPS >= new_debt * 10_000
  ```

- With the current `LIQUIDATION_THRESHOLD_BPS = 8000`, a borrower must have at least 125% collateralization after the borrow.
- Prevents under-collateralized positions that could lead to protocol insolvency.

### Interest Calculation

- **Annual Rate**: 5% (500 basis points)
- Interest accrues on each `borrow` and `repay` using banker's rounding via `calculate_interest_with_rounding`
- Formula: `principal * rate_bps * elapsed_seconds / (BASIS_POINTS_SCALE * SECONDS_PER_YEAR)`
- Checked arithmetic; overflow surfaces as contract panic on mutating paths

### Overflow Protection

- All arithmetic operations use checked methods (`checked_add`, `checked_mul`, etc.)
- Returns `BorrowError::Overflow` if any calculation would overflow
- Prevents integer overflow attacks and ensures data integrity

### Debt Ceiling

- Protocol enforces a maximum total debt limit
- Each borrow checks whether post-borrow `TotalDebt` would exceed `DataKey::DebtCeiling`
- If unset, the default ceiling is 1,000,000,000,000
- Protects protocol from excessive leverage

### Minimum Borrow Threshold

- **Storage Key**: `BorrowMinAmount` (stored in the contract instance storage)
- **Error Code**: `LendingError::BelowMinimumBorrow` (`1008`)
- **Rationale**: Dust-sized loans accrue negligible interest (which rounds to zero under discrete math) and are highly uneconomic to liquidate since gas/transaction fees exceed the loan's value. Enforcing a configurable minimum borrow size protects protocol liquidity, prevents unliquidatable bad debt, and preserves the protocol's economics.
- **Admin Configuration**: The admin can update the minimum borrow size dynamically at any time using the `set_min_borrow` endpoint.

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
    pub principal: i128,
    pub last_update: u64,
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

1. **Always check health factor**: Ensure collateral supports the full post-borrow debt including accrued interest
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
