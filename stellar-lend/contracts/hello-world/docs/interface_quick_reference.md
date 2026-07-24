# Interface Quick Reference

Read-only views exposed by the `hello-world` contract for integrators,
liquidation bots, and monitoring tools. Each entry documents the formula
and a worked example so a caller can sanity-check the value they get back
without reading the implementation.

## `get_pool_twap_price(asset, window_secs) -> Option<u128>`

Source: [`oracle.rs`](../src/oracle.rs) (`get_pool_twap_price`), backed by
[`amm_twap.rs`](../src/amm_twap.rs) (`get_twap`, `has_window_coverage`).
Contract entrypoint: [`lib.rs`](../src/lib.rs).

### What it's for

The oracle module (`oracle.rs`) falls back to an AMM time-weighted average
price (`amm_twap::get_twap`) whenever the primary price feed is stale or
missing. Until this view existed, that fallback value was only visible
*indirectly* — as whatever `get_price` happened to return — with no way to
inspect it on its own. `get_pool_twap_price` exposes that same fallback
value directly, so a monitor or liquidation bot can check "is the fallback
price usable right now, and what is it?" without waiting for the primary
feed to actually go stale first.

### Formula

For a pool tracking `asset` against its paired token, with cumulative
price accumulator `P` (updated on every swap/liquidity event) and a window
`[T − window_secs, T]`:

```text
twap = (P(T) − P(T − window_secs)) / window_secs
```

`P(T)` is the accumulator extrapolated to the current ledger timestamp;
`P(T − window_secs)` is read from the nearest persisted snapshot at or
before that point (or the earliest available history, if the window
extends further back than any snapshot). The result is scaled by
`amm_twap::PRICE_SCALE` (`1e18`) — divide by `1e18` to get the
human-readable price of one unit of `asset` in terms of its paired token.

This is **the same calculation** `oracle::get_price`'s fallback path uses
internally (`try_twap_fallback` calls the identical `amm_twap::get_twap`
function) — `get_pool_twap_price` does not reimplement or approximate it.

### Sentinel behavior (`None`)

Unlike the internal fallback path (which is allowed to hard-abort the
transaction when data isn't available, since that's an acceptable
fail-safe deep inside a settlement flow), this view **never panics**. It
returns `None` whenever calling `get_twap` would otherwise be unsafe:

| Condition | Result |
|---|---|
| No pool state recorded yet for `asset` | `None` |
| Pool exists, but zero/insufficient elapsed history for the window | `None` |
| `window_secs` below `amm_twap::MIN_WINDOW_SECS` (25s) | `None` |
| Sufficient snapshot history covers the window | `Some(twap_value)` |

Treat `None` as "not usable for this window right now" — not as a
transient error worth retrying immediately.

### Worked example

A pool has been swapping for a while. At ledger time `T = 10_000`:

- A snapshot at `t = 9_850` recorded `price0_cumulative = 100_000 × 1e18`.
- The live accumulator, extrapolated to `T = 10_000`, reads
  `price0_cumulative = 130_000 × 1e18`.

Calling `get_pool_twap_price(asset, 150)` (a 150-second window, matching
`oracle::TWAP_FALLBACK_WINDOW_SECS`):

```text
window_secs   = 150          (T - 150 = 9_850, exactly matches the snapshot)
delta         = 130_000e18 − 100_000e18 = 30_000e18
twap          = 30_000e18 / 150 = 200e18
```

Result: `Some(200_000_000_000_000_000_000)` → divide by `1e18` → **200**
units of the paired token per unit of `asset`.

If instead no snapshot existed within the requested window (e.g. the pool
was created less than 150 seconds ago), the call returns `None` rather
than panicking or returning a misleadingly-extrapolated number.

### Notes for integrators

- This is a pure read — it never writes to contract storage, unlike
  `get_price`, which may cache the price it resolves.
- The returned scale (`1e18`) intentionally matches the AMM's own
  accumulator scale, **not** the protocol's 6-decimal internal price
  format used elsewhere (e.g. `get_price`'s `i128` return). Rescale
  yourself if you need to compare directly against `get_price`'s output:
  divide by `(amm_twap::PRICE_SCALE / 1_000_000)` to match.
