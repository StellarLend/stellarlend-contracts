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

The borrow function allows users to borrow assets from the StellarLend protocol against deposited collateral. The system accrues interest on each borrow, enforces a post-borrow health factor of at least 1.0, and respects protocol-level constraints such as debt ceilings and pause states.

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

- `Ok(principal)` with the updated debt principal on success
- `Err(LendingError)` on failure

## Error Types

| Error                    | Description                                           |
| ------------------------ | ----------------------------------------------------- |
| `InsufficientCollateral` | Post-borrow health factor would fall below 1.0        |
| `DebtCeilingExceeded`    | Protocol's total debt ceiling would be exceeded       |
| `InvalidAmount`          | Amount is zero or negative                            |
| `BelowMinimumBorrow`     | Borrow amount is below the minimum threshold          |
| `Overflow`               | Arithmetic overflow occurred during calculation       |

Borrow also panics when the protocol or borrow operation is paused, or when emergency state blocks borrows.

## Security Assumptions

### Health Factor (Collateralization)

- **Liquidation threshold**: 80% (`LIQUIDATION_THRESHOLD_BPS = 8000`)
- **Minimum health factor**: 1.0 (`HEALTH_FACTOR_SCALE = 10000`)
- Before committing debt, `borrow` loads `DataKey::Collateral`, computes effective debt after accrual via `effective_debt` using `current_borrow_rate`, and requires:

  ```
  collateral * LIQUIDATION_THRESHOLD_BPS >= HEALTH_FACTOR_SCALE * new_debt
  ```

  Equivalently: `(collateral * 8000) / new_debt >= 10000`.

- **Worked example**: 100 collateral and 80 debt → `100 * 8000 = 800_000 >= 10_000 * 80 = 800_000` (HF exactly 1.0). A borrow to 81 debt would fail because `800_000 < 810_000`.

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

- Protocol enforces a maximum total debt limit via `DataKey::DebtCeiling` (set by admin through `set_debt_ceiling`)
- Each borrow checks whether post-borrow `TotalDebt` would exceed the ceiling
- Returns `LendingError::DebtCeilingExceeded` when `new_total_debt > ceiling`
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

// Deposit collateral, then borrow up to the health-factor limit.
contract.deposit(user.clone(), 200)?;
contract.borrow(user.clone(), 100)?; // HF = (200 * 8000) / 100 = 16000
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

1. **Always check collateral ratio**: Ensure collateral is at least 150% of borrow amount
2. **Monitor interest accrual**: Interest compounds over time, check positions regularly
3. **Respect debt ceiling**: Large borrows may fail if they exceed protocol limits
4. **Handle pause state**: Implement retry logic for paused protocol scenarios
5. **Use appropriate amounts**: Ensure amounts are above minimum thresholds

## Testing

Comprehensive tests cover:

- ✅ Successful borrow with sufficient collateral
- ✅ Insufficient collateral / sub-1.0 health factor rejection
- ✅ Protocol pause enforcement
- ✅ Invalid amount validation
- ✅ Below minimum borrow rejection
- ✅ Debt ceiling enforcement (post-borrow `TotalDebt`)
- ✅ Multiple borrows accumulation
- ✅ Interest accrual over time
- ✅ Health factor boundary and overflow protection
- ✅ Pause/unpause functionality

Dedicated coverage lives in `src/borrow_health_factor_test.rs`.

Run tests with:

```bash
cargo test -p stellarlend-lending
```

## Security Considerations

1. **Authorization**: User must authorize the transaction via `require_auth()`
2. **Health factor validation**: Post-borrow HF must be `>= 1.0` using effective debt and liquidation threshold
3. **Overflow protection**: All arithmetic uses checked operations
4. **Debt ceiling**: Post-borrow `TotalDebt` cannot exceed `DataKey::DebtCeiling`
5. **Pause mechanism**: Emergency stop functionality
6. **Interest Calculation**: Uses saturating arithmetic to prevent overflow
7. **Storage Isolation**: User positions stored separately to prevent cross-contamination

## Future Enhancements

- Multi-asset collateral support
- Dynamic interest rates based on utilization
- Liquidation mechanism for under-collateralized positions
- Oracle integration for accurate asset pricing
- Variable collateral ratios per asset type
- Governance-controlled parameter updates
