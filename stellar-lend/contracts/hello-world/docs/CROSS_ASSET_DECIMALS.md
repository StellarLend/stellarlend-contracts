# Cross-Asset Oracle Decimal Normalization

Fixes issue #1122.

## Problem

Assets registered in the protocol can have oracle price feeds that use different
decimal scales (e.g. a USD stablecoin feed may report prices with 6 decimal
places while an ETH feed uses 18). If these raw prices are summed directly when
computing a user's position, the result is off by orders of magnitude — a
silent solvency miscalculation.

Example of the broken behaviour:

| Asset | Raw price | Decimals | "Value" (broken) |
|-------|-----------|----------|------------------|
| USDC  | 1 000 000 | 6        | 1 000 000        |
| ETH   | 2 000 000 000 000 000 000 | 18 | 2 000 000 000 000 000 000 |
| **Sum** | — | — | **≈ 2 × 10¹⁸** (wrong) |

The ETH position dominates purely because of its decimal scale, not its dollar
value.

---

## Solution

Every asset stores a `price_decimals: u32` field in `AssetConfig`.  Before any
value aggregation in `get_user_position_summary`, each raw price is normalised
to a shared **internal scale** of 10¹⁸ decimal places.

### Formula

```
if asset_decimals == INTERNAL_DECIMALS (18):
    normalised_price = raw_price

if asset_decimals < INTERNAL_DECIMALS:
    normalised_price = raw_price × 10^(INTERNAL_DECIMALS − asset_decimals)

if asset_decimals > INTERNAL_DECIMALS:
    normalised_price = raw_price ÷ 10^(asset_decimals − INTERNAL_DECIMALS)
```

### Rounding direction

| Value type  | Rounding | Rationale |
|-------------|----------|-----------|
| Collateral  | Floor    | Understates collateral → conservative for the protocol |
| Debt        | Ceiling  | Overstates debt → conservative for the protocol |

### Value computation

After normalisation all dollar-values are computed in the 10¹⁸ internal scale:

```
collateral_value_i = supplied_i × normalised_price_i  ÷  10¹⁸   (floor)
debt_value_i       = borrowed_i × normalised_price_i  ÷  10¹⁸   (ceiling)

total_collateral   = Σ collateral_value_i
total_debt         = Σ debt_value_i
borrow_capacity    = Σ (collateral_value_i × collateral_factor_i ÷ 10 000)

is_healthy = (total_debt == 0) OR (borrow_capacity >= total_debt)
```

---

## Worked Example

| Asset | Amount | Raw price | price_decimals | Normalised price (10¹⁸) | Dollar value |
|-------|--------|-----------|----------------|------------------------|--------------|
| USDC  | 100    | 1 000 000 | 6              | 1 000 000 000 000 000 000 | $100 |
| ETH   | 1      | 1 000 000 000 000 000 000 | 18 | 1 000 000 000 000 000 000 | $1 |
| **Total collateral** | | | | | **$101** |

USDC `collateral_factor` = 9000 (90 %), ETH `collateral_factor` = 7500 (75 %):

```
borrow_capacity = (100 × 9000 / 10 000) + (1 × 7500 / 10 000)
               = 90 + 0   (integer division)
               = 90
```

---

## Overflow Guard

All scaling multiplications use **checked arithmetic** (`checked_mul`).  If
a raw price would overflow `i128` after scaling, `normalize_price` returns
`None` and the caller propagates `CrossAssetError::Overflow`.  This prevents
silent wrap-around.

The maximum `price_decimals` accepted at asset registration is **38** (matching
the practical limit of `i128` arithmetic); values above 38 are rejected with
`CrossAssetError::InvalidDecimals`.

---

## No Regression Guarantee

When all assets share the same `price_decimals` value the normalisation factor
is 1 (multiply or divide by `10^0 = 1`), so the result is bit-identical to the
previous behaviour.  The regression test `test_no_regression_same_decimals`
validates this.

---

## Test Coverage

Tests live in `src/cross_asset_decimals_test.rs`:

| Test | What it covers |
|------|----------------|
| `test_normalize_same_decimals` | Identity when decimals == INTERNAL |
| `test_normalize_6_to_18_decimals` | Upscaling 6 → 18 |
| `test_normalize_8_to_18_decimals` | Upscaling 8 → 18 |
| `test_normalize_18_to_6_floor_vs_ceil` | Downscaling, floor vs ceiling |
| `test_normalize_exact_multiple_no_rounding_diff` | No rounding diff on exact multiples |
| `test_normalize_overflow_guard` | `i128` overflow returns `None` |
| `test_normalize_zero_price` | Zero price handled correctly |
| `test_position_summary_equal_usd_different_decimals` | Mixed decimals sum to same USD value |
| `test_borrow_health_check_mixed_decimals` | Borrow health check across mixed decimals |
| `test_no_regression_same_decimals` | Same decimals → unchanged semantics |
| `test_invalid_decimals_rejected` | `price_decimals > 38` rejected at init |
| `test_price_update_reflected_in_summary` | Price updates flow into summaries |
