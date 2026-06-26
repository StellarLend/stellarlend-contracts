# Interest Numeric Assumptions and Safety Limits

This note documents the canonical numeric constants, scaling factors, rounding modes, and overflow/underflow protections used across the StellarLend interest accrual system.

## Scope

- `contracts/lending/src/debt.rs` (`accrue_interest`, `settle_accrual`, `effective_debt`, `borrow_amount`, `repay_amount`)
- `contracts/lending/src/rounding_strategy.rs` (`calculate_interest_with_rounding`, `apply_rounding`, `reconcile_debt_with_drift_correction`)
- `contracts/lending/src/lib.rs` (`get_position` health factor calculation, liquidation math)

## Canonical Constants

All constants are defined in `contracts/lending/src/rounding_strategy.rs`:

| Constant | Type | Value | Purpose |
|----------|------|-------|---------|
| `INTEREST_PRECISION` | `i128` | `1_000_000` (10^6) | Intermediate fractional precision for interest math |
| `BASIS_POINTS_SCALE` | `i128` | `10_000` | Denominator for basis-points (100% = 10_000 bps) |
| `SECONDS_PER_YEAR` | `u64` | `31_536_000` (365 * 24 * 60 * 60) | Time denominator for APR calculations |
| `DEFAULT_APR_BPS` | `i128` | `500` | Default annual percentage rate (5%) |

### Important: This Protocol Does NOT Use SCALE_18

Some DeFi protocols use `SCALE_18 = 10^18` for fixed-point arithmetic. **This protocol uses `INTEREST_PRECISION = 10^6`** for intermediate interest calculations. The 10^6 scale provides 6 decimal places of fractional precision, which is sufficient for sub-cent accuracy on typical loan sizes while keeping intermediate products within `i128` bounds.

### Combined Denominator

The full denominator used in interest calculations is:

```
DENOMINATOR = SECONDS_PER_YEAR * BASIS_POINTS_SCALE
            = 31_536_000 * 10_000
            = 315_360_000_000
```

## Interest Calculation Formula

The core formula (from `calculate_interest_with_rounding`) is:

```
numerator   = borrowed_amount * elapsed_seconds * rate_bps * INTEREST_PRECISION
denominator = SECONDS_PER_YEAR * BASIS_POINTS_SCALE  (= 315_360_000_000)

raw_result  = numerator / denominator        (integer division)
remainder   = numerator % denominator        (fractional remainder)

final_interest = raw_result / INTEREST_PRECISION   (back-convert from precision scale)
```

### Worked Example 1: $100,000 at 5% APR for 1 second

```
borrowed_amount  = 100_000
elapsed_seconds  = 1
rate_bps         = 500  (5% APR)

numerator   = 100_000 * 1 * 500 * 1_000_000
            = 50_000_000_000_000

denominator = 315_360_000_000

raw_result  = 50_000_000_000_000 / 315_360_000_000
            = 158 (integer division)

remainder   = 50_000_000_000_000 % 315_360_000_000
            = 172_160_000_000

final_interest = 158 / 1_000_000 = 0  (truncated to 0 whole units)
```

With **Bankers rounding** (the default in `debt.rs`), the fractional part `172_160_000_000 / 315_360_000_000 ≈ 0.546` is greater than 0.5, so the raw_result rounds up to 159:

```
final_interest = 159 / 1_000_000 = 0  (still 0 whole units)
```

This demonstrates that for very small time intervals, interest accrual rounds to 0 at the token-unit level. The fractional remainder is tracked in `InterestCalcResult.remainder` for drift analysis but is not added to debt.

### Worked Example 2: $100 at 5% APR for 1 year

```
borrowed_amount  = 100
elapsed_seconds  = 31_536_000 (SECONDS_PER_YEAR)
rate_bps         = 500

numerator   = 100 * 31_536_000 * 500 * 1_000_000
            = 1_576_800_000_000_000_000

denominator = 315_360_000_000

raw_result  = 1_576_800_000_000_000_000 / 315_360_000_000
            = 5_000_000

remainder   = 0 (exact division)

final_interest = 5_000_000 / 1_000_000 = 5  (exactly $5)
```

