# Per-Asset Debt Accrual (cross-asset borrowing)

Cross-asset positions borrow a **specific asset**, so their debt is denominated
in that asset's raw units. Adding those units to the protocol-wide
`DataKey::TotalDebt` — and accruing them against the single global
`DataKey::BorrowIndex` whose rate comes from single-asset
`TotalDebt / TotalDeposits` utilisation — mixes incompatible units and makes
every borrower's rate depend on unrelated activity.

This document describes the per-asset accounting that replaced it.

## 1. State layout

| Key | Storage | Meaning |
|-----|---------|---------|
| `BorrowIndexAsset(asset)` | instance | Per-asset borrow index, scale `debt::INDEX_SCALE` (1.0 = `10_000_000`) |
| `LastIndexUpdateAsset(asset)` | instance | Timestamp of the last per-asset index update |
| `TotalDebtAsset(asset)` | persistent | Sum of stored (settled) principals across all users, in `asset` units |
| `TotalCollateralAsset(asset)` | persistent | Per-asset supplied collateral, in `asset` units (utilisation denominator) |

The global `BorrowIndex`, `LastIndexUpdate`, and `TotalDebt` are **never**
read or written by cross-asset borrow, repay, settlement, or liquidation.
Single-asset flows (`borrow`, `repay`, `borrow_against_collateral`, …) keep
using them unchanged.

## 2. Rate

`debt::compute_asset_borrow_rate` is a pure function of storage:

```text
utilisation_bps = clamp(TotalDebtAsset(asset) × 10_000 / TotalCollateralAsset(asset), 0, 10_000)
rate_bps        = rate_model::compute_borrow_rate(utilisation_bps, RateParams)
```

- A zero-supply asset reports utilisation `0` (no division by zero).
- Rate parameters are global for now (per-asset parameters are out of scope).
  When no `RateParams` are configured the rate is `debt::DEFAULT_APR_BPS`.
- The per-asset rate deliberately **bypasses the global rate-smoothing state**
  (`RateModelKey::LastRate`, …). That state is a single global value; feeding
  it per-asset utilisation would couple the assets again. It is also a write,
  which read-only views must not perform.

Two assets at different utilisation therefore accrue at different rates, and
single-asset activity (which only moves `TotalDebt`, `TotalDeposits`, and the
global index) cannot change cross-asset accrual.

## 3. Settlement

`cross_asset::settle_asset_position(env, asset, position, now)` is the single
mutating settlement helper used by `borrow_asset`, `repay_asset`,
`liquidate_cross_asset`, and the two-phase borrow/repay paths:

1. Compute `asset`'s live rate (§2).
2. Advance and persist `BorrowIndexAsset(asset)` with `debt::accrue_index`
   (monotonic, overflow-guarded exactly like the global index).
3. Settle the position with `debt::settle_position`: `principal × index /
   snapshot`, then refresh the snapshot to the new index.
4. Credit the insurance fund its configured share of the interest.

`get_cross_position_summary`, `get_cross_debt_value`,
`compute_aggregate_health_factor`, and `get_asset_debt_view` use
`project_asset_position_debt`, which performs steps 1–3 **without writing**.
Both paths use the same rate, the same stored index, and the same elapsed
seconds, so a view at timestamp `T` returns exactly the principal a repayment
at `T` would settle — and views never settle positions or advance indices.

`TotalDebtAsset(asset)` is adjusted by the same principal delta as the user's
position on every mutating touch, including settled interest. This preserves

```text
Σ_users user debt(asset) == TotalDebtAsset(asset)
```

across borrow / repay / liquidation sequences, where `user debt` is the stored
(settled-at-last-touch) principal.

## 4. Migration: snapshots against the old global index

The contract under scope has no upgrade entrypoint, so the release is deployed
as a fresh instance and starts with empty per-asset state. The rules below
still define exactly how an existing state would be interpreted, and are what
the implementation guarantees:

1. **Rebasing is implicit, not a rewrite.** The first time an asset is touched,
   `BorrowIndexAsset(asset)` is seeded with the *current global*
   `DataKey::BorrowIndex` value (not `INDEX_SCALE`), and
   `LastIndexUpdateAsset(asset)` is seeded with the current timestamp.
2. A pre-existing cross-asset position's `borrow_index_snapshot` is therefore
   already expressed on the same scale as the new per-asset index. Its first
   settlement charges `principal × new_index / old_global_snapshot` — interest
   the global index had accrued up to the migration point plus per-asset
   interest from the seed time onward. No manual snapshot migration is needed,
   and no interest is invented or dropped.
3. Once the seed is written, cross-asset flows no longer advance the global
   index at all. The global index is left frozen for the cross-asset book and
   continues to serve single-asset positions only.
4. Positions with `borrow_index_snapshot == 0` (pre-borrow-index records) keep
   the legacy elapsed-time accrual for one settlement, then adopt the per-asset
   index snapshot.
5. `TotalCollateralAsset(asset)` is populated from deposits made after this
   release. Collateral supplied before the counter existed is **not**
   backfilled: utilisation for such an asset reads `0` (base rate) until new
   deposits are recorded, and withdrawals saturate the counter at zero instead
   of failing. A live migration should seed the counter from off-chain supply
   data before enabling cross-asset borrowing.
6. Pre-existing pollution of the global `TotalDebt` is **not** retroactively
   cleaned: there is no on-chain registry of every asset ever borrowed, so the
   sum of `TotalDebtAsset` cannot be enumerated and subtracted. The global
   counter simply stops receiving cross-asset deltas. A fresh deployment (the
   path for this contract, which has no upgrade entrypoint) starts both books
   at zero and never mixes them.

## 5. Guarantees

- Cross-asset borrow, repay, and liquidation never change `TotalDebt`,
  `BorrowIndex`, or `LastIndexUpdate`.
- Per-asset indices are monotonic; a backwards clock or a negative time delta
  accrues nothing (`accrue_index` clamps).
- Index accrual keeps the global overflow guard: an index above
  `i128::MAX / INDEX_SCALE` traps rather than wrapping.
- Zero-supply assets have utilisation `0`, never a division by zero.
- Views (`get_cross_position_summary`, `get_asset_debt_view`,
  `get_asset_borrow_rate`, `get_asset_utilization_bps`) perform no storage
  writes.
