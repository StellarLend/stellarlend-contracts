# Cross-Asset Operations

The Cross-Asset implementation in StellarLend allows users to interact with multiple assets within a single position. This provides better capital efficiency by aggregating all collateral value to support a diversified debt portfolio.

## Key Features

- **Unified Position Logic**: All collateral assets contribute to a single USD-denominated borrowing capacity.
- **Risk Management**: Each asset has its own Loan-to-Value (LTV) and Liquidation Threshold (LT).
- **Asset Specificity**: Supports `set_asset_params` for admin configuration of LTV, LT, and price feeds.
- **Aggregate Health Factor**: HealthFactor = (Σ CollateralValue_i * LTV_i) / Σ DebtValue_j.
- **Isolation Mode**: Riskier assets can be flagged as *isolated*, capping their collateral contribution at a per-asset debt ceiling and preventing cross-margining with other collateral.

## Aggregation Pipeline

The cross‑asset module aggregates collateral and debt across multiple assets to compute a unified health factor.

**Per‑Asset Valuation**
- Collateral amount is multiplied by the oracle price (`price_record.price`) and divided by `PRICE_DIVISOR = 10_000_000` to obtain a USD‑denominated value.
- The resulting value is then multiplied by the asset's liquidation threshold expressed in basis points (`params.liquidation_threshold_bps`). This yields the *weighted collateral* contribution.

**Debt Valuation**
- Effective debt is calculated via `crate::debt::effective_debt`, then multiplied by the oracle price and divided by `PRICE_DIVISOR` to obtain USD debt value.
- All debt values are summed to `total_debt_value`.

**Health Factor Calculation**
```
if total_debt_value == 0 {
    health_factor = HEALTH_FACTOR_NO_DEBT; // sentinel = 100_000_000
} else {
    // `weighted_collateral` is the sum of weighted collateral values.
    health_factor = weighted_collateral / total_debt_value;
}
```
The `HEALTH_FACTOR_SCALE = 10_000` represents a health factor of 1.0 (100 %).

**Worked Example**
- Collateral: 100 USDC @ $1.00, LT = 90 % (9_000 bps)
- Collateral: 1 ETH @ $2,000.00, LT = 80 % (8_000 bps)
- Debt: 1,000 USDC @ $1.00

Calculation:
- USDC value = 100 * 1_000_0000 / 10_000_000 = 100
- ETH value = 1 * 2_000_000_000 / 10_000_000 = 2000
- Weighted collateral = (100 * 9_000) + (2000 * 8_000) = 900_000 + 16_000_000 = 16_900_000
- Debt value = 1_000 * 1_000_0000 / 10_000_000 = 1_000
- Health factor = 16_900_000 / 1_000 = 16_900 (i.e., 1.69 × 10_000)

This example matches the computation performed by `compute_aggregate_health_factor`.

## Isolation Mode

### Overview

Isolation mode is a risk-containment control for assets that are volatile, thinly traded, or newly listed. An isolated asset may still be used as collateral, but under two additional constraints:

