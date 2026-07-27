# Cross-Asset Liquidation Notes

## Overview

In this lending protocol, a single user position tracks **one debt asset** and
**one collateral asset**. Cross-asset liquidation refers to the case where those
two assets differ — the debt token is different from the collateral token, each
with their own independent oracle price.

---

## How Cross-Asset Health Factor Works

```text
collateral_value (USD) = collateral_raw_amount * price(collateral_asset) / 1e8
debt_value       (USD) = total_debt_raw        * price(debt_asset)       / 1e8

health_factor          = (collateral_value * liq_threshold_bps / 10_000)
                         * 10_000 / debt_value
```

The health factor is computed in a common dollar unit. The two oracle prices
convert raw token amounts into comparable values. A ratio that looks healthy in
raw units may be deeply underwater in dollar terms (and vice-versa).

---

## Oracle Requirements for Cross-Asset Liquidations

| Requirement | Detail |
|-------------|--------|
| **Both prices required** | If either the debt or collateral oracle price is absent (or returns 0), `HF = 0`, which the protocol treats as "cannot evaluate → not liquidatable". This prevents phantom liquidations. |
| **Staleness applies to both** | Staleness is checked per-asset via the hardened oracle module. A stale collateral price causes HF = 0 even when the debt price is fresh. |
| **Price scale is 1e8** | `100_000_000` = $1.00. Both prices must be in the same scale. |
| **Price must be > 0** | Zero or negative prices are treated as absent. |
| **Legacy oracle fallback** | If the hardened oracle module has no price for an asset, the legacy `set_oracle` path is tried. Both paths produce the same 1e8-scaled result. |

---

## Borrow-Time vs Liquidation-Time Checks

The collateral adequacy check at **borrow time** operates in **raw units**:

```text
required_collateral_raw = borrow_raw * 15000 / 10000   (150% minimum)
```

This is price-agnostic. As a result:
- A position that is safe in raw units may become liquidatable as oracle prices shift.
- A position whose raw ratio is below 150% cannot be opened at all.
- **Tests must set raw amounts that satisfy 150%** regardless of the oracle prices used.

The liquidation eligibility check at **liquidation time** operates in **dollar values**
(using oracle prices), not raw units. The two checks use different scales.

---

## Cross-Asset Scenarios and Coverage

| Scenario | Oracle Behaviour | Expected Outcome |
|----------|-----------------|-----------------|
| Collateral appreciates relative to debt | Both prices present; col >> debt | Healthy, no liquidation |
| Collateral crashes | col price drops sharply | HF < 1.0, liquidation opens |
| Debt price spikes | debt price increases | HF drops below 1.0 |
| Close-factor cap (cross-price) | Prices differ; cap still in raw debt units | Max repay = total_debt_raw * close_factor |
| Collateral near-zero crash | col price → 0 | Seizure capped at available balance |
| Full liquidation (100% CF) | Both prices present | Debt = 0, HF = NO_DEBT sentinel |
| Missing debt oracle | Debt price absent | HF = 0 → liquidation rejected |
| Missing collateral oracle | Collateral price absent | HF = 0 → liquidation rejected |

---

## Incentive (Bonus) Calculation

The seized collateral is computed **in raw units**, not dollar-adjusted:

```text
seized_raw = min(repay_raw * (10_000 + incentive_bps) / 10_000, collateral_balance)
```

This means that when the collateral token is cheap relative to the debt token,
the liquidator receives more collateral *units* but the same dollar value.
When collateral is expensive, the liquidator receives fewer collateral units for
the same dollar-equivalent repayment.

The `min()` bound ensures the seizure never exceeds on-chain collateral, preventing
negative balances even after extreme oracle crashes.

---

## Security Notes

1. **Oracle manipulation risk**: A manipulated collateral oracle can make a healthy
   position appear liquidatable, enabling unjust liquidation. Mitigation: use
   time-weighted average prices (TWAP) or multi-oracle aggregation.

2. **Staleness as a denial-of-service**: If an oracle feed goes stale, all positions
   using that asset become non-liquidatable (HF = 0). Under-water positions cannot
   be resolved until the oracle is refreshed. Configure fallback oracles for critical
   assets.

3. **Collateral seizure is raw-unit, not dollar-unit**: In extreme cross-asset price
   divergence, the 10% incentive in raw units may represent a very different dollar
   bonus than intended. Protocol operators should monitor incentive dollar-value at
   current market prices.

4. **Minimum borrow constraint**: The protocol requires `borrow_amount >= 1000` by
   default. Tests must use amounts above this threshold.

5. **150% raw collateral ratio**: All borrows must satisfy `collateral_raw >= borrow_raw * 1.5`
   regardless of oracle prices. This is a hard protocol invariant enforced at borrow time.
