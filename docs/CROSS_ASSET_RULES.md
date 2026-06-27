# Cross-Asset Borrow and Repay Rules and Invariants

## Overview

The StellarLend protocol supports cross-asset borrowing and repaying, allowing users to deposit multiple types of collateral and borrow different assets. This document outlines the rules, invariants, and edge cases for these operations.

## Core Concepts

### Asset Configuration

Each asset in the protocol has the following parameters:

- **Collateral Factor**: Percentage of asset value that counts toward borrowing capacity (e.g., 75% = 7500 basis points)
- **Borrow Factor**: Multiplier applied to borrowed asset value for risk calculation (e.g., 80% = 8000 basis points)
- **Reserve Factor**: Percentage of interest allocated to protocol reserves (e.g., 10% = 1000 basis points)
- **Max Supply**: Maximum total supply cap for the asset (0 = unlimited)
- **Max Borrow**: Maximum total borrow cap for the asset (0 = unlimited)
- **Can Collateralize**: Whether the asset can be used as collateral
- **Can Borrow**: Whether the asset can be borrowed
- **Price**: Current price in base units (7 decimals precision)

### Position Tracking

Each user has separate positions for each asset, tracking:

- **Collateral**: Amount deposited as collateral
- **Debt Principal**: Original borrowed amount
- **Accrued Interest**: Interest accumulated over time
- **Last Updated**: Timestamp of last position update

### Health Factor

The health factor determines whether a position can be liquidated:

```
Health Factor = (Weighted Collateral Value / Weighted Debt Value) * 10000
```

- Health Factor >= 10000 (1.0): Position is healthy
- Health Factor < 10000 (1.0): Position is liquidatable

**Weighted Collateral Value** = Sum of (Collateral Amount × Price × Collateral Factor) for all assets

**Weighted Debt Value** = Sum of (Debt Amount × Price × Borrow Factor) for all assets

## Borrowing Rules

### Single Asset Borrowing

1. User must have sufficient collateral deposited
2. Borrowed amount must not exceed borrow capacity
3. Health factor must remain >= 1.0 after borrow
4. Asset must have `can_borrow = true`
5. Total borrows must not exceed `max_borrow` cap

### Multi-Asset Borrowing

1. Collateral from all assets is aggregated into total weighted collateral value
2. User can borrow any enabled asset up to their total borrow capacity
3. Each borrow operation checks the unified health factor
4. Borrows are tracked separately per asset

### Borrow Capacity Calculation

```
Borrow Capacity = Weighted Collateral Value - Weighted Debt Value
```

This represents the maximum additional value (in USD) that can be borrowed.

### Edge Cases

#### Borrowing Against Multiple Collaterals

- **Scenario**: User deposits USDC ($10k) and ETH ($10k), borrows BTC
- **Calculation**: Total collateral = $20k, Weighted = $15k (75%), can borrow up to $15k worth of BTC
- **Invariant**: Health factor considers all collateral and all debt

#### Sequential Borrows

- **Scenario**: User borrows USDC, then borrows ETH
- **Rule**: Each borrow reduces available borrow capacity
- **Invariant**: Sum of all debt values must not exceed weighted collateral value

#### Borrow at Maximum Capacity

- **Scenario**: User borrows exactly at 75% collateral ratio
- **Result**: Health factor = 1.0, no additional borrowing possible
- **Risk**: Any price movement can trigger liquidation

## Repayment Rules

### Partial Repayment

1. Repayment amount can be less than total debt
2. Interest is paid first, then principal
3. Health factor improves proportionally
4. Borrow capacity increases

### Full Repayment

1. Repaying more than debt amount caps at total debt
2. Debt principal and accrued interest both become zero
3. User can withdraw all collateral after full repayment

### Multi-Asset Repayment

1. Each asset's debt is repaid independently
2. Repaying one asset's debt improves overall health factor
3. User can choose which asset to repay first

### Repayment Ordering

When repaying debt with accrued interest:

```
1. Pay accrued interest first
2. Pay remaining principal
```

Example:
- Debt Principal: 1000
- Accrued Interest: 50
- Repay 75: Interest becomes 0, Principal becomes 975

