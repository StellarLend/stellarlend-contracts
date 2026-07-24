# Per-Asset Collateral Factor (LTV) Tiering

Closes #1121.

## Problem

Before this change, every registered asset contributed to a user's borrow
capacity by the same uniform rule: `capacity = collateral_value × factor / 10_000`.
The factor lived on the asset config but was effectively a constant across
the protocol — every asset supplied collateral against the same protocol-
level appetite. In practice this means a volatile long-tail asset backs the
same borrow capacity per dollar of value as a blue-chip stablecoin, which
underprices the risk of the riskier collateral.

## Solution

`AssetConfig` now exposes a `collateral_factor_bps` field per asset. The
field name makes the basis-point unit explicit at the contract boundary,
and is bounded to `[MIN_COLLATERAL_FACTOR_BPS, MAX_COLLATERAL_FACTOR_BPS]`
which is `[0, 10_000]`. Each asset's contribution to borrow capacity is
now multiplied by its *own* factor during aggregation.

The aggregate borrow capacity formula becomes:

```
borrow_capacity = Σ_i (collateral_value_i × collateral_factor_bps_i / 10_000)
```

where `collateral_value_i` is the asset's value normalised to the shared
18-decimal internal scale (see `CROSS_ASSET_DECIMALS.md`).

## Bounds & Validation

| Bound | Value | Meaning |
|-------|-------|---------|
| `MIN_COLLATERAL_FACTOR_BPS` | `0` | Asset contributes zero borrow capacity. Useful for assets that should be recognised as collateral (so users can supply them) but should never underwrite debt. |
| `MAX_COLLATERAL_FACTOR_BPS` | `10_000` | 100 % LTV. Matches the pre-tier behaviour: a full-factor asset backs its full value. No regression for assets registered at the maximum. |

`initialize_asset`, `update_asset_config`, and any caller that mutates the
field must reject out-of-range values with
`CrossAssetError::InvalidCollateralFactor`.

Validation lives in two places:
1. `cross_asset::initialize_asset` — rejects invalid
   `collateral_factor_bps` on the `AssetConfig` it stores.
2. `cross_asset::update_asset_config` — rejects invalid factor changes
   whenever `Some(_)`. Passing `None` is a no-op.

## Why "factor-weighted contribution" instead of "discounted total"?

The aggregate position summary preserves two distinct quantities:

| Field | What's in it | Use case |
|-------|--------------|----------|
| `total_collateral_value` | Raw (un-weighted) sum of collateral values. | Display "what the user has on the books." |
| `borrow_capacity` | Sum of factor-weighted contributions. | The LTV check used by `cross_asset_borrow` to gate a borrow. |

Discounting `total_collateral_value` directly would conflate two different
questions (display value vs risk-weighted capacity) and would surface
existing factor choices to off-chain consumers that aggregate raw value
independently. Keeping the two fields distinct lets view clients render
either number, while the borrow gate continues to use `borrow_capacity`.

## Worked Example

A user holds:

| Asset | Supplied | Raw price | price_decimals | Normalised price (10¹⁸) | `collateral_factor_bps` | Asset value (raw) |
|-------|----------|-----------|----------------|------------------------|-----------------------|-------------------|
| USDC  | 1 000    | 1 000 000 | 6              | 10¹⁸                   | 9 000  (90 %)         | 1 000 × 1 = $1 000 |
| ETH   | 100      | 2 000 000 000 000 000 000 | 18 | 2 × 10¹⁸ | 7 500 (75 %) | 100 × 2 = $200 |
| LONG  | 500      | 100 000 000 | 8             | 10¹⁸                   | 4 000 (40 %)          | 500 × 1 = $500 |

### Aggregation

```
total_collateral_value
    = 1_000 + 200 + 500
    = 1_700

borrow_capacity
    = (1_000 × 9000 + 200 × 7500 + 500 × 4000) / 10_000
    = (9_000_000 + 1_500_000 + 2_000_000) / 10_000
    = 12_500_000 / 10_000
    = 1_250
```

### Borrow gate

If the user attempts to borrow 1 250 units (exactly at capacity):
- `borrow_capacity (1250) >= total_debt (1250)` → `is_healthy == 1`.
  Borrow succeeds.

If the user attempts to borrow 1 251 units (one unit above capacity):
- `borrow_capacity (1250) < total_debt (1251)` → `is_healthy == 0`.
  `cross_asset_borrow` rolls back and returns
  `CrossAssetError::InsufficientCollateral`.

### Effect of zero factor

If a single asset is configured at `0` bps, its contribution to
`borrow_capacity` is zero. The asset still appears in
`total_collateral_value`, but it does not underwrite any debt. This is a
useful configuration for newly-listed long-tail assets: they can be
supplied and withdrawn (so the protocol can hold them, transfer them in
and out, etc.) but cannot back borrows until governance sets a positive
factor.

## Overflow Safety

`borrow_capacity = val × factor_bps / 10_000` uses `checked_mul`. The factor
is bounded at registration to `[0, 10_000]`, so the multiplication cannot
amplify `val` beyond 10× — the worst case (10_000 bps on the largest
collateral value `i128::MAX / 10`) would still be representable in i128
because `val` itself is already the result of a checked `* / 10^18` step.
`CrossAssetError::Overflow` propagates if any step exceeds `i128::MAX`.

## No Regression Guarantee

When every asset is registered with `collateral_factor_bps = 10_000`, the
factor becomes a constant multiplier of 1, so `borrow_capacity` simplifies
back to `Σ collateral_value_i` — exactly the pre-tier behaviour. The
regression test `test_full_factor_no_regression` in
`src/cross_asset_ltv_test.rs` exercises this end-to-end through
`cross_asset_borrow` and verifies that a borrow at the previous boundary
succeeds and a borrow one unit above fails.

## Test Coverage

Tests live in `src/cross_asset_ltv_test.rs`:

| Test | What it covers |
|------|----------------|
| `test_init_rejects_negative_factor` | `factor_bps < 0` rejected at init |
| `test_init_rejects_factor_above_max` | `factor_bps > 10_000` rejected at init |
| `test_init_accepts_zero_factor_boundary` | Boundary 0 accepted |
| `test_init_accepts_max_factor_boundary` | Boundary 10_000 accepted |
| `test_update_rejects_out_of_range_factor` | Update rejects out-of-range, no partial mutation |
| `test_update_factor_takes_effect_immediately` | Update propagates to next summary read |
| `test_full_factor_no_regression` | Full-factor asset preserves prior LTV |
| `test_zero_factor_asset_no_capacity` | Zero-factor asset contributes 0 capacity |
| `test_mixed_factor_portfolio` | Mixed blue-chip/long-tail weighted sum |
| `test_factor_weighting_arithmetic_reference` | Independent arithmetic confirms formula |
| `test_factor_does_not_change_total_collateral_value` | Factor leaves raw value untouched |
| `test_factor_50bps_small_collateral` | Integer-division floor edge case |

## Storage Layout Impact

The `AssetConfig` struct gained a renamed field (`collateral_factor` →
`collateral_factor_bps`). This changes the on-disk layout of the
`Config(AssetKey)` storage slot. Document a re-deploy / migration in
release notes before this ships. New fields are not added; the layout
remains the same size.
