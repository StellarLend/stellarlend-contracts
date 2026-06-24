# Interest Rate Kink Model

This document is the contributor reference for the lending contract borrow-rate
curve implemented in [`src/rate_model.rs`](src/rate_model.rs). The model prices
borrow demand from protocol utilization, expressed in basis points (bps):

```text
utilization_bps = total_debt * 10_000 / total_supply
```

When `total_supply` is zero, `current_borrow_rate` treats utilization as `0`.
If `DataKey::RateParams` is not configured, the lending contract preserves the
legacy fixed rate by returning `DEFAULT_APR_BPS = 500` from `debt.rs`.

## Parameters

All values are annualized bps unless otherwise noted.

| Field | Default | Meaning |
| --- | ---: | --- |
| `base_rate_bps` | `100` | 1.00% APR at zero utilization before floor/ceiling clamps. |
| `kink_utilization_bps` | `8_000` | 80% utilization point where the slope changes. |
| `multiplier_bps` | `2_000` | Below-kink slope. Each full 100% of utilization adds 20.00% APR. |
| `jump_multiplier_bps` | `10_000` | Above-kink slope. Each full 100% above the kink adds 100.00% APR. |
| `rate_floor_bps` | `50` | Minimum returned borrow APR, 0.50%. |
| `rate_ceiling_bps` | `10_000` | Maximum returned borrow APR, 100.00%. |

## Formula

`compute_borrow_rate(utilization_bps, params)` first calculates the below-kink
portion:

```text
pre_kink_rate =
  base_rate_bps
  + min(utilization_bps, kink_utilization_bps) * multiplier_bps / 10_000
```

If utilization is above the kink, it adds the jump-rate portion:

```text
raw_rate =
  pre_kink_rate
  + (utilization_bps - kink_utilization_bps) * jump_multiplier_bps / 10_000
```

If utilization is at or below the kink:

```text
raw_rate = pre_kink_rate
```

The final borrow APR is clamped:

```text
borrow_rate_bps = min(max(raw_rate, rate_floor_bps), rate_ceiling_bps)
```

All arithmetic is integer bps arithmetic. Intermediate multiplication, addition,
subtraction, and division use checked operations in `rate_model.rs`.

## Default Curve Examples

Using `RateParams::default()`:

| Utilization | Input bps | Calculation | Borrow APR |
| --- | ---: | --- | ---: |
| 0% | `0` | `100` | `100` bps (1.00%) |
| 50% | `5_000` | `100 + 5_000 * 2_000 / 10_000` | `1_100` bps (11.00%) |
| 80% kink | `8_000` | `100 + 8_000 * 2_000 / 10_000` | `1_700` bps (17.00%) |
| 100% | `10_000` | `1_700 + (10_000 - 8_000) * 10_000 / 10_000` | `3_700` bps (37.00%) |

```text
borrow_rate_bps
10_000 |                              ceiling
 3_700 |                         *
 1_700 |                   *
 1_100 |             *
   100 | *
       +---0%-----50%-----80%----100%---- utilization
```

## Edge Behavior

- Utilization can exceed 100% if `total_debt > total_supply`; the jump slope
  continues to apply until `rate_ceiling_bps` clamps the result.
- Low custom configurations can never return below `rate_floor_bps`.
- High custom configurations can never return above `rate_ceiling_bps`.
- The configured curve is monotonic non-decreasing for the default parameters.
- `current_borrow_rate` reads `TotalDebt`, `TotalDeposits`, and `RateParams`
  once through `load_rate_snapshot`, then computes the rate from that snapshot.

## Tests

The source-level tests in `src/rate_model.rs` cover the main curve checkpoints:

- zero utilization returns the default base rate;
- below-kink utilization is linear;
- the 80% kink returns `1_700` bps;
- 100% utilization returns `3_700` bps;
- floor and ceiling clamps apply;
- property tests assert monotonicity, deterministic output, non-negative rates,
  and floor/ceiling bounds over broad utilization ranges.

The integration-style tests near `current_borrow_rate` in `src/lib.rs` cover
the storage snapshot behavior and the legacy `DEFAULT_APR_BPS` fallback when
`RateParams` is absent.
