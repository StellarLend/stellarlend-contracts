# Lending Contract Valuation

## Single-position collateral and debt valuation

The legacy single-position interface can be configured with
`set_collateral_asset` and `set_debt_asset`. When both assets are configured
and differ, borrow solvency, withdrawal solvency, liquidation eligibility,
liquidation seizure, and health-factor views use each asset's fresh oracle
price. When the debt asset is unset or matches the collateral asset, the
contract preserves legacy raw-unit calculations.

Oracle prices use a fixed seven-decimal scale: `10_000_000` represents one
value unit. Collateral value is rounded down and debt value is rounded up.
Arithmetic overflow and unavailable or stale prices fail closed on
state-changing valuation paths.

Liquidation seizure in cross-asset mode is
`repay_amount * debt_price * (10_000 + incentive_bps) / (collateral_price * 10_000)`,
rounded down to collateral token units.

`get_position` and `get_health_factor` remain schema-v1 views. The
`get_position_v2` view adds `price_stale`; with stale prices it returns the last
computable health factor and marks the result stale. If either required price
has never been reported, is invalid, or cannot be safely valued due to
arithmetic overflow, it returns a zero health factor with `price_stale = true`.
Consumers that need freshness information should use `get_position_v2`.
