# Risk Parameters

StellarLend exposes admin-only controls for liquidation risk parameters while preserving the historical defaults.

| Parameter | Storage key | Default | Bounds | Used by |
|---|---|---:|---:|---|
| Liquidation threshold | `DataKey::LiquidationThresholdBps` | `8000` | `1..=10000` | `liquidate`, `get_position`, `get_health_factor` |
| Close factor | `DataKey::CloseFactorBps` | `5000` | `1..=10000` | `liquidate` |
| Liquidation incentive | `DataKey::LiquidationIncentiveBps` | `1000` | `0..=5000` | `liquidate` |

Setters require the current admin signature and reject out-of-range values with `LendingError::InvalidFeeBps` before writing storage. Getters return the defaults until an admin stores an override.

Operationally, risk teams can raise the liquidation threshold to make positions liquidatable sooner, lower the close factor to reduce single-transaction borrower impact, or tune the incentive to balance liquidator participation against borrower collateral loss.
