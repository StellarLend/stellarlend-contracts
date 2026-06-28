# Cross-Asset Module Storage Layout

This document describes the persistent storage structure of the cross‑asset module in the StellarLend hello‑world contract.

## Overview

All cross‑asset storage keys are defined in a single `#[contracttype] enum CrossAssetDataKey` in [`src/cross_asset.rs`](../src/cross_asset.rs). All keys use the `persistent()` storage tier and require layout stability across contract upgrades.

## Storage Map

| Key (`CrossAssetDataKey`) | Storage tier | Value type | Default if absent | Writers / owners | Upgrade‑sensitive |
|---------------------------|--------------|------------|-------------------|------------------|-------------------|
| `Config(AssetKey)` | `persistent()` | `AssetConfig` | (returns `AssetNotFound` error) | `initialize_asset`, `update_asset_config`, `update_asset_price` | Yes |
| `AssetList` | `persistent()` | `Vec<AssetKey>` | Empty vector | `initialize_asset` | Yes |
| `UserSupply(AssetKey, Address)` | `persistent()` | `i128` | 0 | `cross_asset_deposit`, `cross_asset_withdraw` | Yes |
| `UserDebt(AssetKey, Address)` | `persistent()` | `i128` | 0 | `cross_asset_borrow`, `cross_asset_repay` | Yes |
| `TotalSupply(AssetKey)` | `persistent()` | `i128` | 0 | `cross_asset_deposit`, `cross_asset_withdraw` | Yes |
| `TotalDebt(AssetKey)` | `persistent()` | `i128` | 0 | `cross_asset_borrow`, `cross_asset_repay` | Yes |

## TTL Policy

The cross‑asset module does not currently implement explicit TTL extension helpers; all persistent entries follow normal Soroban storage lifetime and rent renewal.

## Upgrade and Migration Notes

- **Append‑only**: New storage key variants must be added to the end of `CrossAssetDataKey`.
- **Structural stability**: The `AssetConfig` struct must preserve field ordering and types across upgrades.
- **Default values**: Absent numeric keys (`UserSupply`, `UserDebt`, `TotalSupply`, `TotalDebt`) are treated as 0.