### Edge Cases

#### Repay More Than Debt

- **Scenario**: User tries to repay 1000 but only owes 500
- **Result**: Only 500 is repaid, debt becomes 0
- **Invariant**: Debt cannot be negative

#### Partial Repay Across Multiple Assets

- **Scenario**: User has USDC debt (10k) and ETH debt (5 ETH), repays 5k USDC
- **Result**: USDC debt becomes 5k, ETH debt unchanged
- **Effect**: Health factor improves, borrow capacity increases

#### Repay One Asset Fully, Keep Others

- **Scenario**: User repays all USDC debt but keeps ETH debt
- **Result**: USDC debt = 0, ETH debt unchanged
- **Invariant**: Position summary reflects only remaining debt

## Collateral Management Rules

### Withdrawal Rules

1. User can only withdraw up to their deposited collateral
2. Withdrawal must not cause health factor to drop below 1.0
3. If user has no debt, can withdraw all collateral
4. Withdrawal from one asset considers all collateral and debt

### Collateral Devaluation

#### Single Collateral Devaluation

- **Scenario**: User has USDC and ETH collateral, ETH price drops 50%
- **Effect**: Total collateral value decreases, health factor decreases
- **Risk**: May become liquidatable if health factor < 1.0

#### All Collateral Devaluation

- **Scenario**: All collateral assets lose value simultaneously
- **Effect**: Weighted collateral value drops significantly
- **Risk**: High likelihood of liquidation

#### Borrowed Asset Appreciation

- **Scenario**: User borrows ETH, ETH price doubles
- **Effect**: Debt value doubles, health factor decreases
- **Risk**: May trigger liquidation

### Collateral Removal Edge Cases

#### Withdraw One Collateral, Maintain Health

- **Scenario**: User has USDC ($20k) and ETH ($10k) collateral, debt $15k
- **Action**: Withdraw $10k USDC
- **Result**: Remaining collateral ($20k) still supports debt
- **Invariant**: Health factor remains >= 1.0

#### Withdraw Breaks Health Factor

- **Scenario**: User has USDC ($10k) and ETH ($10k) collateral, debt $14k
- **Action**: Try to withdraw $5k USDC
- **Result**: Transaction fails
- **Reason**: Remaining collateral ($15k) × 0.75 = $11.25k < $14k debt

### Withdrawal Boundary Constraints

The protocol enforces deterministic withdrawal limits precisely at the collateral ratio boundaries. These boundaries ensure that no withdrawal can leave a position undercollateralized or liquidatable.

#### Deterministic Edge Cases

1. **Exact Capacity Withdrawal**
   - **Position**: $100 Collateral (80% LTV), $80 Debt.
   - **Boundary**: Weighted Collateral ($80) == Debt ($80).
   - **Constraint**: Any withdrawal of > 0 units will fail.
   - **Rounding**: Even a withdrawal of 1 atomic unit (10^-7) is rejected if it drops the weighted value below the debt.

2. **Multi-Asset Weighted Boundary**
   - **Position**: $50 Asset A (80% LTV) + $50 Asset B (60% LTV). Total Weighted = $40 + $30 = $70.
   - **Debt**: $70.
   - **Constraint**: Withdrawal from Asset A is blocked. Withdrawal from Asset B is blocked.
   - **Buffer**: Repaying $1 of debt allows withdrawing up to $1 / 0.8 = $1.25 of Asset A.

3. **Price Move Boundary**
   - **Position**: 1 ETH @ $2000 (80% LTV). Weighted = $1600. Debt = $1500.
   - **Event**: ETH price drops to $1875.
   - **New Weighted**: $1875 * 0.8 = $1500.
   - **Result**: Position hits the withdrawal boundary. All further withdrawals are blocked until debt is repaid or price recovers.

### Security Notes

1. **Prevention of Undercollateralized Withdrawals**
   - Every withdrawal operation triggers a full `health_factor` re-calculation using real-time oracle prices.
   - The operation is atomic: if the post-withdrawal `health_factor` is < 1.0 (10000), the entire transaction reverts.
   - This prevents users from "gaming" rounding errors or price lags to extract more value than their collateral supports.

