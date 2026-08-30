# Rate Smoothing State View

## Rationale

The lending rate model persists the effective smoothed borrow rate and the
ledger where it was last updated. Liquidation bots and indexers also need the
last target rate that produced the stored smoothing state so they can explain
rate movement without replaying utilization and rate parameters off-chain.

`get_rate_smoothing_state()` exposes the stored state directly:

```text
RateSmoothingState {
  schema_version: 1,
  current_rate_bps,
  last_target_rate_bps,
  last_update_ledger,
}
```

The view is observability-only. It does not call the borrow-rate calculator, it
does not update the per-ledger cache, and it does not recompute utilization.

## Worked Example

Assume these rate parameters:

- `max_rate_change_per_ledger_bps = 50`
- `hysteresis_bps = 0`
- floor and ceiling leave the example rates unclamped

At ledger `100`, `update_and_get_rate(env, 1_700, params)` initializes the
smoothing state. Because this is the first update, the smoothed rate is the
target:

```text
current_rate_bps = 1_700
last_target_rate_bps = 1_700
last_update_ledger = 100
```

At ledger `101`, utilization pushes the target to `2_700`. Smoothing allows a
single-ledger move of only `50 bps`, so the persisted effective rate becomes
`1_750` while the target is recorded as `2_700`:

```text
current_rate_bps = 1_750
last_target_rate_bps = 2_700
last_update_ledger = 101
```

`get_rate_smoothing_state()` returns those persisted values exactly. It does
not derive a fresh target from current deposits/debt and therefore cannot drift
from the state written by `update_and_get_rate`.

## Edge Cases

- **Uninitialized state:** returns `schema_version = 1` and zero for all rate
  and ledger fields.
- **Same-ledger reads:** repeated calls return the same values and do not
  mutate storage.
- **Clamped updates:** `current_rate_bps` is the final clamped smoothed rate
  persisted under `RateModelKey::LastRate`; `last_target_rate_bps` remains the
  target argument passed to `update_and_get_rate`.
- **Indexer compatibility:** the response is versioned. Future breaking schema
  changes should introduce a new version or a new view rather than changing the
  meaning of the version `1` fields.
