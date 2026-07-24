# Interest-rate kink model

This document describes the target borrow-rate curve implemented by `compute_borrow_rate` in `src/rate_model.rs`. The function uses a single kink at utilization `kink_utilization_bps` and applies a different slope below and above that point before clamping the result to the configured floor and ceiling.

## Borrow-rate formula

Let `u` be utilization in basis points, where `0 <= u <= 10_000`.

If `u <= kink_utilization_bps`:

$$
R(u) = \operatorname{clamp}\left(\text{base\_rate\_bps} + \left\lfloor\frac{u \cdot \text{multiplier\_bps}}{10{,}000}\right\rfloor,\ \text{rate\_floor\_bps},\ \text{rate\_ceiling\_bps}\right)
$$

If `u > kink_utilization_bps`:

$$
R(u) = \operatorname{clamp}\left(\text{base\_rate\_bps} + \left\lfloor\frac{\text{kink\_utilization\_bps} \cdot \text{multiplier\_bps}}{10{,}000}\right\rfloor + \left\lfloor\frac{(u - \text{kink\_utilization\_bps}) \cdot \text{jump\_multiplier\_bps}}{10{,}000}\right\rfloor,\ \text{rate\_floor\_bps},\ \text{rate\_ceiling\_bps}\right)
$$

The implementation uses `checked_add`, `checked_mul`, and `checked_div` and then applies:

```text
raw_rate.max(rate_floor_bps).min(rate_ceiling_bps)
```

All rates in this document are expressed in basis points (bps), where `100 bps = 1%`.

## Default parameters

The defaults are taken directly from `RateParams::default()`:

| Field | Default value | Meaning |
|---|---:|---|
| `base_rate_bps` | 100 | Starting rate at zero utilization |
| `kink_utilization_bps` | 8,000 | Utilization where the slope changes |
| `multiplier_bps` | 2,000 | Slope below the kink |
| `jump_multiplier_bps` | 10,000 | Slope above the kink |
| `rate_floor_bps` | 50 | Minimum effective borrow rate |
| `rate_ceiling_bps` | 10,000 | Maximum effective borrow rate |
| `max_rate_change_per_ledger_bps` | `i128::MAX` | Smoothing cap; not used by the raw target-rate function |
| `hysteresis_bps` | 0 | Hysteresis band; not used by the raw target-rate function |

The curve therefore has:

- a pre-kink slope of `2,000 / 10,000 = 20%` of the utilization scale
- a post-kink slope of `10,000 / 10,000 = 100%` of the utilization scale
- a floor at `50 bps` and a ceiling at `10,000 bps`

## ASCII sketch

```text
rate (bps)
  ^
  |                .
  |               / \
  |              /   \
  |             /     \
  |            /       \
  |           /         \
  |          /           \
  |         /             \
  |        /               \
  +-------------------------> utilization (bps)
    0      8000            10000
      \__ pre-kink slope __/  \__ post-kink slope __/
```

## Worked examples

Using `RateParams::default()`:

- At `0%` utilization (`u = 0`):
  - `base_rate_bps + 0 = 100`
  - effective rate: `100 bps`

- At the kink (`u = 8,000`):
  - `100 + (8,000 * 2,000 / 10,000) = 100 + 1,600 = 1,700`
  - effective rate: `1,700 bps`

- At `100%` utilization (`u = 10,000`):
  - pre-kink part: `1,700`
  - excess above kink: `(10,000 - 8,000) * 10,000 / 10,000 = 2,000`
  - raw rate: `1,700 + 2,000 = 3,700`
  - effective rate: `3,700 bps`

These examples are also asserted by the unit tests in `src/rate_model.rs`.