2. **Oracle Reliability**
   - Withdrawal constraints are only as strong as the price feed.
   - If an oracle update is stale or missing, the protocol enters a fail-safe mode where withdrawals (and borrows) are blocked to prevent state corruption.

3. **Rounding Direction**
   - To maintain protocol safety, the system always rounds **down** for collateral value and **up** for debt value.
   - This conservative rounding ensures that at the boundary, the protocol always errs on the side of over-collateralization.

## Invariants

### System-Wide Invariants

1. **Health Factor Consistency**: Health factor calculation must be consistent across all operations
2. **No Negative Debt**: Debt principal and accrued interest cannot be negative
3. **No Negative Collateral**: Collateral balance cannot be negative
4. **Borrow Capacity Accuracy**: Borrow capacity = Weighted Collateral - Weighted Debt
5. **Price Staleness**: Prices older than 1 hour trigger error

### Per-Asset Invariants

1. **Total Supply Tracking**: Sum of all user collateral = total supply for asset
2. **Total Borrow Tracking**: Sum of all user debt = total borrows for asset
3. **Cap Enforcement**: Total supply <= max_supply, total borrows <= max_borrow
4. **Configuration Validity**: Collateral factor, borrow factor, reserve factor in [0, 10000]

### Per-User Invariants

1. **Position Isolation**: Each user's position is independent
2. **Asset Independence**: Collateral and debt tracked separately per asset
3. **Health Factor Enforcement**: Cannot borrow if health factor would drop below 1.0
4. **Withdrawal Restriction**: Cannot withdraw if health factor would drop below 1.0

## Security Considerations

### Price Oracle Dependency

- All calculations depend on accurate, up-to-date prices
- Stale prices (>1 hour) cause operations to fail
- Price manipulation can affect health factors and liquidations

### Collateral Factor Changes

- Admin can change collateral factors
- Existing positions are affected immediately
- Users may become liquidatable after factor reduction

### Asset Disabling

- Admin can disable borrowing or collateralization
- Existing positions remain valid
- New operations on disabled assets fail

### Flash Loan Attacks

- Cross-asset operations are atomic
- Price manipulation within a transaction is limited
- Health factor checks prevent undercollateralized positions

## Testing Coverage

The test suite covers:

1. **Multi-Collateral Borrowing**: Borrowing against 2-3 different collateral types
2. **Multi-Asset Borrowing**: Borrowing multiple different assets
3. **Partial Repayment**: Repaying portions of debt across multiple assets
4. **Collateral Devaluation**: Price drops affecting health factor
5. **Collateral Removal**: Withdrawing collateral with and without debt
6. **Sequential Operations**: Multiple borrow/repay cycles
7. **Boundary Conditions**: Very small and very large amounts
8. **Configuration Changes**: Collateral factor and asset disabling
9. **Multiple Users**: Independent positions and shared price updates
10. **Precision**: Health factor calculations at exact thresholds

## Example Scenarios

### Scenario 1: Multi-Collateral Borrow

```
Initial State:
- Deposit: $10,000 USDC (CF: 75%)
- Deposit: 5 ETH @ $2,000 = $10,000 (CF: 75%)
- Total Collateral: $20,000
- Weighted Collateral: $15,000

Action: Borrow $12,000 USDC

Result:
- Debt: $12,000
- Weighted Debt: $9,600 (BF: 80%)
- Health Factor: ($15,000 / $9,600) * 10000 = 15625
- Status: Healthy ✓
```

### Scenario 2: Partial Repayment

```
Initial State:
- Collateral: $50,000 USDC
- Debt: $30,000 USDC, 10 ETH @ $2,000 = $20,000
- Total Debt: $50,000
- Health Factor: 1.0

Action: Repay $15,000 USDC

Result:
- USDC Debt: $15,000
- ETH Debt: $20,000
- Total Debt: $35,000
- Health Factor: Improved to 1.43
- Borrow Capacity: Increased by $12,000
```

### Scenario 3: Collateral Devaluation

