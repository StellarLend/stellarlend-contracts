# Collateral factor, oracle prices, and max borrow

This document summarizes how **loan-to-value (collateral factor)** and **oracle prices** interact with **maximum borrow** in the hello-world contract. Executable checks live in `src/tests/collateral_factor_max_borrow_spec.rs`.

## Single-asset borrow (`borrow_asset`)

- **Inputs**: posted collateral (global balance), optional borrow asset address, per-asset `AssetParams.collateral_factor`, and protocol `min_collateral_ratio` (default 150%, i.e. 15_000 bps).

- **Max borrow (no prior debt, no interest)**:

  ```text
  max_borrow = floor(C * cf / 10_000 * 10_000 / min_collateral_ratio)
  ```

  where `C` is collateral amount, `cf` is collateral factor in basis points.

- **Important**: The borrow asset `Option<Address>` selects which `AssetParams` row applies. To exercise a non-default factor, call `borrow_asset` with `Some(asset)` after configuring that asset; native collateral (`deposit_collateral` with `None`) can still back the loan.

## Cross-asset registry (`initialize_ca` / `cross_asset_*`)

- **Prices**: Stored per asset in `AssetConfig.price` (USD, 7 decimals). Admin updates via `update_asset_price`. Staleness rules apply when aggregating positions (see `cross_asset`).

- **Health**: Position summary weights **collateral value × liquidation threshold** (not `collateral_factor`) against debt value. The invariant `collateral_factor <= liquidation_threshold` is enforced when configs are updated.

- **Max borrow (intuition)**: A borrow is allowed if the post-borrow **health factor** stays at or above **1.0** (`10_000` scaled). Oracle prices scale both collateral and debt values when the same price applies to both legs of a single-asset loop.

## Trust boundaries (short)

| Actor        | Capability |
|-------------|------------|
| Admin       | Initialize assets, set risk/cross-asset params, push oracle prices (trusted in this reference design). |
| User        | Authorize own borrow/repay; cannot change others’ positions. |
| Oracle feed | Trusted only if the admin (or future signed feed) is trusted; stale prices are rejected for affected positions. |

## Reentrancy & token flows

- `borrow_asset` uses a reentrancy guard before mutating position state.
- Under `cfg(test)`, token transfers for `Some(asset)` borrows are skipped so tests do not require live token contracts.
- Production builds perform `transfer` from the lending contract to the user after balance checks.

See module-level comments in `src/borrow.rs` and `src/cross_asset.rs` for implementation detail.
