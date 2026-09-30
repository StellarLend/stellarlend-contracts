# Health Factor Invariants

## Rationale

`compute_health_factor(collateral_value, debt_value, liquidation_threshold_bps)` is the pure arithmetic guardrail that decides how much risk a lending position carries. The function returns a fixed-point health factor scaled by `SCALE` using:

```text
weighted_collateral = collateral_value * liquidation_threshold_bps / BPS_SCALE
health_factor       = weighted_collateral * SCALE / debt_value
```

The property suite in `src/health_factor_proptest.rs` protects the invariants that reviewers and protocol users rely on:

- **Collateral monotonicity:** for a fixed debt and threshold, increasing collateral must never lower health factor.
- **Debt inverse monotonicity:** for a fixed collateral and threshold, increasing debt must never raise health factor.
- **No-debt saturation:** a position with zero debt is treated as infinitely healthy and returns `i128::MAX`.
- **Typed arithmetic failures:** overflowing intermediate products return `MathError::Overflow` instead of panicking.
- **Boundary correctness:** `0` bps produces zero weighted collateral for indebted positions, `BPS_SCALE` bps uses the full collateral value, and thresholds above `BPS_SCALE` return `MathError::OutOfRange`.

The proptests use a fixed seed and bounded case count so CI and reviewer machines exercise deterministic input streams while still covering a broad randomized surface.

## Worked example

Given:

```text
collateral_value          = 1_000_000_000
 debt_value              =   500_000_000
liquidation_threshold_bps = 8_000       # 80%
BPS_SCALE                 = 10_000
SCALE                     = 10_000_000
```

The function computes:

```text
weighted_collateral = 1_000_000_000 * 8_000 / 10_000
                    =   800_000_000

health_factor       = 800_000_000 * 10_000_000 / 500_000_000
                    = 16_000_000
```

Because `SCALE` is `10_000_000`, the returned value `16_000_000` represents a health factor of `1.6`. The position is above the liquidation boundary of `1.0`.

If collateral rises while debt and threshold are unchanged, the weighted collateral numerator can only increase, so the health factor must not decrease. If debt rises while collateral and threshold are unchanged, the denominator increases, so the health factor must not increase.

## Edge-case notes

- **Zero debt:** the function returns `Ok(i128::MAX)` before doing collateral multiplication. This avoids a division-by-zero path and documents the saturated no-debt sentinel.
- **Zero collateral with non-zero debt:** valid inputs return `Ok(0)` because the numerator is zero.
- **Zero threshold:** valid indebted positions return `Ok(0)` because all collateral is weighted to zero.
- **Full threshold (`BPS_SCALE`):** health factor is exactly `collateral_value * SCALE / debt_value` for safe inputs.
- **Threshold above `BPS_SCALE`:** returns `Err(MathError::OutOfRange)`.
- **Negative collateral or debt:** returns `Err(MathError::OutOfRange)`.
- **Overflow:** inputs such as `i128::MAX` collateral with a 100% threshold cannot be multiplied inside `i128`; the function returns `Err(MathError::Overflow)`.

Run the invariant suite with:

```bash
cargo test -p stellarlend-lending health_factor_proptest
```
