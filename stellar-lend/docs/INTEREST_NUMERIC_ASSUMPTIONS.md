# Interest Numeric Assumptions and Safety Limits

This note documents the canonical numeric constants, scaling factors, rounding modes, and overflow/underflow protections used across the StellarLend interest accrual system.

## Scope

- `contracts/lending/src/debt.rs` (`accrue_interest`, `accrue_interest_split`, `settle_accrual`, `settle_accrual_split`, `effective_debt`, `effective_supply_rate`, `borrow_amount`, `repay_amount`)
- `contracts/lending/src/math.rs` (`split_interest_by_reserve_factor`, `compute_supply_rate`, `compute_borrow_rate`, `compute_utilization`)
- `contracts/lending/src/rounding_strategy.rs` (`calculate_interest_with_rounding`, `apply_rounding`, `reconcile_debt_with_drift_correction`)
- `contracts/lending/src/lib.rs` (`get_position` health factor calculation, liquidation math)

## Canonical Constants

All constants are defined in `contracts/lending/src/rounding_strategy.rs`:

| Constant | Type | Value | Purpose |
|----------|------|-------|---------|
| `INTEREST_PRECISION` | `i128` | `1_000_000` (10^6) | Intermediate fractional precision for interest math |
| `BASIS_POINTS_SCALE` | `i128` | `10_000` | Denominator for basis-points (100% = 10_000 bps) |
| `SECONDS_PER_YEAR` | `u64` | `31_536_000` (365 * 24 * 60 * 60) | Time denominator for APR calculations |

In `contracts/lending/src/debt.rs`:

| Constant | Type | Value | Purpose |
|----------|------|-------|---------|
| `DEFAULT_APR_BPS` | `i128` | `500` | Default annual percentage rate (5%) |
| `DEFAULT_RESERVE_FACTOR_BPS` | `u32` | `0` | Default reserve factor (0% — all interest to depositors) |

### Important: This Protocol Does NOT Use SCALE_18

Some DeFi protocols use `SCALE_18 = 10^18` for fixed-point arithmetic. **This protocol uses `INTEREST_PRECISION = 10^6`** for intermediate interest calculations. The 10^6 scale provides 6 decimal places of fractional precision, which is sufficient for sub-cent accuracy on typical loan sizes while keeping intermediate products within `i128` bounds.

### Combined Denominator

The full denominator used in interest calculations is:

```
DENOMINATOR = SECONDS_PER_YEAR * BASIS_POINTS_SCALE
            = 31_536_000 * 10_000
            = 315_360_000_000
```

---

## Interest Calculation Formula

The core formula (from `calculate_interest_with_rounding`) is:

```
numerator   = borrowed_amount * elapsed_seconds * rate_bps * INTEREST_PRECISION
denominator = SECONDS_PER_YEAR * BASIS_POINTS_SCALE  (= 315_360_000_000)

raw_result  = numerator / denominator        (integer division)
remainder   = numerator % denominator        (fractional remainder)

final_interest = raw_result / INTEREST_PRECISION   (back-convert from precision scale)
```

---

## Utilization-Driven Supply Rate with Reserve-Factor Split

### Overview

Borrower interest is split deterministically between depositors and the protocol
reserve at the point of accrual. No interest is created or destroyed — every
basis point a borrower pays lands on exactly one side.

### Borrow Rate (utilization-driven)

The borrow APR comes from the two-slope jump-rate model in `rate_model.rs`:

```
if utilization <= kink:
    borrow_rate = base_rate + (utilization × multiplier) / 10_000

if utilization > kink:
    borrow_rate = base_rate
                + (kink × multiplier) / 10_000
                + ((utilization − kink) × jump_multiplier) / 10_000
```

Clamped to `[rate_floor_bps, rate_ceiling_bps]`.

### Supply Rate Formula

The depositor supply APR is derived from the borrow rate and utilization after
applying the reserve factor:

```
supply_rate_bps = borrow_rate_bps
                × utilization_bps / 10_000
                × (10_000 − reserve_factor_bps) / 10_000
```

This is implemented in both `debt::effective_supply_rate` and
`math::compute_supply_rate`. Both functions produce identical results for the
same inputs (cross-checked by `supply_rate_agrees_with_math_compute_supply_rate`
in the test suite).

**When `reserve_factor_bps == 0`** (the default) the formula simplifies to:

