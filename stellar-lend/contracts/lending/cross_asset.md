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

## Security Considerations

- **Price Feeds**: The implementation relies on price oracles. Ensure oracles are reliable and current.
- **Rounding**: All calculations use conservative rounding to protected the protocol.
- **Auth**: Critical operations require user or admin authorization.