1. **Debt ceiling** — the total debt backed by that asset across all users cannot exceed `isolation_debt_ceiling` (denominated in the asset's raw units). Borrows that would push past the ceiling are rejected with `IsolationCeilingExceeded`.
2. **No cross-margining** — when a user posts an isolated asset as their collateral in `borrow_against_collateral`, the ceiling check is applied per-asset. The isolated asset cannot amplify borrowing power through aggregation with other collateral types.

Normal (non-isolated) assets remain fully fungible in the cross-asset position model.

### Isolation-Mode Storage

Two new `DataKey` variants are used:

| Key | Storage tier | Description |
|-----|-------------|-------------|
| `AssetIsolation(Address)` | `persistent` | Stores `IsolationConfig { isolated: bool, isolation_debt_ceiling: i128 }` |
| `IsolationDebt(Address)` | `persistent` | Running total of debt currently backed by this isolated asset |

### Admin API

#### `set_asset_isolation(asset, isolated, isolation_debt_ceiling) -> Result<(), LendingError>`

Enable or disable isolation mode for `asset`. Admin-only.

- `isolated = true, isolation_debt_ceiling > 0` — enables isolation with the given ceiling.
- `isolated = false` — disables isolation; the ceiling value is stored but ignored.
- Returns `InvalidIsolationCeiling` if `isolated = true` and `isolation_debt_ceiling ≤ 0`.

#### `get_asset_isolation(asset) -> Option<IsolationConfig>`

Returns the isolation configuration for `asset`. Returns `None` when no configuration has been set.

#### `get_isolation_debt(asset) -> i128`

Returns the current running total of debt backed by `asset` acting as isolated collateral. Returns `0` when unconfigured or when no debt has been recorded.

#### `check_isolation_ceiling(collateral_asset, borrow_amount) -> Result<(), LendingError>`

Read-only view that returns `Ok(())` if the given `borrow_amount` would not breach the ceiling, or `IsolationCeilingExceeded` if it would. Useful for frontends and off-chain tooling to preflight a borrow.

### Cross-Asset Borrow / Repay API

#### `borrow_against_collateral(user, amount, collateral_asset) -> Result<i128, LendingError>`

Isolation-aware borrow. In addition to the standard pause/emergency/min-borrow checks:

1. Calls `check_isolation_ceiling_internal` — rejects with `IsolationCeilingExceeded` if the ceiling would be breached.
2. On success, increments `IsolationDebt(collateral_asset)` by the net new principal.

Non-isolated assets pass through with no additional overhead.

#### `repay_against_collateral(user, amount, collateral_asset) -> Result<i128, LendingError>`

Isolation-aware repay. On success, decrements `IsolationDebt(collateral_asset)` by the net principal reduction. The counter uses saturating subtraction and will not go below zero.

### Worked Example

```
Setup:
  EXOTIC token — isolated = true, isolation_debt_ceiling = 100_000

User A borrows 60_000 against EXOTIC:
  IsolationDebt(EXOTIC) = 0 + 60_000 = 60_000  ✓ (60_000 ≤ 100_000)

User B borrows 40_000 against EXOTIC:
  IsolationDebt(EXOTIC) = 60_000 + 40_000 = 100_000  ✓ (100_000 ≤ 100_000)

User C tries to borrow 1 against EXOTIC:
  IsolationDebt(EXOTIC) = 100_000 + 1 = 100_001  ✗ → IsolationCeilingExceeded

User A repays 30_000:
  IsolationDebt(EXOTIC) = 100_000 − 30_000 = 70_000

User C tries again with 30_000:
  IsolationDebt(EXOTIC) = 70_000 + 30_000 = 100_000  ✓
```

### Security Notes

- **Pre-mutation check**: `check_isolation_ceiling_internal` runs before any state is mutated. If the check fails the borrow is fully rejected — no partial state changes occur.
- **Ceiling is aggregate, not per-user**: The ceiling applies to the combined outstanding debt across all users who posted the isolated asset as collateral.
- **Ceiling change is immediate**: Lowering the ceiling below the current `IsolationDebt` does not liquidate existing positions, but it does block further borrowing until the outstanding debt falls back under the new ceiling.
- **Disabling isolation is immediate**: Setting `isolated = false` removes all ceiling enforcement for subsequent borrows; existing `IsolationDebt` entries are left in storage but no longer consulted.
- **Non-isolated path unchanged**: `borrow` and `repay` (the original single-collateral functions) do not touch `IsolationDebt`. Only the `_against_collateral` variants participate in isolation tracking.

## Operations

### `set_asset_params`
Admin only function to configure an asset's parameters.
- `ltv`: Maximum amount that can be borrowed against the asset (basis points).
- `liquidation_threshold`: Point at which the asset becomes eligible for liquidation (basis points).
- `price_feed`: The oracle address providing the asset's price.
- `debt_ceiling`: Total system-wide debt allowed for this asset.
- **Event**: Emits `AssetParamsSetEvent`.

### `deposit_collateral_asset`
Users can deposit any supported asset as collateral. This increases their total borrowing power based on the asset's USD value and its specific LTV.
- **Pause Check**: Blocked if `PauseType::Deposit` or `PauseType::All` is set.
- **Token Transfer**: Automatically transfers tokens from user to the contract.
- **Event**: Emits `CrossDepositEvent`.

### `borrow_asset`
Users can borrow any supported asset as long as their aggregate Health Factor remains above 1.0 (10000 basis points).
- **Pause Check**: Blocked if `PauseType::Borrow` or `PauseType::All` is set.
- **Token Transfer**: Automatically transfers tokens from the contract to the user.
- **Event**: Emits `CrossBorrowEvent`.

### `repay_asset`
Users repay borrowed assets to reduce their total debt and improve their position's Health Factor.
- **Pause Check**: Blocked if `PauseType::Repay` or `PauseType::All` is set.
- **Token Transfer**: Automatically transfers tokens from user to the contract.
- **Event**: Emits `CrossRepayEvent`.

### `withdraw_asset`
Collateral withdrawal is allowed only if the remaining position stays healthy (Health Factor > 1.0).
- **Pause Check**: Blocked if `PauseType::Withdraw` or `PauseType::All` is set.
- **Token Transfer**: Automatically transfers tokens from the contract to the user.
- **Event**: Emits `CrossWithdrawEvent`.

### `get_cross_position_summary`
Returns a summary of the user's position:
- `total_collateral_usd`: Aggregated value of all collateral.
- `total_debt_usd`: Aggregated value of all debt.
- `health_factor`: Unified risk indicator for the entire position.

## Valuation and Oracle Requirements

### Valuation Examples

The protocol uses a deterministic valuation model for multi-collateral positions. The total collateral value and health factor are calculated by aggregating per-asset values.

**Example 1: Multi-Collateral Position**
- **User Deposits**:
  - 100 USDC at $1.00 (LTV 90%)
  - 1 ETH at $2,000.00 (LTV 80%)
- **Calculations**:
  - USDC Value: $100.00
  - ETH Value: $2,000.00
  - Total Collateral Value: $2,100.00
  - Weighted Collateral: ($100 * 0.9) + ($2,000 * 0.8) = $90 + $1,600 = $1,690.00
- **User Borrows**:
  - 1,000 USDC ($1,000.00)
- **Health Factor**:
  - $1,690 / $1,000 = 1.69 (16,900 bps)

### Oracle Requirements for Multi-Collateral

To ensure safe and deterministic valuation, the oracle must satisfy the following:
1. **Freshness**: Prices must be updated within the configured staleness window (default 1 hour). If any asset in a position has a stale price, the entire position's summary query will fail to prevent unsafe operations.
2. **Precision**: All prices are scaled to 7 decimals (`10,000,000 = $1.00`) within the cross-asset module to maintain consistency.
3. **Availability**: Both primary and fallback feeds are supported. Fallback is automatically used if the primary is stale or missing.
4. **Monotonicity**: Valuation must remain monotonic with respect to price changes. A price increase in collateral must never decrease the health factor.

## Security Considerations

- **Price Feeds**: The implementation relies on price oracles. Ensure oracles are reliable and current.
- **Rounding**: All calculations use conservative rounding (floor for collateral value and health factor) to protect the protocol.
- **Auth**: Critical operations require user or admin authorization.
- **Isolation Mode**: Isolated assets are capped, non-amplifying collateral. The ceiling check is pre-mutation and atomic. See the Isolation Mode section above for full security properties.