```
supply_rate_bps = borrow_rate_bps × utilization_bps / 10_000
```

which is the utilization-weighted borrow rate with no protocol cut — identical
to the previous behaviour before the reserve factor was introduced.

### Interest Split Formula

At accrual time, gross borrower interest is split by `split_interest_by_reserve_factor`:

```
reserve_cut      = floor(total_interest × reserve_factor_bps / 10_000)
depositor_yield  = total_interest − reserve_cut
```

The depositor share is computed as a complement (subtraction from the total)
rather than a second multiplication. This guarantees:

```
depositor_yield + reserve_cut == total_interest   (exact, no rounding gap)
```

Integer division floors the reserve cut, so any fractional unit that cannot be
divided exactly remains with the depositor. The protocol never takes more than
its exact share.

### No-Leakage Invariant

For every call to `accrue_interest_split` or `settle_accrual_split`:

```
split.depositor_yield + split.reserve_cut == split.total_interest
```

This is verified exhaustively in `supply_rate_split_test::split_no_leakage_invariant_exhaustive`
across all combinations of total interest and reserve factor from 0 to 10 000 bps.

### Entry Points

| Function | Location | Purpose |
|---|---|---|
| `accrue_interest_split(principal, elapsed, rate_bps, reserve_factor_bps)` | `debt.rs` | Compute gross interest and its depositor/reserve split |
| `settle_accrual_split(position, now, rate_bps, reserve_factor_bps)` | `debt.rs` | Settle interest into principal and return the split |
| `effective_supply_rate(borrow_rate_bps, utilization_bps, reserve_factor_bps)` | `debt.rs` | Depositor APR in basis points |
| `split_interest_by_reserve_factor(total_interest, reserve_factor_bps)` | `math.rs` | Pure-math split (no Env dependency, fuzzable) |
| `compute_supply_rate(borrow_rate_bps, utilization_bps, reserve_factor_bps)` | `math.rs` | Supply APR — same formula as `effective_supply_rate` |

---

## Worked Examples

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

With **Bankers rounding** (the default in `debt.rs`), the fractional part
`172_160_000_000 / 315_360_000_000 ≈ 0.546` is greater than 0.5, so the
raw_result rounds up to 159:

```
final_interest = 159 / 1_000_000 = 0  (still 0 whole units)
```

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

The exact interest for 1 month at 5% on $1,000 is $4.167. After rounding and
back-conversion, the protocol accrues 4 whole units.

### Worked Example 4: Reserve-factor split — $100,000 at 5% APR, 20% reserve, 1 year

```
Step 1 — gross borrow interest (same as Example 2 at 100× scale):
  total_interest = 5_000

Step 2 — reserve cut (floor division):
  reserve_cut = floor(5_000 * 2_000 / 10_000)
              = floor(1_000_000 / 10_000)
              = 1_000

Step 3 — depositor yield (complement):
  depositor_yield = 5_000 − 1_000 = 4_000

Verification: 4_000 + 1_000 == 5_000  ✓
```

Depositors earn an effective supply APR of:

```
supply_rate = 500 * 10_000 / 10_000 * 8_000 / 10_000
            = 500 * 0.8
            = 400 bps  (4%)
```

i.e. 80% of the borrow rate at 100% utilization (the 20% reserve factor accounts
for the other 20%).

### Worked Example 5: Supply APR at 50% utilization, 20% reserve

```
borrow_rate_bps    = 500   (5% APR)
utilization_bps    = 5_000 (50%)
reserve_factor_bps = 2_000 (20%)

supply_rate = 500 * 5_000 / 10_000 * (10_000 − 2_000) / 10_000
           = 500 * 5_000 / 10_000 * 8_000 / 10_000
           = 250 * 8_000 / 10_000
           = 200 bps  (2%)
```

Confirmed by `supply_rate_worked_example_50pct_util_20pct_reserve` in the test suite.

---

## Basis Points (BPS) Conversions

The protocol uses basis points throughout for rates, thresholds, and factors:

| BPS Value | Percentage | Usage |
|-----------|------------|-------|
| `10_000` | 100% | Full utilization, max rate ceiling, 100% reserve |
| `5_000` | 50% | Close factor (liquidation) |
| `2_000` | 20% | Example reserve factor |
| `1_000` | 10% | Liquidation incentive bonus; example reserve factor |
| `500` | 5% | Default APR |
| `100` | 1% | Max drift tolerance example |
| `8_000` | 80% | Liquidation threshold (health factor base); default kink utilization |

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

