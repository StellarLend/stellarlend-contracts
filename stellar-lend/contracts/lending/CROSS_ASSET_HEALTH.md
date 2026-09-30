# Cross-Asset Health Factor — Formula Specification

This document is the canonical reference for the multi-asset health factor
aggregation performed by
[`compute_aggregate_health_factor`](src/cross_asset.rs).  It covers the
exact arithmetic, every scaling constant, rounding direction, the saturated
no-debt value, and a worked two-collateral two-debt example whose numbers are
independently verified by the doc-test in
[`cross_asset_health_doctest.rs`](src/cross_asset_health_doctest.rs).

See also:
- [`cross_asset.md`](cross_asset.md) — aggregation pipeline overview and
  isolation-mode rules.
- [`docs/CROSS_ASSET_RULES.md`](../../docs/CROSS_ASSET_RULES.md) — invariants,
  view guarantees, and security notes.
- [`CROSS_ASSET_HEALTH_PERF.md`](CROSS_ASSET_HEALTH_PERF.md) — read-budget
  rationale and storage-read benchmarks.

---

## 1. Scaling Constants

| Constant | Value | Location | Meaning |
|----------|-------|----------|---------|
| `PRICE_DIVISOR` | `10_000_000` | `cross_asset.rs` (module-private) | Oracle price scale: 1 USD = 10 000 000 raw price units |
| `HEALTH_FACTOR_SCALE` | `10_000` | `cross_asset.rs` (public) | 1.0× health factor expressed as an integer |
| `HEALTH_FACTOR_NO_DEBT` | `100_000_000` | `cross_asset.rs` (public) | Sentinel returned when outstanding debt is zero (10 000× scale) |
| `DEFAULT_APR_BPS` | `500` (5 %) | `debt.rs` (public) | Annual borrow rate applied by `effective_debt` when no dynamic rate is set |

**Price convention.** Every oracle `PriceRecord.price` is stored with
seven decimal places of precision:

```
$1.00  →  10_000_000
$0.50  →   5_000_000
$2.00  →  20_000_000
```

**Basis-points convention.** Risk parameters are expressed in basis points:
`10_000 bps = 100 %`.  For example, an 80 % liquidation threshold is stored as
`liquidation_threshold_bps = 8_000`.

---

## 2. Aggregation Formula

Given a user with **N** collateral assets and **M** debt assets:

```text
weighted_collateral = Σ_{i=1..N}  collateral_i × price_i × liquidation_threshold_bps_i

total_debt_value    = Σ_{j=1..M}  effective_debt_j × price_j

health_factor       = weighted_collateral / total_debt_value   (integer floor division)
```

Where:
- `collateral_i` — raw balance stored under `DataKey::CollateralAsset(user, asset_i)`.
- `price_i` — `PriceRecord.price` for `asset_i` (raw units, 7-decimal precision).
- `liquidation_threshold_bps_i` — `AssetParams.liquidation_threshold_bps` for `asset_i`.
- `effective_debt_j` — result of `debt::effective_debt(&position_j, now, rate_bps)`,
  which adds accrued interest to the stored principal at the current borrow rate.

### Why `PRICE_DIVISOR` cancels out

`weighted_collateral` and `total_debt_value` are both computed in identical
raw-price units (`amount × raw_price`).  Because the divisor is the same on
both sides of the fraction it cancels exactly:

```text
health_factor = (Σ  amount_i × price_i × threshold_bps_i) / (Σ  debt_j × price_j)
              ≡ (Σ  amount_i × (price_i / PRICE_DIVISOR) × threshold_bps_i)
                /
                (Σ  debt_j  × (price_j / PRICE_DIVISOR))
```

`PRICE_DIVISOR` is therefore **not** applied inside
`compute_aggregate_health_factor`.  It *is* applied in `get_cross_position_value`
and `get_cross_debt_value` (which return human-readable USD-denominated totals),
but those functions serve display purposes and do not feed into the health check.

### Relationship to `HEALTH_FACTOR_SCALE`

The formula is designed so that:

```
health_factor == HEALTH_FACTOR_SCALE  (10_000)
  ⟺  weighted_collateral == total_debt_value
  ⟺  the position is exactly at the liquidation boundary (HF = 1.0)
```

A position with `health_factor < HEALTH_FACTOR_SCALE` is under-collateralised
and may be liquidated.

---

## 3. No-Debt Saturated Value

```text
if debt_assets.is_empty()         →  Ok(HEALTH_FACTOR_NO_DEBT)   // fast path
if total_debt_value == 0          →  Ok(HEALTH_FACTOR_NO_DEBT)   // all debts accrued to zero
```

