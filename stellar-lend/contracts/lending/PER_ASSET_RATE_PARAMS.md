# Per-asset rate-params overrides

Issue: [#1258](https://github.com/StellarLend/stellarlend-contracts/issues/1258)

## Motivation

A multi-asset lending market should not force every listed asset onto a single
interest-rate curve. Volatile long-tail collateral and deep stablecoin pools
need independent base rates, kink points, and jump slopes while still sharing
one protocol-wide default.

## Resolution order

`get_effective_rate_params(asset)` resolves in this order:

1. **Per-asset override** — persistent storage key
   `RateModelKey::AssetParams(asset)`, written by admin via
   `set_asset_rate_params`.
2. **Protocol-global curve** — instance storage key `DataKey::RateParams`.
3. **Hard-coded default** — `RateParams::default()` (see `RATE_MODEL.md`).

`compute_borrow_rate(utilization_bps, params)` stays a pure function. Callers
that know the asset first resolve params, then pass them in:

```text
params = get_effective_rate_params(asset)
rate   = compute_borrow_rate(utilization_bps, &params)
```

## Admin API

| Function | Auth | Effect |
|----------|------|--------|
| `set_asset_rate_params(asset, params)` | admin | validate + store override |
| `clear_asset_rate_params(asset)` | admin | delete override (fall back) |
| `get_effective_rate_params(asset)` | none | resolved params |
| `get_asset_rate_params_override(asset)` | none | raw override or `None` |

### Validation on write

Rejected with `LendingError::InvalidAmount` when any of:

- `rate_floor_bps > rate_ceiling_bps`
- `kink_utilization_bps` not in `0..=10_000`
- negative base rate, slopes, floor, ceiling, or hysteresis

## Worked two-asset example

Assume global defaults (`base=100`, `kink=8000`, `multiplier=2000`,
`jump=10000`, floor=50, ceiling=10000).

| Asset | Override? | Params used | Rate at 40% util |
|-------|-----------|-------------|------------------|
| USDC  | no | global default | `100 + 4000*2000/10000 = 900` bps |
| RARE  | yes: base=500, kink=5000, mult=3000, jump=30000 | override | `500 + 4000*3000/10000 = 1700` bps |

After `clear_asset_rate_params(RARE)`, RARE again prices at 900 bps under the
global curve — no residual storage key.

## Storage

| Key | Tier | Type |
|-----|------|------|
| `RateModelKey::AssetParams(Address)` | persistent | `RateParams` |
| `DataKey::RateParams` (unchanged) | instance | `RateParams` |

No existing key is renamed. Assets without an override produce **byte-identical**
rates to the pre-feature single-curve path.
