# Cross-Asset Operations

The Cross-Asset implementation in StellarLend allows users to interact with multiple assets within a single position. This provides better capital efficiency by aggregating all collateral value to support a diversified debt portfolio.

## Key Features

- **Unified Position Logic**: All collateral assets contribute to a single USD-denominated borrowing capacity.
- **Risk Management**: Each asset has its own Loan-to-Value (LTV) and Liquidation Threshold (LT).
- **Asset Specificity**: Supports `set_asset_params` for admin configuration of LTV, LT, and price feeds.
- **Aggregate Health Factor**: HealthFactor = (Σ CollateralValue_i * LTV_i) / Σ DebtValue_j.

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

The contract’s view function `get_health_factor` will return `16,900` for this position.

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

## Cross‑Linking
- See the formal risk parameters in [CROSS_ASSET_RULES.md](../docs/CROSS_ASSET_RULES.md).
- Overview of the lending module is in the main [README.md](README.md).