`HEALTH_FACTOR_NO_DEBT = 100_000_000` — ten-thousand times `HEALTH_FACTOR_SCALE`.
Callers should treat **any value ≥ HEALTH_FACTOR_NO_DEBT** as "unconditionally
healthy; skip liquidation checks".

Rationale: returning `i128::MAX` would risk overflow in callers that try to
compare or scale the value; the sentinel `100_000_000` is large enough to be
unambiguous while being arithmetically safe.

---

## 4. Rounding Direction

All divisions in the health-factor path use **integer floor (truncation toward
zero)**.  For positive operands this means the result is rounded *down*.

- `weighted_collateral / total_debt_value` floors → the health factor is always
  **at most** the mathematical real-valued ratio.
- A borrow is only permitted when `health_factor >= HEALTH_FACTOR_SCALE`,
  i.e., the floor-rounded result must be ≥ 1.0.  This means borrows at the
  exact boundary are permitted; only a floor-rounded value strictly below
  `HEALTH_FACTOR_SCALE` triggers `HealthFactorTooLow`.

The conservative direction (smaller HF after rounding) ensures the protocol
never over-values collateral relative to debt.

---

## 5. Worked Example — Two Collateral, Two Debt

This example is the exact scenario reproduced by `cross_asset_health_doctest.rs`.
All arithmetic is integer-exact; no rounding occurs in these particular inputs.

### Position Setup

| Role | Asset | Amount | Oracle Price | Liquidation Threshold |
|------|-------|--------|--------------|----------------------|
| Collateral | XLM  | 2 000 units | 5 000 000 (= $0.50) | 7 500 bps (75 %) |
| Collateral | USDC | 1 000 units | 10 000 000 (= $1.00) | 9 000 bps (90 %) |
| Debt | XLM  | 500 units | 5 000 000 (= $0.50) | — |
| Debt | USDC | 200 units | 10 000 000 (= $1.00) | — |

Interest accrual is suppressed in the doc-test by querying at the same
timestamp as the borrow (`effective_debt = principal`, no elapsed time).

### Step-by-Step Calculation

**Weighted collateral (numerator)**

```text
XLM  collateral: 2_000 × 5_000_000 × 7_500
               = 10_000_000_000  ×  7_500
               = 75_000_000_000_000

USDC collateral: 1_000 × 10_000_000 × 9_000
               = 10_000_000_000  ×  9_000
               = 90_000_000_000_000

weighted_collateral = 75_000_000_000_000
                    + 90_000_000_000_000
                    = 165_000_000_000_000
```

**Total debt value (denominator)**

```text
XLM  debt: 500 × 5_000_000 = 2_500_000_000
USDC debt: 200 × 10_000_000 = 2_000_000_000

total_debt_value = 2_500_000_000
                 + 2_000_000_000
                 = 4_500_000_000
```

**Health factor**

```text
health_factor = 165_000_000_000_000 / 4_500_000_000
              = 36_666                    (integer floor; exact = 36_666.666…)
```

`36_666 > HEALTH_FACTOR_SCALE (10_000)` → position is **healthy** ✓

**Human interpretation:** HF = 36 666 / 10 000 ≈ **3.67×** over-collateralised.

### Verification

```
assert_eq!(compute_aggregate_health_factor(&env, &user), Ok(36_666));
```

This assertion is executed by the doc-test in
`cross_asset_health_doctest.rs::test_two_collateral_two_debt_health_factor`.

---

## 6. Additional Edge Cases

### 6.1 Single Collateral, Single Debt (exact boundary)

```text
Collateral: 10_000 units, price = 10_000_000, LT = 8_000 bps
Debt:        8_000 units, price = 10_000_000

weighted_collateral = 10_000 × 10_000_000 × 8_000 = 800_000_000_000_000
total_debt_value    =  8_000 × 10_000_000           =  80_000_000_000

health_factor = 800_000_000_000_000 / 80_000_000_000 = 10_000
             = HEALTH_FACTOR_SCALE  (boundary: healthy, not liquidatable)
```

The floor division here is exact (no remainder), so no rounding occurs.
A debt of 8_001 would yield `799_999_200_000_000 / 80_010_000_000 = 9_998 < 10_000`
→ borrow rejected.

### 6.2 No Debt — Saturated Value

```text
Collateral: any non-zero amount
Debt:       none (empty debt list)

health_factor = HEALTH_FACTOR_NO_DEBT = 100_000_000
```

Both the fast-path (`debt_assets.is_empty()`) and the late-path
(`total_debt_value == 0` after iterating) return this sentinel.

