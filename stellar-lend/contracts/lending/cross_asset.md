# Cross-Asset Operations

The Cross-Asset implementation in StellarLend allows users to interact with multiple assets within a single position. This provides better capital efficiency by aggregating all collateral value to support a diversified debt portfolio.

## Key Features

- **Unified Position Logic**: All collateral assets contribute to a single USD-denominated borrowing capacity.
- **Risk Management**: Each asset has its own Loan-to-Value (LTV) and Liquidation Threshold (LT).
- **Asset Specificity**: Supports `set_asset_params` for admin configuration of LTV, LT, and price feeds.
- **Aggregate Health Factor**: Sum each asset's oracle-priced value into a common
  unit, apply its risk weight, and divide weighted collateral by total debt.

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

## Aggregation Formula And Scales

The cross-asset risk calculation starts by normalizing every collateral and debt
asset into the same oracle-priced value unit. The pure math helpers in
`src/math.rs` use:

| Scale | Value | Use |
|---|---:|---|
| `PRICE_SCALE` | `10_000_000` | Oracle prices, where `10_000_000 = $1.00`. |
| `BPS_SCALE` | `10_000` | Risk weights, where `10_000 = 100%`. |
| `HF_SCALE` | `10_000_000` | `math::SCALE`, where `10_000_000 = 1.0` health factor. |

For each collateral asset `i`:

```text
collateral_value_i = collateral_amount_i * price_i / PRICE_SCALE
weighted_collateral_i = collateral_value_i * liquidation_threshold_i / BPS_SCALE
```

For each debt asset `j`:

```text
debt_value_j = debt_amount_j * price_j / PRICE_SCALE
```

Aggregate across all assets:

```text
total_collateral_value = sum(collateral_value_i)
total_weighted_collateral = sum(weighted_collateral_i)
total_debt_value = sum(debt_value_j)
```

Then derive the unified health factor:

```text
if total_debt_value == 0:
    health_factor = i128::MAX
else:
    health_factor = total_weighted_collateral * HF_SCALE / total_debt_value
```

All divisions use integer floor semantics. A value below `HF_SCALE` is below
1.0 and therefore liquidatable under the shared `math::is_liquidatable` helper.
When presenting the same health factor on the 10,000 UI/BPS scale, divide the
`HF_SCALE` result by `1_000`.

For the rule and invariant view, see
[Cross-Asset Rules](../../../docs/CROSS_ASSET_RULES.md).

### Worked Example: Two Collaterals, One Debt

Assume:

| Asset | Side | Amount | Oracle price | Threshold | Value | Weighted value |
|---|---|---:|---:|---:|---:|---:|
| USDC | Collateral | `1_000` | `10_000_000` | `9_000` BPS | `1_000` | `900` |
| ETH | Collateral | `2` | `20_000_000_000` | `8_000` BPS | `4_000` | `3_200` |
| USDC | Debt | `3_000` | `10_000_000` | n/a | `3_000` | n/a |

Step by step:

```text
usdc_collateral_value = 1_000 * 10_000_000 / 10_000_000
                      = 1_000
eth_collateral_value  = 2 * 20_000_000_000 / 10_000_000
                      = 4_000

usdc_weighted = 1_000 * 9_000 / 10_000
              = 900
eth_weighted  = 4_000 * 8_000 / 10_000
              = 3_200

total_weighted_collateral = 900 + 3_200
                          = 4_100

debt_value = 3_000 * 10_000_000 / 10_000_000
           = 3_000

health_factor = 4_100 * 10_000_000 / 3_000
              = 13_666_666
```

The position is healthy because `13_666_666 > 10_000_000`. Displayed on the
10,000 scale, the same result is `13_666` after floor division.

### Worked Example: Single-Asset Degenerate Case

A cross-asset position with one collateral asset and one debt asset collapses to
the same arithmetic as `compute_health_factor(collateral_value, debt_value,
liquidation_threshold_bps)`.

```text
collateral_value = 1_000
debt_value = 400
liquidation_threshold = 8_000

weighted_collateral = 1_000 * 8_000 / 10_000
                    = 800

health_factor = 800 * 10_000_000 / 400
              = 20_000_000
```

The health factor is exactly `2.0` on the `HF_SCALE` scale.

## Oracle Requirements For Multi-Asset Positions

To ensure safe and deterministic valuation, the oracle must satisfy the following:
1. **Freshness**: Prices must be updated within the configured staleness window (default 1 hour). If any asset in a position has a stale price, the entire position's summary query will fail to prevent unsafe operations.
2. **Precision**: All prices are scaled to 7 decimals (`10,000,000 = $1.00`) within the cross-asset module to maintain consistency.
3. **Availability**: Both primary and fallback feeds are supported. Fallback is automatically used if the primary is stale or missing.
4. **Monotonicity**: Valuation must remain monotonic with respect to price changes. A price increase in collateral must never decrease the health factor.

## Security Considerations

- **Price Feeds**: The implementation relies on price oracles. Ensure oracles are reliable and current.
- **Rounding**: All calculations use conservative rounding (floor for collateral value and health factor) to protect the protocol.
- **Auth**: Critical operations require user or admin authorization.
