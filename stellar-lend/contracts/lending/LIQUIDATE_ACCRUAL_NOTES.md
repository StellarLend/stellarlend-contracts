# Liquidation Accrual and Settlement Notes

## Overview

To guarantee exact accounting, prevent disagreements between individual positions and the protocol-level books, and ensure the auditable capitalization of pending interest, the `liquidate` flow in the lending contract uses a **settle-then-liquidate** execution order.

Instead of reading a transient snapshot of the borrower's outstanding debt (via `effective_debt`), the contract invokes `settle_accrual` at the very beginning of the liquidation transaction. This call performs the following operations:
1. Computes all pending accrued interest since the borrower's `last_update` timestamp.
2. Capitalizes that interest by adding it directly to the borrower's stored `principal`.
3. Stamps the borrower's `last_update` timestamp to the current ledger timestamp.
4. Persists the capitalized state to storage using `save_debt`.

All subsequent liquidation logic—including the health factor check, close factor clamp, incentive calculation, and final debt reduction—operates on this fully-settled, up-to-date position.

---

## Worked Numeric Example

### 1. Initial State (t = 0)
- **Borrower's Deposited Collateral**: `100` units
- **Borrower's Principal Debt**: `80` units
- **Last Update Timestamp (`last_update`)**: `0`
- **Fixed APR (`DEFAULT_APR_BPS`)**: `500` basis points (5% per year)
- **Liquidation Threshold**: `80%` (expressed as `8000` BPS)
- **Close Factor**: `50%` (expressed as `5000` BPS)
- **Liquidation Incentive**: `10%` bonus (expressed as `1000` BPS, multiplier `11000/10000`)

#### Health Factor Check (t = 0)
$$\text{Health Factor} = \frac{\text{Collateral} \times \text{Threshold}}{\text{Debt}} = \frac{100 \times 8000}{80} = 10000\ (\text{exactly } 1.0)$$
Since the health factor is $\ge 10000$, the position is healthy, and any liquidation attempt at $t = 0$ is rejected with `LendingError::PositionHealthy`.

---

### 2. Time Advancement (t = 1 year / 31,536,000 seconds)
Over the course of 1 year, interest accrues on the principal debt of `80` units.

#### Interest Calculation
$$\text{Accrued Interest} = \frac{\text{Principal} \times \text{Elapsed Time} \times \text{APR BPS}}{\text{Seconds Per Year} \times 10000}$$
$$\text{Accrued Interest} = \frac{80 \times 31,536,000 \times 500}{31,536,000 \times 10000} = 4\text{ units}$$

---

### 3. Liquidation Execution (t = 1 year)
A liquidator attempts to liquidate the position. The contract executes the **settle-then-liquidate** steps:

#### Step 3.1: Settle Accrual and Persist
The contract calls `settle_accrual(&position, now, DEFAULT_APR_BPS)`:
- **New Principal**: $80 + 4 = 84$ units.
- **New `last_update`**: `31,536,000` (1 year).
- **Storage Update**: This `DebtPosition { principal: 84, last_update: now }` is saved back to storage.

#### Step 3.2: Re-Evaluate Health Factor
Using the settled debt (`84`):
$$\text{Health Factor} = \frac{100 \times 8000}{84} = 9523\ (0.9523)$$
Since $9523 < 10000$, the position is now unhealthy, and the liquidation proceeds.

#### Step 3.3: Apply Close-Factor and Request Clamps
Suppose the liquidator requests to repay `10` units.
- **Maximum Repayable (50% Close Factor)**:
  $$\text{Max Repay} = \frac{\text{Debt} \times 5000}{10000} = \frac{84 \times 5000}{10000} = 42\text{ units}$$
- **Actual Repay**: $\min(\text{Requested}, \text{Max Repay}) = \min(10, 42) = 10\text{ units}$.

#### Step 3.4: Compute Seized Collateral and Apply Clamp
- **Seized Collateral (with 10% bonus)**:
  $$\text{Seized} = \frac{\text{Actual Repay} \times 11000}{10000} = \frac{10 \times 11000}{10000} = 11\text{ units}$$
- **Final Seized**: $\min(\text{Seized}, \text{Available Collateral}) = \min(11, 100) = 11\text{ units}$.

#### Step 3.5: Final State Transition and Storage Write
- **Final Debt**: $84 - 10 = 74$ units.
- **Final Collateral**: $100 - 11 = 89$ units.
- **Last Update Timestamp**: `31,536,000` (1 year).

The contract writes `DebtPosition { principal: 74, last_update: now }` and collateral balance `89` back to storage. The liquidator pays `10` debt tokens and receives `11` collateral tokens.

---

## Summary of Invariants Preserved
- **Auditable Capitalization**: The intermediate accrual phase is fully committed, preventing any silent re-basing of unpaid interest.
- **No Free Interest**: Interest cannot be bypassed or minimized during liquidations.
- **Rounding Consistency**: All divisions use checked floor rounding, ensuring that fractional remainders always benefit the protocol solvency.