### 6.3 Rounding Direction (floor)

When the division is not exact the result rounds **down**:

```text
weighted_collateral = 165_000_000_000_000    (from §5)
total_debt_value    =   4_500_000_000 + 1    (add 1 to force a remainder)
                    =   4_500_000_001

health_factor = 165_000_000_000_000 / 4_500_000_001
              = 36_666   (not 36_667)
```

The floor keeps the protocol on the safe side: the computed health factor is
never greater than the true mathematical ratio.

### 6.4 Zero-Balance Collateral or Debt (skipped)

Assets with a zero collateral balance or zero effective debt are silently
skipped in their respective loop (`if amount == 0 { continue }` / `if debt == 0 { continue }`).
They do not contribute to either the numerator or denominator, and they do not
trigger price-feed lookups beyond what was already cached.

### 6.5 Price Cache (overlapping assets)

If the same asset address appears in both the collateral list and the debt list
(e.g., a user borrows the same asset they also deposited as collateral), its
`PriceRecord` is fetched from persistent storage **once** during the collateral
loop and served from a local `Map<Address, PriceRecord>` price cache during the
debt loop.  The health factor value is unaffected; only the storage-read count
changes.

---

## 7. Overflow Protection

Every multiplication uses `checked_mul` and every addition uses `checked_add`.
Any overflow returns `Err(LendingError::Overflow)`.

Practical limit: with `amount ≤ i128::MAX / (price × threshold_bps)` the
intermediate products fit.  At realistic values
(price ≤ 10^15, amount ≤ 10^18, threshold ≤ 10_000) the intermediate
`amount × price × threshold` ≤ 10^36, which slightly exceeds `i128::MAX ≈ 1.7×10^38`
at extreme edge cases — protocol-level deposit caps prevent reaching these
extremes in practice.

---

## 8. Function Signatures

```rust
// src/cross_asset.rs

/// Sentinel health factor returned when a user has zero outstanding debt.
pub const HEALTH_FACTOR_NO_DEBT: i128 = 100_000_000;

/// Baseline scale for health-factor comparisons (1.0 = HEALTH_FACTOR_SCALE).
pub const HEALTH_FACTOR_SCALE: i128 = 10_000;

/// Compute the aggregate health factor across all collateral and debt assets.
///
/// Returns Ok(HEALTH_FACTOR_NO_DEBT) when debt is zero, Ok(health_factor)
/// otherwise, or Err(LendingError) on missing params/price/overflow.
pub fn compute_aggregate_health_factor(
    env: &Env,
    user: &Address,
) -> Result<i128, LendingError>;

/// Return the total USD value of a user's cross-asset collateral positions.
/// Divides each (amount × price) by PRICE_DIVISOR = 10_000_000.
pub fn get_cross_position_value(env: &Env, user: &Address) -> Result<i128, LendingError>;

/// Return the total USD value of a user's cross-asset debt positions.
/// Divides each (effective_debt × price) by PRICE_DIVISOR = 10_000_000.
pub fn get_cross_debt_value(env: &Env, user: &Address) -> Result<i128, LendingError>;
```

---

## 9. Consistency Between `compute_aggregate_health_factor` and the `get_cross_*` View Functions

`compute_aggregate_health_factor` **does not** divide by `PRICE_DIVISOR`.
`get_cross_position_value` and `get_cross_debt_value` **do** divide by `PRICE_DIVISOR`.

This is intentional and consistent:

- For the HF ratio the divisor cancels, so omitting it avoids unnecessary
  divisions and preserves full integer precision.
- For the USD-denominated view functions the divisor converts raw oracle units
  into a human-readable USD equivalent.

A manual sanity check using the worked example from §5:

```
get_cross_position_value  (USDC-denominated):
  XLM:  2_000 × 5_000_000  / 10_000_000 = 1_000 USDC
  USDC: 1_000 × 10_000_000 / 10_000_000 = 1_000 USDC
  total_collateral_usd = 2_000 USDC

get_cross_debt_value  (USDC-denominated):
  XLM:  500 × 5_000_000  / 10_000_000 = 250 USDC
  USDC: 200 × 10_000_000 / 10_000_000 = 200 USDC
  total_debt_usd = 450 USDC

Manual weighted HF check (using LT from §5):
  weighted_collateral_usd = 1_000 × 7_500/10_000 + 1_000 × 9_000/10_000
                          = 750 + 900 = 1_650
  health_factor           = 1_650 × 10_000 / 450
                          = 36_666  ✓  (matches §5)
```

The two approaches agree to the floor-division limit.
