# Risk Parameters

The `risk_params` module provides critical parameter configuration and safety enforcement for the Stellar-Lend protocol. It introduces administrative flexibility to define the safe operation boundaries, while enforcing rigid limits to prevent catastrophic invalid configurations.

## Parameters

The following parameters are controlled by this module:
- **Minimum Collateral Ratio (MCR)**: The threshold collateral percentage users must deposit to stay in good standing. It is represented in basis points (`11_000` = `110%`). Minimum bound: `100%`. Maximum bound: `500%`.
- **Liquidation Threshold**: The specific point at which a borrower is considered distressed and eligible for liquidation. Represented in basis points (`10_500` = `105%`). This threshold *must always be smaller than* or equal to the MCR.
- **Close Factor**: The maximum proportion of a distressed borrower's debt that a liquidator can repay in a single transaction. Represented in basis points (`5_000` = `50%`). Values range from `0%` to `100%`.
- **Liquidation Incentive**: The bonus given to liquidators for helping clear bad debt from the protocol. Represented in basis points (`1_000` = `10%`). Values range from `0%` to `50%` safely.

## Safety Measures

The module natively enforces safety boundaries:
1. **Admin Only**: Parameter changes are protected by standard admin authentication.
2. **Bounds Checking**: Parameters cannot be set to mathematically invalid or unsafe extremes (e.g. negative liquidation parameters or close factors above `100%`).
3. **Paced Rate Changes**: Updates are subject to a maximum change delta of `10%` per update. This mitigates governance attacks or errors by preventing instant drastic protocol disruption.

## Interacting with the Module

You can request parameter values programmatically from `HelloContract` across standard read interfaces:
- `get_min_collateral_ratio()`
- `get_liquidation_threshold()`
- `get_close_factor()`
- `get_liquidation_incentive()`

Admins update via:
- `set_risk_params(admin, optional_min_collateral_ratio, optional_liquidation_threshold, optional_close_factor, optional_liquidation_incentive)`
