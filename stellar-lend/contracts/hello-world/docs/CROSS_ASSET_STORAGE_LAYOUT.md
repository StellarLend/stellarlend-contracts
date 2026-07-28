# Cross-Asset Module Storage Layout

This document describes the persistent storage structure of the cross‑asset module in the StellarLend **hello-world** contract (`src/cross_asset.rs`).

## Overview

All cross‑asset storage keys are defined in a single `#[contracttype] enum CrossAssetDataKey` in [`src/cross_asset.rs`](../src/cross_asset.rs). All keys use the `persistent()` storage tier and require layout stability across contract upgrades.

> **Note:** A related standalone crate, `stellar-lend/cross_asset_test`, shares the asset-registry / supply-debt balance key shapes (`Config`, `AssetList`, `UserSupply`, …) but does **not** include hello-world’s borrow-isolation cap key (`MaxDebtAssetsPerUser`). This doc describes **hello-world only**.

## Storage Map

| Key (`CrossAssetDataKey`) | Storage tier | Value type | Default if absent | Writers / owners | Upgrade‑sensitive |
|---------------------------|--------------|------------|-------------------|------------------|-------------------|
| `Config(AssetKey)` | `persistent()` | `AssetConfig` | (returns `AssetNotFound` error) | `initialize_asset`, `update_asset_config`, `update_asset_price` | Yes |
| `AssetList` | `persistent()` | `Vec<AssetKey>` | Empty vector | `initialize_asset` | Yes |
| `UserSupply(AssetKey, Address)` | `persistent()` | `i128` | 0 | `cross_asset_deposit`, `cross_asset_withdraw` | Yes |
| `UserDebt(AssetKey, Address)` | `persistent()` | `i128` | 0 | `cross_asset_borrow`, `cross_asset_repay` | Yes |
| `TotalSupply(AssetKey)` | `persistent()` | `i128` | 0 | `cross_asset_deposit`, `cross_asset_withdraw` | Yes |
| `TotalDebt(AssetKey)` | `persistent()` | `i128` | 0 | `cross_asset_borrow`, `cross_asset_repay` | Yes |
| `MaxDebtAssetsPerUser` | `persistent()` | `u32` | Unlimited (`None` / key absent) | `set_max_debt_assets_per_user` | Yes |

### What is *not* a hello-world key

hello-world does **not** store a `UserDebtAssets(Address)` list under `CrossAssetDataKey`. That shape exists on the separate **lending** contract (`DataKey::UserDebtAssets`). hello-world’s isolation tier is the scalar cap `MaxDebtAssetsPerUser` only.

## TTL Policy

The cross‑asset module does not currently implement explicit TTL extension helpers; all persistent entries follow normal Soroban storage lifetime and rent renewal.

## Upgrade and Migration Notes

- **Append‑only**: New storage key variants must be added to the end of `CrossAssetDataKey`.
- **Structural stability**: The `AssetConfig` struct must preserve field ordering and types across upgrades.
- **Default values**: Absent numeric keys (`UserSupply`, `UserDebt`, `TotalSupply`, `TotalDebt`) are treated as 0. Absent `MaxDebtAssetsPerUser` means no cap.

### Known field rename: `collateral_factor` → `collateral_factor_bps`

As of the per-asset collateral-factor tiering PR (closes #1121, see
[`COLLATERAL_FACTOR_TIERS.md`](./COLLATERAL_FACTOR_TIERS.md)), the
`AssetConfig` struct field `collateral_factor: i128` was renamed to
`collateral_factor_bps: i128`. The type, ordering, and size are unchanged
— the only difference is the field name — but **Soroban's contracttype
serialisation hashes the field name into the storage slot key**, so any
contract already deployed with the old field name will reject old
`AssetConfig` records when loading them.

Migration policy:

- A contract deployed **before** the rename must run a one-shot storage
  migration before upgrading to the rename version (read each `Config`
  entry, drop it, re-write with the new field name).
- A contract deployed **after** the rename is safe to upgrade to any
  future version that preserves this field name.
- Future renames should add the new field rather than rename, to keep the
  invariant intact; this rename is a one-off documented exception.
