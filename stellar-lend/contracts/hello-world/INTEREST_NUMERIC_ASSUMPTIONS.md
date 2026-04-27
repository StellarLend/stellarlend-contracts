# Interest Rate Module — Numeric Assumptions

This document specifies the numeric invariants, ranges, and safety properties
of the interest rate module in `src/interest_rate.rs`.

## Unit System

All rate parameters use **basis points** (bps):
- 1 bps = 0.01%
- 10 000 bps = 100%

## Constants

| Constant             | Value         | Notes                             |
|----------------------|---------------|-----------------------------------|
| `BASIS_POINTS_SCALE` | 10 000        | 100% in bps                       |
| `SECONDS_PER_YEAR`   | 31 536 000    | 365 × 86 400 (no leap seconds)   |
| `MAX_SLOPE_BPS`      | 100 000       | 1 000% cap on slope parameters    |

## Parameter Ranges

| Parameter                | Type  | Min   | Max        | Notes                          |
|--------------------------|-------|-------|------------|--------------------------------|
| `base_rate_bps`          | i128  | 0     | 10 000     | [0%, 100%]                     |
| `kink_utilization_bps`   | i128  | 1     | 9 999      | (0%, 100%) exclusive           |
| `multiplier_bps`         | i128  | 0     | 100 000    | [0%, 1000%]                    |
| `jump_multiplier_bps`    | i128  | 0     | 100 000    | [0%, 1000%]                    |
| `rate_floor_bps`         | i128  | 0     | 10 000     | Must be ≤ `rate_ceiling_bps`   |
| `rate_ceiling_bps`       | i128  | 0     | 10 000     | Must be ≥ `rate_floor_bps`     |
| `spread_bps`             | i128  | 0     | 10 000     | [0%, 100%]                     |
| `emergency_adjustment`   | i128  | -10000| 10 000     | Applied additively to rate     |

## Rate Computation

### Utilization

```
utilization = (total_borrows × 10 000) / total_deposits
```

- If `total_deposits ≤ 0` → utilization = 0 (no division by zero)
- Result clamped to `[0, 10 000]`

### Borrow Rate

```
if utilization ≤ kink:
    rate = base_rate + (utilization × multiplier) / kink

if utilization > kink:
    rate = base_rate + multiplier + ((utilization − kink) × jump_multiplier) / (10 000 − kink)
```

Then: `rate = clamp(rate + emergency_adjustment, floor, ceiling)`

### Supply Rate

```
supply_rate = max(borrow_rate − spread, floor)
```

## Interest Accrual

### Simple (Linear)

```
interest = principal × rate_bps × elapsed_seconds / (10 000 × 31 536 000)
```

Overflow risk: `principal × rate_bps × elapsed_seconds` must fit in `i128`.
- Max safe: `principal × rate × time < 2^127 ≈ 1.7 × 10^38`
- For principal = 10^18, rate = 10 000, max safe elapsed ≈ 5.4 × 10^15 seconds (171M years)

### Compound (Yearly Discrete)

For each full year: `balance = balance + balance × rate / 10 000`
For remaining seconds: simple interest on compounded balance.

- Handles up to ~200 years at 100% APR for principals ≤ 10^30
- Deterministic (no floating-point)
- Always ≥ simple interest for multi-year horizons

## Checked Arithmetic

Every arithmetic operation uses `checked_mul`, `checked_div`, `checked_add`, `checked_sub`.
On overflow → `InterestRateError::Overflow`.
On division by zero → `InterestRateError::DivisionByZero`.

## Security Invariants

1. **Admin-only mutation**: `update_interest_rate_config` and `set_emergency_rate_adjustment` require `require_admin`.
2. **Initialization guard**: `initialize_interest_rate_config` can only be called once.
3. **Rate clamping**: Final borrow rate is always in `[floor, ceiling]` regardless of config or emergency adjustment.
4. **Supply rate floor**: Supply rate never goes below `rate_floor_bps`.
5. **No division by zero**: Zero deposits → 0 utilization; kink bounds prevent zero denominators.
6. **Determinism**: No floating-point, no randomness, no external calls.
