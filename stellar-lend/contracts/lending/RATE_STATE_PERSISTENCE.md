# Rate-Model Smoothing-State Persistence

`update_and_get_rate(env, target_rate, params)` in
[`src/rate_model.rs`](src/rate_model.rs) smooths the borrow rate toward a target
and **persists** the result so that subsequent invocations — in *separate ledger
calls* — continue smoothing from the prior on-chain rate instead of restarting
from a default.

## Persisted state

Two keys live in instance storage under the `RateModelKey` enum:

| Key                     | Meaning                                              |
| ----------------------- | ---------------------------------------------------- |
| `RateModelKey::LastRate`       | The last clamped, smoothed rate (bps).        |
| `RateModelKey::LastRateLedger` | The ledger sequence of that last update.      |

## Lifecycle

1. **First call (no stored state).** `LastRateLedger` is absent (`0`), so the
   function seeds `last_rate = target_rate` and uses `elapsed = 0`. With zero
   elapsed ledgers `compute_smoothed_rate` returns the (hysteresis-adjusted)
   target, which is then floor/ceiling-clamped and stored. Net effect: the first
   call **initialises to the target**.

2. **Subsequent calls (state reloaded).** `LastRateLedger != 0`, so the prior
   `LastRate` is reloaded and `elapsed = current_ledger - last_ledger`. The
   smoothed step is bounded by `max_rate_change_per_ledger_bps * elapsed`, so the
   new rate moves from the **persisted** prior rate toward the new target — never
   jumping straight to it (unless smoothing is disabled).

3. **Smoothing disabled.** When `max_rate_change_per_ledger_bps == i128::MAX`,
   `compute_smoothed_rate` returns the target verbatim every call. The persisted
   value tracks the latest target and does not distort the following call.

## Convergence

Across repeated calls toward a fixed target the persisted rate moves one bounded
step per ledger and converges **monotonically without overshoot** — matching the
pure-function smoothing behaviour proven in `rate_smoothing_test`.

## Tests

See [`src/rate_persistence_test.rs`](src/rate_persistence_test.rs):

- `first_call_with_no_state_returns_target_and_persists_it`
- `second_call_smooths_from_persisted_prior_rate_not_a_fresh_default`
- `repeated_calls_converge_toward_fixed_target_without_overshoot`
- `smoothing_disabled_returns_target_verbatim_each_call`
- `oscillating_targets_track_persisted_state_across_ledgers`