### Worked Example 3: $1,000 at 5% APR for 1 month

```
borrowed_amount  = 1_000
elapsed_seconds  = 2_628_000 (SECONDS_PER_YEAR / 12)
rate_bps         = 500

numerator   = 1_000 * 2_628_000 * 500 * 1_000_000
            = 1_314_000_000_000_000_000

denominator = 315_360_000_000

raw_result  = 1_314_000_000_000_000_000 / 315_360_000_000
            = 4_166_666

remainder   = 1_314_000_000_000_000_000 % 315_360_000_000
            = 208_000_000_000

With Bankers rounding:
  half_divisor = 315_360_000_000 / 2 = 157_680_000_000
  remainder (208_000_000_000) > half_divisor (157_680_000_000)
  => rounds up to 4_166_667

final_interest = 4_166_667 / 1_000_000 = 4  (truncated to 4 whole units)
```

The exact interest for 1 month at 5% on $1,000 is $4.167. After rounding and back-conversion, the protocol accrues 4 whole units.

## Basis Points (BPS) Conversions

The protocol uses basis points throughout for rates, thresholds, and factors:

| BPS Value | Percentage | Usage |
|-----------|------------|-------|
| `10_000` | 100% | Full utilization, max rate ceiling |
| `5_000` | 50% | Close factor (liquidation) |
| `1_000` | 10% | Liquidation incentive bonus |
| `500` | 5% | Default APR |
| `100` | 1% | Max drift tolerance example |
| `8_000` | 80% | Liquidation threshold (health factor base) |

### BPS to Decimal Conversion

```
decimal = bps / 10_000
bps     = decimal * 10_000
```

Example: `500 bps / 10_000 = 0.05` (5%)

### Health Factor Scale

Health factor uses the same `10_000` base as BPS:

- `10_000` = healthy (HF = 1.0)
- `< 10_000` = liquidatable
- `100_000` = sentinel for no-debt positions (see `lib.rs:get_position`)

The health factor formula (from `lib.rs:549`):
```
health_factor = (collateral * 8000) / debt
```
Where `8000` is the `LIQUIDATION_THRESHOLD` in BPS (80%).