```
Initial State:
- Collateral: 10 ETH @ $2,000 = $20,000
- Debt: $10,000 USDC
- Health Factor: 1.5

Event: ETH price drops to $1,000

Result:
- Collateral Value: $10,000
- Weighted Collateral: $7,500
- Health Factor: 0.75
- Status: Liquidatable ✗
```

## Best Practices

1. **Maintain Buffer**: Keep health factor well above 1.0 (recommended: > 1.5)
2. **Diversify Collateral**: Use multiple asset types to reduce single-asset risk
3. **Monitor Prices**: Watch for price volatility in collateral and borrowed assets
4. **Partial Repayments**: Regularly repay debt to maintain healthy position
5. **Avoid Maximum Borrowing**: Don't borrow at full capacity to prevent liquidation

## View Guarantees

The following guarantees hold for `get_cross_position_summary` and all other read-only view functions. These are verified by the invariant test suite in `cross_asset_view_invariants_test.rs`.

### G-1 — Read-only (no state mutation)

`get_cross_position_summary` reads from persistent storage only; it never writes. Calling it any number of times in any order does not change collateral balances, debt balances, or any other contract state.

### G-2 — Determinism

For a fixed contract state, every call to `get_cross_position_summary` for the same user returns the same `PositionSummary`. The function is purely deterministic.

### G-3 — Total collateral consistency

`total_collateral_usd` always equals the arithmetic sum of `amount_i × price_i ÷ PRICE_DIVISOR` for every asset `i` in the user's `collateral_balances` map. No asset is double-counted or omitted.

```
total_collateral_usd = Σ_i  (collateral_balances[i] × price_i) / 10_000_000
```

### G-4 — Total debt consistency

`total_debt_usd` always equals the arithmetic sum of `amount_j × price_j ÷ PRICE_DIVISOR` for every asset `j` in the user's `debt_balances` map.

```
total_debt_usd = Σ_j  (debt_balances[j] × price_j) / 10_000_000
```

### G-5 — Health factor formula

`health_factor` is computed from the above totals using:

```
if total_debt_usd == 0:
    health_factor = 1_000_000   # HF_NO_DEBT sentinel
else:
    weighted_collateral = Σ_i (collateral_value_i × ltv_i) / BPS_SCALE
    health_factor       = weighted_collateral × BPS_SCALE / total_debt_usd
```

All divisions use integer floor semantics (truncation toward zero). `BPS_SCALE = 10_000`.

### G-6 — Monotonicity in collateral and debt

- Adding collateral (while debt is constant) never decreases `health_factor`.
- Adding debt (while collateral is constant) never increases `health_factor`.

These properties hold as long as prices are positive (guaranteed by asset param validation).

### G-7 — User isolation

`get_cross_position_summary(user_A)` depends only on `user_A`'s position storage. Operations by `user_B` (deposits, borrows, repayments) have no effect on `user_A`'s summary.

### G-8 — Ordering invariance

Depositing assets in any order produces the same `total_collateral_usd` and `health_factor` because position maps accumulate balances additively regardless of insertion sequence.

### G-9 — Rounding is conservative (floor division)

All LTV weighting and USD-value conversions use integer floor division. This means:
- `weighted_collateral` can only be less than or equal to the real-valued result.
- A borrow is only permitted when the floor-divided health factor is **strictly above 1.0** (> `BPS_SCALE`).
- Borrowers cannot extract more value than the floor-rounded weighted collateral.

### G-10 — No view-based exploitation

Because view functions are read-only and deterministic, there is no mechanism through which a caller can:
- Manipulate another user's health factor by calling the view.
- Gain assets or reduce debt through repeated view calls.
- Trigger liquidation thresholds without an actual price or balance change.

### Boundary conditions

| Condition | Guaranteed behaviour |
|-----------|----------------------|
| No collateral, no debt | `total_collateral_usd = 0`, `total_debt_usd = 0`, `health_factor = HF_NO_DEBT` |
| Collateral but no debt | `total_collateral_usd ≥ 0`, `total_debt_usd = 0`, `health_factor = HF_NO_DEBT` |
| LTV = 0 | `weighted_collateral = 0`; borrow rejected by health check |
| Overpayment of debt | Capped at outstanding balance; `total_debt_usd` goes to 0 |
| Same asset in collateral and debt | Counted independently in both totals (no netting) |

