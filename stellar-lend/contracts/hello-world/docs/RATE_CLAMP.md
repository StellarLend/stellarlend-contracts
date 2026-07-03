# Borrow Rate Clamp

## Overview

`HelloContract::get_borrow_rate` now applies an admin-configurable hard band to the final effective borrow rate.  The band is stored on `InterestRateConfig` as:

- `min_rate_bps` — inclusive lower bound for the effective borrow rate.
- `max_rate_bps` — inclusive upper bound for the effective borrow rate.

All values are basis points (`bps`): `1 bps = 0.01%`, and `10_000 bps = 100%`.

## Default Semantics

Defaults preserve the previous effective behavior until governance/admin chooses a narrower band:

```text
min_rate_bps = 0
max_rate_bps = i128::MAX
```

That means a default deployment has no new practical ceiling and no non-zero floor.

## Formula

Utilization is calculated from protocol totals:

```text
utilization_bps = total_borrows * 10_000 / total_deposits
```

If `total_deposits <= 0`, utilization is `0`.

The raw jump-rate curve is:

```text
if utilization_bps <= kink_utilization_bps:
    raw_rate = base_rate_bps
             + utilization_bps * multiplier_bps / kink_utilization_bps
else:
    raw_rate = base_rate_bps
             + multiplier_bps
             + (utilization_bps - kink_utilization_bps)
               * jump_multiplier_bps
               / (10_000 - kink_utilization_bps)
```

Emergency adjustment is added, then the clamp is applied as the **last** borrow-rate step:

```text
effective_borrow_rate_bps = clamp(
    raw_rate + emergency_adjustment_bps,
    min_rate_bps,
    max_rate_bps
)
```

where:

```text
clamp(x, min, max) = max(min, min(x, max))
```

## Supply Rate Consistency

`get_supply_rate` derives from the clamped borrow rate, not the raw curve output:

```text
supply_rate_bps = max(effective_borrow_rate_bps - spread_bps, min_rate_bps)
```

This keeps supplier-facing rates consistent with the rate borrowers actually pay.

## Worked Example

Configuration:

```text
base_rate_bps = 100              # 1.00%
kink_utilization_bps = 8_000     # 80%
multiplier_bps = 2_000           # +20% to kink
jump_multiplier_bps = 10_000     # +100% from kink to full utilization
min_rate_bps = 250               # 2.50% floor
max_rate_bps = 3_000             # 30.00% ceiling
spread_bps = 500                 # 5.00%
```

At 100% utilization:

```text
raw_rate = 100 + 2_000 + ((10_000 - 8_000) * 10_000 / 2_000)
         = 100 + 2_000 + 10_000
         = 12_100 bps

effective_borrow_rate = clamp(12_100, 250, 3_000)
                      = 3_000 bps

supply_rate = max(3_000 - 500, 250)
            = 2_500 bps
```

At 0% utilization with a raw/base rate below the floor:

```text
raw_rate = 0 bps
effective_borrow_rate = clamp(0, 250, 3_000) = 250 bps
```

## Validation

The implementation rejects invalid clamp configuration where:

- `min_rate_bps < 0`
- `max_rate_bps < min_rate_bps`

The rate computation uses checked arithmetic and returns an error on overflow or invalid denominators.