The health factor formula (from `lib.rs`):
```
health_factor = (collateral * 8000) / debt
```
Where `8000` is the `LIQUIDATION_THRESHOLD` in BPS (80%).

---

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
| Debt accrual (`accrue_interest`) | `Bankers` | Nearest, ties to even | Minimises cumulative drift over many accruals |
| Reserve cut (`split_interest_by_reserve_factor`) | Floor (integer `/`) | Down (toward zero) | Protocol never takes more than exact share |
| Health factor calculation | Truncate (integer division) | Down (toward zero) | Conservative: overestimates risk |
| Liquidation seized collateral | Truncate (integer division) | Down | Protocol-safe: never seizes more than owed |
| Flash loan fee | Truncate (integer division) | Down | Borrower-safe: fee never exceeds exact amount |
| Close factor repayment cap | Truncate (integer division) | Down | Borrower-safe: caps repayment conservatively |

### Bankers Rounding Detail

Bankers rounding (`apply_rounding` in `rounding_strategy.rs`):

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

This ensures that over many accruals, rounding bias cancels out rather than
accumulating in one direction.

---

## Numeric Safety Properties

### Arithmetic Type

- **Primary type**: `i128` for all balances, rates, and interest results
- **Intermediate precision**: Multiplied by `INTEREST_PRECISION` (10^6) before division
- **NOT I256**: The production implementation uses `i128` with checked arithmetic throughout

### Overflow Protection

All mutations use checked arithmetic:

| Location | Protection | Behavior on Overflow |
|----------|------------|---------------------|
| `calculate_interest_with_rounding` | `checked_mul` chain | Returns `RoundingError::Overflow` |
| `accrue_interest` | Via `calculate_interest_with_rounding` | Returns `DebtError::Overflow` |
| `settle_accrual` | `checked_add` on principal | Returns `DebtError::Overflow` |
| `settle_accrual_split` | `checked_add` on principal | Returns `DebtError::Overflow` |
| `effective_debt` | `checked_add` on principal | Returns `DebtError::Overflow` |
| `effective_supply_rate` | `checked_mul` / `checked_div` / `checked_sub` | Returns `DebtError::Overflow` |
| `split_interest_by_reserve_factor` | `checked_mul` / `checked_div` / `checked_sub` | Returns `MathError::Overflow` / `MathError::OutOfRange` |
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

---

## Long-Horizon / Extreme Scenarios Covered

- Multi-decade to centuries-scale timestamp jumps (including `u64::MAX` in lending tests)
- Maximum configured annual rate (10000 bps) for accrued-interest monotonicity checks
- Overflow boundary test where the last safe elapsed second succeeds and the next second returns overflow
- Extreme high-utilization + aggressive configuration + emergency adjustment still clamped to ceiling
- 24-month and 100-month accrual cycles with drift bounded to < 20 and < 50 units respectively
- Reserve factor at 0%, 100%, and all intermediate values — split invariant verified exhaustively

## Security Notes

- No test relies on unchecked casts for financial results
- Expected behavior under extreme inputs is deterministic:
  - Saturation in `lending` (via `unwrap_or(i128::MAX)`)
  - Explicit error returns via `DebtError::Overflow` and `RoundingError::Overflow`
- This prevents silent wraparound and protects debt/accounting invariants under adversarial time jumps and parameter settings
- The no-leakage invariant (`depositor_yield + reserve_cut == total_interest`) is enforced by using subtraction for the depositor share rather than a second multiplication, eliminating the possibility of rounding creating or destroying units
- Drift is tracked but not automatically corrected; reconciliation is available via `reconcile_debt_with_drift_correction` with configurable max drift tolerance

## Related Documentation

- `contracts/lending/src/rounding_strategy.rs` - Constants and rounding implementation
- `contracts/lending/src/debt.rs` - Debt position management, accrual, and interest-split entry points
- `contracts/lending/src/math.rs` - Pure-math helpers including `split_interest_by_reserve_factor`
- `contracts/lending/src/supply_rate_split_test.rs` - Test suite for the reserve-factor split feature
- `contracts/lending/src/interest_drift_regression_test.rs` - Long-horizon drift and overflow tests
- [`docs/INTEREST_ROUNDING_FIX.md`](INTEREST_ROUNDING_FIX.md) - Rounding fix history
