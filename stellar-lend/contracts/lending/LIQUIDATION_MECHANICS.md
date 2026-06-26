# Liquidation Mechanics

## Overview
This document provides a single source of truth for the liquidation arithmetic used by the StellarLend lending contract. All formulas are expressed using the contract’s fixed‑point scaling (basis points = 1/10 000).

## Parameter Reference
| Parameter | Storage Key | Units (BPS) | Default |
|-----------|------------|------------|---------|
| `health_factor_threshold_bps` | `set_liquidation_threshold_bps` | basis points (bps) | 8000 |
| `close_factor_bps` | `set_close_factor_bps` | bps | 5000 |
| `liquidation_bonus_bps` | part of `AssetParams.liquidation_bonus` | bps | 1000 |

## Formulas
1. **Health factor** (scaled by 10 000):
   ```
   health_factor = (collateral_value * collateral_factor_bps) / (debt_value * 10_000)
   ```
   Position is liquidatable when `health_factor < 10_000` (i.e. < 1.0).
2. **Maximum repay amount (close factor)**:
   ```
   max_repay = debt_balance * close_factor_bps / 10_000
   ```
3. **Actual repay**:
   `actual_repay = min(requested_amount, max_repay)`
4. **Collateral seized (including bonus)**:
   ```
   seized_usd = actual_repay * debt_price * (10_000 + liquidation_bonus_bps) / 10_000
   collateral_to_seize = seized_usd / collateral_price
   actual_seized = min(collateral_to_seize, collateral_balance)
   ```
5. **Bad debt (shortfall)**:
   If `actual_seized < collateral_to_seize` then:
   ```
   shortfall_usd = (collateral_to_seize - actual_seized) * collateral_price
   bad_debt = shortfall_usd / debt_price
   ```

## Worked Example #1 – Standard Liquidation
- **Collateral**: 1 000 units of asset A, price = 1
- **Debt**: 900 units of asset B, price = 1
- **Params**: `collateral_factor_bps = 8000` (80 %), `liquidation_bonus_bps = 1000` (10 %)
- **Health factor** = `(1000 * 1 * 8000) / (900 * 1 * 10 000) = 0.888 < 1` → liquidatable.
- **Close factor**: `max_repay = 900 * 5000 / 10 000 = 450`
- **Liquidator requests** 500 → `actual_repay = min(500, 450) = 450`
- **Collateral seized**:
  - `seized_usd = 450 * 1 * (10 000 + 1 000) / 10 000 = 495`
  - `collateral_to_seize = 495 / 1 = 495`
  - `actual_seized = min(495, 1000) = 495`
- **Result**: `debt_repaid = 450`, `collateral_seized = 495`, `bad_debt = 0`.

## Worked Example #2 – Close‑Factor Capped
- Same collateral and debt as Example #1, but liquidator requests **200**.
- `max_repay = 450` → `actual_repay = 200` (under the cap).
- `seized_usd = 200 * (10 000 + 1 000) / 10 000 = 220`
- `collateral_to_seize = 220`
- `actual_seized = 220`
- **Result**: `debt_repaid = 200`, `collateral_seized = 220`, `bad_debt = 0`.

## Shortfall / Bad‑Debt Scenario
If the borrower’s collateral balance is insufficient to cover `collateral_to_seize`, the difference is recorded as **bad debt** (the protocol absorbs the shortfall).

## References
- Rust implementation: [`src/lib.rs`](file:///c:/Users/HomePC/stellarlend-contracts/stellar-lend/contracts/lending/src/lib.rs#L234-L348)
- Contract interface: [`README.md`](file:///c:/Users/HomePC/stellarlend-contracts/stellar-lend/contracts/lending/README.md)
