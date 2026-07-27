# Asset Price Age Tests & Staleness Documentation

## Overview

`get_asset_price_age` in `stellar-lend/contracts/hello-world/src/cross_asset.rs` reports how old a per-asset oracle price is in seconds relative to the current ledger timestamp.

Tracking price age is critical for decentralized lending protocols to prevent stale oracle prices from causing unfair liquidations or over-leveraged borrowing during oracle outages or market volatility.

---

## Rationale & Design Philosophy

1. **Price Staleness Protection**: Protocol position calculations and health checks rely on up-to-date oracle prices. `get_asset_price_age` enables off-chain liquidators, keeper bots, and on-chain risk guards to inspect price staleness per asset.
2. **Safe Error Handling (No Panic)**: Requesting the price age of an unregistered or unknown asset returns `Err(CrossAssetError::AssetNotFound)` rather than causing a WASM contract panic.
3. **Monotonic & Saturating Time Calculation**: Ledger time calculation uses `now.saturating_sub(last_update_ts)` to guarantee that time advancing yields monotonic age growth while protecting against underflow in edge cases (such as minor clock adjustments in test environments).

---

## Worked Example

### 1. Registering an Asset

When an asset is registered via `initialize_asset`, its `last_update_ts` is initialized to the current ledger timestamp:

```rust
let env = Env::default();
env.ledger().set_timestamp(1_000);

initialize_asset(&env, None, config).unwrap();
// last_update_ts is set to 1000
```

### 2. Price Age Growth Over Time

As time advances without price updates, `get_asset_price_age` increases linearly:

```rust
env.ledger().set_timestamp(2_500);

let age = get_asset_price_age(&env, None).unwrap();
// age = 2_500 - 1_000 = 1_500 seconds
```

### 3. Resetting Age via Price Update

Updating the asset price via `update_asset_price` records the latest ledger timestamp and resets the price age to 0:

```rust
env.ledger().set_timestamp(3_000);
update_asset_price(&env, None, 1_100_000).unwrap();

let age = get_asset_price_age(&env, None).unwrap();
// age = 3_000 - 3_000 = 0 seconds
```

---

## Edge-Case Handling

| Scenario | Behavior / Result | Safety Guarantee |
|----------|------------------|------------------|
| **Unknown Asset** | Returns `Err(CrossAssetError::AssetNotFound)` | No WASM panic; caller receives typed error. |
| **Fresh Update** | Resets `last_update_ts = env.ledger().timestamp()` | Age drops to 0 at the update timestamp. |
| **Monotonic Growth** | Age grows linearly with `env.ledger().timestamp()` | Guarantees consistent staleness evaluation. |
| **Underflow Guard** | Uses `now.saturating_sub(last_update_ts)` | Prevents panic even if `now < last_update_ts`. |

---

## Running Test Coverage

To run the dedicated asset price age test suite:

```bash
cargo test -p hello-world asset_price_age
```

To run all contract tests across the workspace:

```bash
cargo test --workspace
```
