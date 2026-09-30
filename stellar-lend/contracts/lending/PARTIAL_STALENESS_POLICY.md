# Partial Staleness Policy (Cross-Asset)

This document defines how the lending contract treats **partial oracle
staleness**: the situation where one collateral or debt asset on a multi-asset
position has a stale price while other legs remain fresh.

Canonical enforcement site: [`src/cross_asset.rs`](src/cross_asset.rs)
(`ensure_position_prices_fresh`, called from `borrow_asset_internal`).

Per-asset freshness is evaluated by `get_price_for_asset`, which rejects a
price when:

```text
now > record.timestamp + DEFAULT_ORACLE_MAX_AGE_SECS
```

with typed error `LendingError::StaleOracleTimestamp` (code `5002`).

---

## Why partial staleness is dangerous

Cross-asset health aggregates every collateral and debt leg:

```text
health_factor = Σ (coll_i × price_i × threshold_i) / Σ (debt_j × price_j)
```

If even one `price_i` / `price_j` is stale, the true value of the position is
**unknown**. A borrow that only re-checks the asset being borrowed could still
proceed against an inflated (or otherwise untrustworthy) collateral valuation,
opening an under-collateralised debt position.

The protocol therefore **fails closed** on risk-increasing operations when any
leg is stale, and **fails open** on risk-reducing operations so users can
always de-risk.

---

## Fail-closed vs fail-open table

| Operation | Entry point | Partial stale leg | Policy | Error / outcome |
|-----------|-------------|-------------------|--------|-----------------|
| Cross-asset **borrow** | `borrow_asset` → `borrow_asset_internal` | Any collateral or debt leg on the position, **or** the asset being borrowed | **Fail closed** | `LendingError::StaleOracleTimestamp` |
| Cross-asset **repay** | `repay_asset` → `repay_asset_internal` | Any leg | **Fail open** | Repay succeeds (no price scan) |
| Cross-asset **withdraw** | `withdraw_asset` → `withdraw_asset_internal` | Any leg used by post-withdraw HF | **Fail closed** (via `compute_aggregate_health_factor` → `get_price_for_asset`) | `StaleOracleTimestamp` / `PriceFeedNotFound` |
| Cross-asset **deposit** | `deposit_collateral_asset` → `deposit_collateral_asset_internal` | N/A (no valuation gate) | **Fail open** | Deposit succeeds without price scan |
| Aggregate HF / position views | `compute_aggregate_health_factor`, `get_cross_position_value`, `get_cross_debt_value` | Any scanned leg | **Fail closed** | `StaleOracleTimestamp` / `PriceFeedNotFound` |
| Legacy single-asset **borrow** / **liquidate** | `borrow`, `liquidate` | Valuation collateral or debt asset | **Fail closed** (via `require_fresh_valuation_prices`) | `StaleOracleTimestamp` |

### Summary rules

| Class | Examples | On partial staleness |
|-------|----------|----------------------|
| **Risk-increasing** | Borrow more debt, withdraw collateral | **Fail closed** — reject until every contributing price is fresh |
| **Risk-reducing** | Repay debt | **Fail open** — always allowed; no oracle dependency |
| **Risk-neutral / additive collateral** | Deposit collateral | **Fail open** — no borrow-power claim until a later risk-increasing op re-validates prices |
| **Read / valuation** | Health factor, position value | **Fail closed** — never report a number built from a stale leg |

---

## Borrow-path enforcement details

`borrow_asset_internal` calls `ensure_position_prices_fresh(env, user, asset)`
**after** auth and **before** any debt mutation:

1. Scan every address in `UserCollateralAssets(user)` via `get_price_for_asset`.
2. Scan every address in `UserDebtAssets(user)` via `get_price_for_asset`.
3. Scan the `asset` being borrowed (covers first borrow of a new debt asset
   that is not yet on the debt list).

Any stale or missing price aborts the whole borrow with a typed
`LendingError`. The subsequent `compute_aggregate_health_factor` check remains
as defense-in-depth for post-borrow HF.

Repay intentionally **does not** call this helper.

---

## Tests

Regression coverage lives in
[`src/partial_staleness_guard_test.rs`](src/partial_staleness_guard_test.rs):

| Test | Expected |
|------|----------|
| Fresh borrowed asset, stale collateral | Borrow rejected (`StaleOracleTimestamp`) |
| All legs fresh | Borrow allowed |
| Stale existing debt leg | Borrow rejected (`StaleOracleTimestamp`) |
| Stale legs on repay | Repay allowed |

Related single-asset staleness coverage:
[`src/oracle_staleness_test.rs`](src/oracle_staleness_test.rs).

---

## Operational notes

- Keep oracle pushers well inside `DEFAULT_ORACLE_MAX_AGE_SECS` (default **3600 s**).
- A single stalled feed blocks **new borrows** (and HF-gated withdraws) for any
  user holding that asset on either side of a position, until the feed is
  refreshed.
- Users can still **repay** and **deposit** during a partial outage, which is
  the intended escape hatch for reducing exposure.