## Isolation Mode Rules

### Definition

An asset is in **isolation mode** when its `IsolationConfig.isolated` flag is `true`. Isolated assets are permitted as collateral but subject to two hard constraints:

1. **Debt ceiling** — The aggregate outstanding debt backed by the isolated asset across all users must not exceed `isolation_debt_ceiling`. New borrows that would push the running `IsolationDebt` past this ceiling are rejected with `IsolationCeilingExceeded` (error code 2008).
2. **Non-amplifying** — An isolated asset's collateral contribution does not combine with other collateral to produce additional borrowing capacity beyond what the ceiling allows. Users must call `borrow_against_collateral` (not the generic `borrow`) to have isolation mode enforced and tracked.

### Isolation-Mode Invariants

1. **Ceiling never exceeded** — After every successful `borrow_against_collateral`, `IsolationDebt(asset) ≤ isolation_debt_ceiling`.
2. **IsolationDebt non-negative** — The running counter is always ≥ 0. Over-repayment is absorbed by saturating subtraction.
3. **Non-isolated assets unaffected** — A call to `borrow_against_collateral` with a non-isolated `collateral_asset` does not touch `IsolationDebt` and passes through with no overhead.
4. **Counter consistency** — `IsolationDebt(asset)` equals the sum of all net principal additions from `borrow_against_collateral` minus all net principal reductions from `repay_against_collateral` for that asset, over all users.
5. **Ceiling change is immediate** — Updating `isolation_debt_ceiling` takes effect on the very next borrow check. Existing debt is not retroactively invalidated; only new borrows are affected.
6. **Disabling is immediate** — Setting `isolated = false` removes all ceiling enforcement; the `IsolationDebt` counter is left in storage but never consulted until isolation is re-enabled.

### Worked Example

```
Config:  EXOTIC  isolated=true  isolation_debt_ceiling=500_000

Step 1 — User A borrows 300_000 against EXOTIC:
  check: 0 + 300_000 = 300_000 ≤ 500_000  ✓
  IsolationDebt(EXOTIC) = 300_000

Step 2 — User B borrows 200_000 against EXOTIC:
  check: 300_000 + 200_000 = 500_000 ≤ 500_000  ✓  (exactly at ceiling)
  IsolationDebt(EXOTIC) = 500_000

Step 3 — User C tries to borrow 1 against EXOTIC:
  check: 500_000 + 1 = 500_001 > 500_000  ✗  → IsolationCeilingExceeded

Step 4 — Admin lowers ceiling to 400_000:
  User C cannot borrow any amount now (500_000 > 400_000 already).

Step 5 — User A repays 150_000:
  IsolationDebt(EXOTIC) = 500_000 − 150_000 = 350_000

Step 6 — User C borrows 50_000:
  check: 350_000 + 50_000 = 400_000 ≤ 400_000  ✓
  IsolationDebt(EXOTIC) = 400_000
```

## Conclusion

The cross-asset system provides flexibility for users to manage positions across multiple assets while maintaining protocol solvency through health factor enforcement. Isolation mode adds a risk-containment layer for volatile or thinly-traded assets, capping their systemic exposure without removing them from the protocol entirely. Understanding these rules and invariants is crucial for safe protocol usage and integration.

## E2E Lifecycle: Worked Scenario (Issue #1143)

This section walks through the complete cross-asset lifecycle tested by
`cross_asset_e2e_test.rs`: deposit collateral in Asset A, borrow Asset B,
trigger a price shock, and verify the position is liquidatable with correct
post-liquidation accounting.

### Setup

| Parameter | Asset A (collateral) | Asset B (debt) |
|-----------|---------------------|----------------|
| Initial price | $1.00 (10\_000\_000) | $1.00 (10\_000\_000) |
| LTV (bps) | 7 500 | 6 000 |
| Liquidation threshold (bps) | 8 000 | 7 000 |
| Debt ceiling | 1 000 000 000 000 | 1 000 000 000 000 |

