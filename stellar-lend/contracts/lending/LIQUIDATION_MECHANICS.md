# Liquidation Mechanics

## Overview
This document details the liquidation process implemented in the StellarLend Lending contract (`stellar-lend/contracts/lending/src/lib.rs`). It aligns with the protocol constants and explains each step of the calculation.

## Key Constants
| Constant | Value | Description |
|---|---|---|
| `HEALTH_FACTOR_SCALE` | `10_000` | Scaling factor for health factor.
| `LIQUIDATION_THRESHOLD_BPS` | `8000` | Threshold (80%) at which a position becomes liquidatable.
| `CLOSE_FACTOR` | `5000` (50%) | Maximum proportion of a borrower’s debt that can be repaid in a single liquidation.
| `INCENTIVE_BPS` | `1000` (10%) | Bonus applied to the seized collateral.
| `BPS_DENOM` | `10_000` | Basis points denominator.

## Formulae
1. **Health Factor (HF)**
   ```
   HF = floor(collateral * LIQUIDATION_THRESHOLD_BPS / debt)
   ```
   - If `HF >= 10000` the position is healthy; otherwise it is eligible for liquidation.
2. **Maximum Repayable Debt (Close‑Factor Cap)**
   ```
   max_repay = floor(debt * CLOSE_FACTOR / BPS_DENOM)
   actual_repay = min(requested_amount, max_repay)
   ```
3. **Seized Collateral (Incentive)**
   ```
   seized = floor(actual_repay * (BPS_DENOM + INCENTIVE_BPS) / BPS_DENOM)
   seized = min(seized, collateral)
   ```
   The extra `INCENTIVE_BPS` gives the liquidator a 10 % bonus on the amount repaid.
4. **Shortfall / Bad Debt**
   If `actual_repay` does not fully cover the debt, the remaining debt becomes **bad debt** and is handled by the protocol’s bad‑debt accounting flow.

## Worked Numeric Examples
### Example 1 – Simple Liquidation
- **Collateral**: `8_000`
- **Debt**: `10_000`
- **Requested Repay Amount**: `6_000`

1. HF = floor(8_000 × 8_000 / 10_000) = **6_400** → liquidatable.
2. `max_repay` = floor(10_000 × 5_000 / 10_000) = **5_000**.
3. `actual_repay` = min(6_000, 5_000) = **5_000**.
4. `seized` = floor(5_000 × (10_000 + 1_000) / 10_000) = floor(5_000 × 11_000 / 10_000) = **550**.
5. New collateral balance = 8_000 − 550 = **7_450**.
6. Remaining debt = 10_000 − 5_000 = **5_000**.

### Example 2 – Close‑Factor Capped & Shortfall
- **Collateral**: `2_000`
- **Debt**: `12_000`
- **Requested Repay Amount**: `8_000`

1. HF = floor(2_000 × 8_000 / 12_000) = **1_333** → liquidatable.
2. `max_repay` = floor(12_000 × 5_000 / 10_000) = **6_000**.
3. `actual_repay` = min(8_000, 6_000) = **6_000** (close‑factor caps repayment).
4. `seized` = floor(6_000 × 11_000 / 10_000) = **6_600** → capped by collateral, so `seized` = **2_000**.
5. Collateral is fully seized; borrower still owes **6_000** debt, which becomes **bad debt**.

## Parameter Table
| Parameter | Symbol | Scale | Source |
|---|---|---|---|
| Liquidation Threshold | `LIQUIDATION_THRESHOLD_BPS` | BPS (8000) | `src/lib.rs` line 93 |
| Close Factor | `CLOSE_FACTOR` | BPS (5000) | `src/lib.rs` line 1088 |
| Incentive Bonus | `INCENTIVE_BPS` | BPS (1000) | `src/lib.rs` line 1107 |
| BPS Denominator | `BPS_DENOM` | 10_000 | `src/lib.rs` line 96 |

## References
- Contract source: `stellar-lend/contracts/lending/src/lib.rs`
- Liquidation tests: `tests/liquidate_event_test.rs`, `tests/liquidate_close_factor_test.rs`
- Protocol design notes: `CROSS_ASSET_LIQUIDATION_NOTES.md`

---
*This document is version‑controlled and will be updated alongside any changes to the liquidation implementation.*