See [`docs/glossary.md#health-factor-hf`](../../docs/glossary.md#health-factor-hf) for the full glossary entry.

## Rounding Modes

Four rounding modes are available in `RoundingMode`:

| Mode | Behavior | When to Use |
|------|----------|-------------|
| `Truncate` | Drops fractional part (always rounds toward zero) | Not used for accrual |
| `Floor` | Same as truncate for positive values | Not used for accrual |
| `Bankers` | Round to nearest; ties round to even | **Default for debt accrual** |
| `Ceil` | Always rounds up (any fractional part -> +1) | Conservative scenarios |

### Rounding Direction at Every Boundary

| Operation | Rounding Mode | Direction | Rationale |
|-----------|---------------|-----------|-----------|
| Debt accrual (`accrue_interest`) | `Bankers` | Nearest, ties to even | Minimizes cumulative drift over many accruals |
| Health factor calculation | Truncate (integer division) | Down (toward zero) | Conservative: overestimates risk |
| Liquidation seized collateral | Truncate (integer division) | Down | Protocol-safe: never seizes more than owed |
| Flash loan fee | Truncate (integer division) | Down | Borrower-safe: fee never exceeds exact amount |
| Close factor repayment cap | Truncate (integer division) | Down | Borrower-safe: caps repayment conservatively |

### Bankers Rounding Detail

Bankers rounding (`apply_rounding` in `rounding_strategy.rs:115-144`):

```
if remainder < half_divisor:
    round down (keep quotient)
elif remainder > half_divisor:
    round up (quotient + 1)
else:  // remainder == half_divisor (exact tie)
    if quotient is even:
        round down (keep quotient)
    else:
        round up (quotient + 1)
```

This ensures that over many accruals, rounding bias cancels out rather than accumulating in one direction.

### Ceil Safety Clamp

`calculate_interest_with_rounding` includes a safety clamp (lines 96-106) that ensures `Ceil` mode never produces a lower integer interest than `Floor` mode due to integer division edge-cases. The clamp computes the floor-rounded result and forces the ceil result to be >= floor:

```rust
if mode == RoundingMode::Ceil {
    let (floor_rounded, _) = apply_rounding(full_division, remainder, denominator, RoundingMode::Floor);
    let floor_interest = floor_rounded / INTEREST_PRECISION;
    if final_interest < floor_interest {
        final_interest = floor_interest;
        final_remainder = 0;
    }
}
```

## Numeric Safety Properties

### Arithmetic Type

- **Primary type**: `i128` for all balances, rates, and interest results
- **Intermediate precision**: Multiplied by `INTEREST_PRECISION` (10^6) before division
- **NOT I256**: The original design note mentioned `I256` intermediates, but the production implementation uses `i128` with checked arithmetic throughout

### Overflow Protection

All mutations use checked arithmetic:

| Location | Protection | Behavior on Overflow |
|----------|------------|---------------------|
| `calculate_interest_with_rounding` | `checked_mul` chain | Returns `RoundingError::Overflow` |
| `accrue_interest` | Via `calculate_interest_with_rounding` | Returns `DebtError::Overflow` |
| `settle_accrual` | `checked_add` on principal | Returns `DebtError::Overflow` |
| `effective_debt` | `checked_add` on principal | Returns `DebtError::Overflow` |
| `borrow_amount` | `checked_add` for new principal | Returns `DebtError::Overflow` |
| `get_position` (health factor) | `checked_mul` then `unwrap_or(i128::MAX)` | Saturates to `i128::MAX` |

### Maximum Safe Inputs

The overflow boundary depends on the product:

```
borrowed_amount * elapsed_seconds * rate_bps * INTEREST_PRECISION < i128::MAX
```

For `rate_bps = 10_000` (100% APR, max configured):

```
borrowed_amount * elapsed_seconds < i128::MAX / (10_000 * 1_000_000)
borrowed_amount * elapsed_seconds < 1.7 * 10^30 / 10^10
borrowed_amount * elapsed_seconds < 1.7 * 10^20
```

For a typical loan of `1_000_000_000` (1 billion units):

```
elapsed_seconds < 1.7 * 10^20 / 1_000_000_000
elapsed_seconds < 1.7 * 10^11 seconds
elapsed_seconds < ~5,400 years
```

Tests verify overflow behavior at `u64::MAX` timestamps and `i128::MAX / 2` principal values (see `interest_drift_regression_test.rs:test_extreme_horizon_overflow_protection`).

## Long-Horizon / Extreme Scenarios Covered

- Multi-decade to centuries-scale timestamp jumps (including `u64::MAX` in lending tests)
- Maximum configured annual rate (10000 bps) for accrued-interest monotonicity checks
- Overflow boundary test where the last safe elapsed second succeeds and the next second returns overflow
- Extreme high-utilization + aggressive configuration + emergency adjustment still clamped to ceiling
- Extreme negative emergency adjustment still clamped to floor
- 24-month and 100-month accrual cycles with drift bounded to < 20 and < 50 units respectively

## Security Notes

- No test relies on unchecked casts for financial results
- Expected behavior under extreme inputs is deterministic:
  - Saturation in `lending` (via `unwrap_or(i128::MAX)`)
  - Explicit error returns via `DebtError::Overflow` and `RoundingError::Overflow`
- This prevents silent wraparound and protects debt/accounting invariants under adversarial time jumps and parameter settings
- Drift is tracked but not automatically corrected; reconciliation is available via `reconcile_debt_with_drift_correction` with configurable max drift tolerance

## Related Documentation

- [`docs/glossary.md`](../../docs/glossary.md) - Protocol terms, BPS scale, Health Factor definition
- [`docs/glossary.md#numeric-scales-summary`](../../docs/glossary.md#numeric-scales-summary) - Summary table of all numeric scales
- `contracts/lending/src/rounding_strategy.rs` - Constants and rounding implementation
- `contracts/lending/src/debt.rs` - Debt position management and accrual entry points
- `contracts/lending/src/interest_drift_regression_test.rs` - Long-horizon drift and overflow tests