```
PRICE_DIVISOR      = 10_000_000   ($1.00 = 10_000_000 raw)
HEALTH_FACTOR_SCALE = 10_000       (HF = 1.0 → 10_000)
HEALTH_FACTOR_NO_DEBT = 100_000_000 (sentinel, no debt)
```

### Step 1 — Deposit Collateral

```
cross_asset_deposit(user, asset_A, 10_000)
→ collateral_balance[A] = 10_000
→ HF = HEALTH_FACTOR_NO_DEBT  (no debt yet)
```

### Step 2 — Borrow Asset B

```
cross_asset_borrow(user, asset_B, 7_000)

weighted_collateral = 10_000 × 10_000_000 × 8_000 / 10_000
                    = 10_000 × 8_000  (price terms cancel at 1:1)
                    = 80_000_000

total_debt_value    = 7_000 × 10_000_000
                    = 70_000_000

HF = weighted_collateral / total_debt_value
   = 80_000_000 / 70_000_000
   = 11_428  (> 10_000 → healthy ✓)
```

### Step 3 — Price Shock

Collateral asset price drops 40 %: $1.00 → $0.60 (6\_000\_000 raw).

```
weighted_collateral = 10_000 × 6_000_000 × 8_000 / 10_000
                    = 48_000_000

total_debt_value    = 7_000 × 10_000_000
                    = 70_000_000

HF = 48_000_000 / 70_000_000 = 6_857  (< 10_000 → liquidatable ✗)
```

### Step 4 — Liquidation (close-factor 50 %, incentive 10 %)

```
repaid_amount  = 7_000 × 5_000 / 10_000 = 3_500   (50 % close factor)
seized_amount  = 3_500 × 11_000 / 10_000 = 3_850   (10 % bonus)
                 min(3_850, 10_000) = 3_850          (within balance)

collateral_after = 10_000 − 3_850 = 6_150
debt_after       =  7_000 − 3_500 = 3_500
```

### Post-Liquidation Invariants

| Invariant | Expression | Result |
|-----------|-----------|--------|
| No value created | seized ≤ collateral\_before | 3 850 ≤ 10 000 ✓ |
| Debt reduced by repaid | debt\_after = debt\_before − repaid | 3 500 = 7 000 − 3 500 ✓ |
| Collateral reduced by seized | col\_after = col\_before − seized | 6 150 = 10 000 − 3 850 ✓ |
| Position improved | HF\_after ≥ HF\_before\_liq | (verified in test) ✓ |

### Step 5 — Borrower Repays Remaining Debt

```
cross_asset_repay(user, asset_B, 3_500)
→ debt_balance[B] = 0
→ HF = HEALTH_FACTOR_NO_DEBT  (no debt)
```

### Step 6 — Withdraw Remaining Collateral

```
cross_asset_withdraw(user, asset_A, 6_150)
→ collateral_balance[A] = 0
→ Position fully closed ✓
```

### Test Coverage (cross_asset_e2e_test.rs)

| Test | Scenario |
|------|---------|
| `e2e_deposit_borrow_repay_withdraw_full_lifecycle` | Happy path |
| `e2e_price_shock_collateral_crash_makes_position_liquidatable` | Col price halved |
| `e2e_price_shock_debt_spike_makes_position_liquidatable` | Debt price doubled |
| `e2e_post_liquidation_invariants_no_value_created` | Invariant assertions |
| `e2e_exactly_at_liquidation_threshold` | HF = 10\_000 boundary |
| `e2e_deep_underwater_seizure_capped_at_available_collateral` | Full seizure clamp |
| `e2e_partial_liquidation_then_full_repay_and_withdraw` | Full lifecycle incl. liq |
| `e2e_two_collateral_one_debt_shock` | Multi-collateral aggregate HF |
| `e2e_user_isolation_shock_does_not_bleed_to_other_user` | G-7 isolation |
| `e2e_withdraw_blocked_when_hf_below_threshold_after_shock` | Withdraw blocked |
| `e2e_borrow_blocked_when_hf_below_threshold_after_shock` | Borrow blocked |
