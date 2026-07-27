# Cross-Asset Config Bounds

## Overview

`update_asset_config` lets a protocol admin change the per-asset risk parameters
(collateral factor, liquidation threshold, supply/borrow caps, flags, oracle
decimals) stored in `AssetConfig`. Without proper guards an admin could silently
create unsafe configurations — for example setting the LTV above the liquidation
threshold so new positions are born already eligible for liquidation, or setting
`price_decimals = 0` so every oracle price is off by a factor of up to 10^18.

This document describes the access-control and validation rules enforced since
the hardening in `closes #<issue>`.

---

## Access control

Every call to `update_asset_config` requires the `caller` argument to match the
stored admin address (`CrossAssetAdminKey::Admin`). Any other address — including
the zero address and the contract's own address — is rejected with
`CrossAssetError::Unauthorized` (variant 10) **before** any state is read or
written.

```
caller == stored_admin → proceed
caller != stored_admin → Err(Unauthorized)
no admin stored       → Err(Unauthorized)
```

The admin is written during protocol initialisation via `set_admin`. Tests that
call `update_asset_config` should either call `set_admin` first, or use
`env.mock_all_auths()` for scenarios where authentication is not the focus.

---

## Validation rules

All rules are evaluated against the **post-update** config (i.e. after merging
the supplied `Some(...)` fields into the stored config). This means:

- Lowering the threshold below the current factor is rejected the same way as
  raising the factor above the current threshold.
- Changing both fields simultaneously is evaluated on their combined result.

| Field | Rule | Error |
|-------|------|-------|
| `collateral_factor_bps` | Must be in `[0, 10_000]` | `InvalidCollateralFactor` (9) |
| `collateral_factor_bps` | Must be ≤ `liquidation_threshold` | `LtvExceedsThreshold` (11) |
| `price_decimals` | Must not be `0` | `ZeroDecimals` (12) |
| `price_decimals` | Must be ≤ `38` | `InvalidDecimals` (8) |

No other fields have explicit bounds today; `max_supply` and `max_borrow` of `0`
mean "unlimited" and all non-zero values are accepted.

---

## Rationale

### LTV ≤ liquidation_threshold

The liquidation threshold is the ratio at which an existing position becomes
eligible for liquidation. The LTV (collateral factor) is the ratio at which new
borrowing capacity is issued against deposited collateral.

If LTV were allowed to exceed the threshold:

```
deposited collateral = 100
borrow_capacity      = 100 × LTV/10_000 = 101    (e.g. LTV = 10_100)
liquidation point    = 100 × threshold/10_000 = 100   (threshold = 10_000)
```

A user who borrows right up to their borrow capacity (101) immediately has
debt > liquidation point (100) and is liquidatable the instant the borrow
is recorded. The protocol would be in an incoherent state from block one.

### LTV ≤ 100 % (10 000 bps)

A collateral factor above 100 % would grant more borrowing power than the
collateral's market value. Any price movement, however small, would make the
position insolvent before any liquidation could clear it.

### price_decimals ≠ 0

`price_decimals` is the exponent used to normalise oracle prices to the shared
18-decimal internal scale. A value of `0` means the raw oracle integer is
treated as having 18 decimal digits of precision, inflating every collateral
value and borrow capacity by up to 10^18. This would allow essentially
unlimited borrowing against any deposit.

### price_decimals ≤ 38

`i128` has at most 38–39 significant decimal digits. Any exponent above 38
overflows `10^(INTERNAL_DECIMALS - price_decimals)` during normalisation, so
the check at registration time already rejects values > 38, and `update_asset_config`
mirrors that guard.

---

## Worked example

Suppose an admin wants to tighten the config for the USDC market:

| Field | Before | After | Note |
|-------|--------|-------|------|
| `collateral_factor_bps` | 8 000 (80 %) | 7 500 (75 %) | Lower LTV |
| `liquidation_threshold` | 8 500 (85 %) | 8 000 (80 %) | Lower threshold |
| `price_decimals` | 6 | 6 | Unchanged |

Post-update invariant check: `7_500 ≤ 8_000` ✓ → accepted.

Now suppose the admin tries to tighten only the threshold without also
reducing the factor:

| Field | Before | After |
|-------|--------|-------|
| `collateral_factor_bps` | 8 000 | 8 000 (unchanged) |
| `liquidation_threshold` | 8 500 | 7 999 |

Post-update invariant check: `8_000 > 7_999` → **rejected** with
`LtvExceedsThreshold`. The admin must lower the factor at the same time (or
first) to keep the invariant.

---

## Edge cases

| Scenario | Outcome |
|----------|---------|
| Factor == threshold (e.g. both 8 000) | Accepted — positions created at exactly the liquidation boundary are immediately liquidatable once health < 1, but are not born underwater |
| All fields `None` | No-op; config unchanged; succeeds; **no event emitted** (nothing changed to audit) |
| Asset not registered | `AssetNotFound` before any auth or bounds check |
| `price_decimals = 1` (minimum non-zero) | Accepted |
| `price_decimals = 38` (maximum) | Accepted |
| `price_decimals = 39` | `InvalidDecimals` |

---

## Event

A `ConfigUpdatedEvent` is emitted on every **successful** update:

```
topics: ("crossAsst", "cfgUpd")
data:   ConfigUpdatedEvent {
    asset_key,
    collateral_factor_bps,
    liquidation_threshold,
    max_supply,
    max_borrow,
    can_collateralize,
    can_borrow,
}
```

All fields in the event reflect the **post-update** state. A failed call emits
no event.

---

## Test coverage

Tests live in `src/cross_asset_config_bounds_test.rs`. The suite covers:

| Test | What it checks |
|------|----------------|
| `test_update_rejects_non_admin` | Non-admin caller rejected; config unchanged |
| `test_update_rejects_when_no_admin_set` | No admin stored → Unauthorized |
| `test_update_rejects_ltv_above_threshold` | factor > threshold → LtvExceedsThreshold |
| `test_update_rejects_threshold_below_current_factor` | threshold drop below factor → LtvExceedsThreshold |
| `test_update_accepts_ltv_equal_to_threshold` | factor == threshold → Ok |
| `test_update_rejects_ltv_above_100_pct` | factor > 10 000 → InvalidCollateralFactor |
| `test_update_rejects_negative_factor` | factor < 0 → InvalidCollateralFactor |
| `test_update_rejects_zero_decimals` | price_decimals == 0 → ZeroDecimals |
| `test_update_rejects_decimals_above_38` | price_decimals == 39 → InvalidDecimals |
| `test_update_valid_persists_changes` | Full valid update → persisted |
| `test_update_all_none_is_noop` | All-None → no change |
| `test_update_emits_config_updated_event` | Successful update → 1 event |
| `test_failed_update_emits_no_event` | Failed update → 0 events |
| `test_update_preserves_other_assets` | Unrelated assets untouched |

Run with:

```bash
cargo test -p cross-asset-test cross_asset_config_bounds
# or for the hello-world mirror:
cargo test -p hello-world cross_asset_config_bounds
```
