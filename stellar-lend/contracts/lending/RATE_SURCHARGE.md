# Emergency rate surcharge band

## Rationale

The base two-slope kink model (described in [`RATE_MODEL.md`](./RATE_MODEL.md)) clamps the borrow rate to a fixed ceiling at high utilization. At utilizations approaching 100 %, the existing curve flattens against the ceiling and provides **no additional price signal** to incentivise repayment or new deposits — a borrower near the ceiling sees no marginal cost increase for further utilisation.

The surcharge band solves this by adding a **third linear segment** that activates only above a configurable `surcharge_kink_bps`, steepening the curve dramatically in the high-utilisation regime. This creates extra pressure to:

- repay existing debt (reducing utilisation),
- supply additional deposits (diluting utilisation),
- or accept a higher cost of capital during a liquidity crunch.

## Formula

Let `u` be utilisation in basis points (`0 <= u <= 10_000`).

The base two-slope rate `r_raw` is computed exactly as in the existing model:

```
r_pre  = base_rate_bps + min(u, kink_utilization_bps) × multiplier_bps / BPS_DENOM
r_raw  = r_pre + max(0, u - kink_utilization_bps) × jump_multiplier_bps / BPS_DENOM
```

When `u > surcharge_kink_bps`, a surcharge term is added **on top** of `r_raw`:

```
surcharge = (u - surcharge_kink_bps) × surcharge_slope / BPS_DENOM
r_final   = clamp(r_raw + surcharge, rate_floor_bps, rate_ceiling_bps)
```

The surcharge is applied **before** the ceiling clamp so it can push the rate up to `rate_ceiling_bps` but never beyond.

## Default — disabled

The surcharge is **disabled by default**:

| Field | Default | Effect |
|---|---|---|
| `surcharge_kink_bps` | `10_000` (100 % util) | Kink sits at max utilisation |
| `surcharge_slope` | `0` | Zero slope = no surcharge |

With either `surcharge_slope = 0` or `surcharge_kink_bps >= 10000`, the surcharge band produces no change to the computed rate, exactly preserving the legacy two-slope behaviour.

## Configuration guidance

- Set `surcharge_kink_bps` to the utilisation level where the liquidity crunch response should begin (e.g. `9_500` = 95 %).
- Choose `surcharge_slope` to control how steeply the surcharge ramps. A slope of `50_000` means each 1 % of utilisation above the kink adds 5 % to the borrow rate.
- Ensure `rate_ceiling_bps` is high enough to accommodate the intended surcharge, or rely on the ceiling as a safety cap.

## Worked example

**Parameters:**

| Field | Value |
|---|---|
| `base_rate_bps` | 100 |
| `kink_utilization_bps` | 8_000 |
| `multiplier_bps` | 2_000 |
| `jump_multiplier_bps` | 10_000 |
| `rate_floor_bps` | 50 |
| `rate_ceiling_bps` | 10_000 |
| `surcharge_kink_bps` | **9_500** |
| `surcharge_slope` | **80_000** |

**Computed rates:**

| Utilisation | Base rate | Jump (above kink) | Raw rate | Surcharge | Final rate | Notes |
|---|---|---|---:|---:|---:|---:|
| 0 % (0) | 100 | — | 100 | 0 | 100 | Floor not hit |
| 50 % (5,000) | 100 | — | 1,100 | 0 | 1,100 | Below kink |
| 80 % (8,000) | 100 | 0 | 1,700 | 0 | 1,700 | At kink |
| 90 % (9,000) | 100 | 1,000 | 1,700 + 1,000 = 2,700 | 0 | 2,700 | Above kink, below surcharge kink |
| 95 % (9,500) | 100 | 1,500 | 3,200 | 0 | 3,200 | At surcharge kink |
| 96 % (9,600) | 100 | 1,600 | 3,300 | (100 × 80k) / 10k = **800** | **4,100** | Surcharge active |
| 98 % (9,800) | 100 | 1,800 | 3,600 | (300 × 80k) / 10k = **2,400** | **6,000** | |
| 100 % (10,000) | 100 | 2,000 | 3,700 | (500 × 80k) / 10k = **4,000** | **7,700** | Below ceiling |
| 100 % (10,000) | 100 | 2,000 | 3,700 | (500 × 80k) / 10k = **4,000** | **7,700** | |

Without the surcharge, the rate at 100 % utilisation would be `3,700 bps`. The surcharge adds **4,000 bps** — more than doubling the borrow rate — creating a strong incentive to repay or deposit.

## Edge-case notes

1. **Surcharge disabled (no-op):** When `surcharge_slope = 0`, the surcharge term is always zero regardless of `surcharge_kink_bps`. When `surcharge_kink_bps >= BPS_DENOM`, the condition `u > surcharge_kink_bps` is never satisfied for any valid utilisation. Both configurations preserve the legacy curve exactly.

2. **Kink at zero:** Setting `surcharge_kink_bps = 0` applies the surcharge at any positive utilisation. This is valid but aggressive — the curve remains monotonic because `surcharge_slope >= 0`.

3. **Ceiling clamp:** The surcharge is added **before** the `min(r, ceiling)` clamp. If the surcharge pushes the rate above the configured ceiling, the final rate is truncated at the ceiling. This prevents unbounded rates while still keeping the surcharge band effective up to the ceiling.

4. **Overflow protection:** All surcharge arithmetic uses `checked_mul` / `checked_div`. An overflow in the surcharge computation returns `Err(RateModelError::Overflow)`, just as with the base rate computation.

5. **Monotonicity:** The surcharge slope is non-negative. The surcharge band is only additive (`r_raw + surcharge`). The existing curve already ensures monotonicity in utilisation. Therefore the combined function is monotonic non-decreasing in utilisation.

6. **Interaction with smoothing:** The surcharge is applied in `compute_borrow_rate`, which computes the **target** borrow rate. The smoothing layer (`compute_smoothed_rate`) and hysteresis act on the target rate as usual. The surcharge does not bypass or interfere with smoothing.

7. **Validation:** `set_rate_params` rejects negative `surcharge_kink_bps` and negative `surcharge_slope`. Non-negative values, including zero, are accepted.
