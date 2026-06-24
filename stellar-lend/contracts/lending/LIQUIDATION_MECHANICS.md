# Liquidation Mechanics

This document describes the liquidation rules implemented by the lending
contract. The formulas below mirror the current `liquidate`,
`get_position`, and `get_health_factor` logic in `src/lib.rs`.

## Parameters

| Parameter | Value | Meaning |
|---|---:|---|
| Basis point denominator | `10_000` | All BPS values use `10_000 = 100%`. |
| Health factor scale | `10_000` | `10_000` represents a health factor of `1.0`. |
| No-debt health factor | `100_000_000` | Sentinel returned by views when a borrower has no debt. |
| Liquidation threshold | `8_000` BPS | Collateral is weighted at 80% for health-factor checks. |
| Healthy boundary | `10_000` | Positions with `health_factor >= 10_000` are not liquidatable. |
| Close factor | `5_000` BPS | A liquidator can repay at most 50% of settled debt per call. |
| Liquidation incentive | `1_000` BPS | Seized collateral is `110%` of the repaid amount before capping. |
| Default accrual APR | `500` BPS | `liquidate` settles borrower debt once at the start of the call. |

## Health Factor

For a borrower with positive settled debt:

```text
health_factor = collateral * 8_000 / settled_debt
```

Integer division truncates toward zero. A position is liquidatable only when:

```text
settled_debt > 0
health_factor < 10_000
```

If settled debt is zero, `liquidate` rejects the call with
`LendingError::PositionHealthy`. The view helpers return the no-debt sentinel
instead of dividing by zero.

The exact healthy boundary is inclusive:

```text
health_factor >= 10_000  => not liquidatable
health_factor <  10_000  => liquidatable
```

For example, with `debt = 1_000`:

```text
collateral = 1_250  => health_factor = 1_250 * 8_000 / 1_000 = 10_000
collateral = 1_249  => health_factor = 1_249 * 8_000 / 1_000 = 9_992
```

The first position is healthy. The second position is liquidatable.

## Repay Cap And Seizure

`liquidate` settles the borrower's debt once, then reuses that same settled
principal for the health-factor check, close-factor cap, and final debt write.

```text
max_repay = settled_debt * 5_000 / 10_000
actual_repay = min(requested_repay, max_repay)

seized_collateral = actual_repay * (10_000 + 1_000) / 10_000
final_seized = min(seized_collateral, collateral)

new_debt = settled_debt - actual_repay
new_collateral = collateral - final_seized
```

The current implementation uses saturating subtraction for the final storage
updates after the caps above are applied.

## Shortfall And Bad Debt

When the incentive-adjusted seizure is larger than available collateral,
`final_seized` is capped at the borrower's collateral balance. The liquidation
still repays only `actual_repay`, so any remaining debt stays in the borrower
position.

The current `liquidate` path does not write a separate bad-debt ledger entry.
Collateral shortfall is represented by exhausted collateral and residual debt
remaining in the borrower position after the call.

## Worked Examples

### Example 1: Healthy Position

```text
collateral = 2_000
settled_debt = 1_000

health_factor = 2_000 * 8_000 / 1_000
              = 16_000
```

Because `16_000 >= 10_000`, the position is healthy and `liquidate` returns
`LendingError::PositionHealthy`.

### Example 2: Close-Factor-Capped Liquidation

```text
collateral = 1_000
settled_debt = 1_000
requested_repay = 800

health_factor = 1_000 * 8_000 / 1_000
              = 8_000
```

The position is liquidatable because `8_000 < 10_000`.

```text
max_repay = 1_000 * 5_000 / 10_000
          = 500

actual_repay = min(800, 500)
              = 500

seized_collateral = 500 * 11_000 / 10_000
                  = 550

final_seized = min(550, 1_000)
             = 550

new_debt = 1_000 - 500
         = 500

new_collateral = 1_000 - 550
               = 450
```

The call returns `500`, the actual debt repaid.

### Example 3: Collateral Shortfall

```text
collateral = 400
settled_debt = 1_000
requested_repay = 500

health_factor = 400 * 8_000 / 1_000
              = 3_200
```

The position is liquidatable because `3_200 < 10_000`.

```text
max_repay = 1_000 * 5_000 / 10_000
          = 500

actual_repay = min(500, 500)
              = 500

seized_collateral = 500 * 11_000 / 10_000
                  = 550

final_seized = min(550, 400)
             = 400

new_debt = 1_000 - 500
         = 500

new_collateral = 400 - 400
               = 0
```

The borrower has no collateral left, but `500` debt remains in the position.
That residual is the protocol shortfall exposed by the current storage state.

