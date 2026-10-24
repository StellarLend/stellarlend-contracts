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

## Status & Traceability

This document describes the **implemented** behaviour of the
`stellar-lend/contracts/hello-world/src/risk_params.rs` module. The paced
rate-change cap, bounds checks, and admin-only enforcement are all exercised
in code and covered by tests.

### Implementation constants (`risk_params.rs`)

| Constant                 | Value (bps) | Meaning                                  |
|--------------------------|-------------|------------------------------------------|
| `MAX_CHANGE_BPS`         | `1_000`     | 10% per-call cap (Paced Rate Changes)     |
| `MIN_COLLATERAL_RATIO_FLOOR` | `10_000`| 100% — minimum for any collateralized position |
| `MAX_COLLATERAL_RATIO`   | `100_000`   | 1000% — upper bound on MCR               |
| `MAX_LIQUIDATION_THRESHOLD` | `100_000`| 1000% — upper bound on liquidation threshold (must stay ≤ MCR) |
| `MIN_CLOSE_FACTOR`       | `1`         | A zero close factor would block all liquidations |
| `MAX_CLOSE_FACTOR`       | `10_000`    | 100%                                     |
| `MIN_LIQUIDATION_INCENTIVE` | `0`      | Liquidators can still claim no bonus     |
| `MAX_LIQUIDATION_INCENTIVE`  | `5_000` | 50%                                       |

### Cap coverage

The 10% delta cap is applied to **all four** parameters via `validate_change` —
not only `min_collateral_ratio` and `liquidation_threshold`.

Violations surface as `RiskParamsError::ParameterChangeTooLarge`, which the
contract entrypoint (`HelloContract::set_risk_params`) maps to
`RiskManagementError::ParameterChangeTooLarge`.

### Regression coverage

`stellar-lend/contracts/hello-world/src/risk_params_paced_change_test.rs`
exercises:

- The documented default values are set by `initialize_risk_params`.
- A 10% upward change is accepted for every parameter.
- A change exceeding 10% upward is rejected with `ParameterChangeTooLarge`.
- A 10% downward change is accepted; >10% downward is rejected.
- Out-of-range values are still rejected (e.g. `close_factor > 100%`,
  `min_collateral_ratio < 100%`).
- Previously-unguarded parameters (`close_factor`, `liquidation_incentive`)
  are now pace-limited (regression guard for the historical direct-set path).
