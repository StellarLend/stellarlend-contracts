# AMM Volume & Last-Price Observability View

## Rationale

The AMM tracks reserves and accrued fees, but exposes no cumulative trade
**volume** or **last execution price**. Off-chain indexers and analytics
dashboards need both: volume to rank pools and compute turnover, and a
last-price reference for charts and routing heuristics.

Re-deriving these from raw swap events is brittle (events can be missed,
re-orgs complicate aggregation). This feature adds two lightweight,
on-chain counters and a single read-only view, so a consumer can fetch the
current state in one call:

```rust
let stats = client.get_volume_stats();
```

Design constraints honoured:

- **No event spam.** Only persistent counters are updated; no events are
  emitted. The data is read on demand via `get_volume_stats`.
- **No effect on swap math.** Counter writes happen after the swap output
  and the price-impact guard have been computed; reserves, fees and the
  constant-product invariant are unchanged.
- **Overflow-safe.** Volume accumulation uses `i128::saturating_add`, so an
  extreme cumulative total pins at `i128::MAX` instead of panicking and
  bricking the pool.
- **Stable wire type.** `get_volume_stats` returns a `#[contracttype]`
  `VolumeStats` struct so decoding is stable across releases.

## The `VolumeStats` struct

| Field                    | Meaning                                                              |
|--------------------------|----------------------------------------------------------------------|
| `cumulative_volume_a_in` | Total asset-A input ever routed through `swap_a_for_b`.              |
| `cumulative_volume_b_in` | Total asset-B input ever routed through `swap_b_for_a`.              |
| `last_price_num`         | Numerator of the last swap's execution price (units of **B per A**). |
| `last_price_denom`       | Denominator of that price. `0` ⇒ no priced swap yet (undefined).     |

### Price convention

The pool's canonical spot price (used by the price-impact guard) is
`reserve_b / reserve_a` — **units of B per unit of A**. The last execution
price is expressed in the same convention, as an exact rational so no
precision is lost to integer division:

- `swap_a_for_b(amount_in)` → input is A, output is B, so
  `price = amount_out / amount_in` ⇒ `num = amount_out`, `denom = amount_in`.
- `swap_b_for_a(amount_in)` → input is B, output is A, so
  `price = amount_in / amount_out` ⇒ `num = amount_in`, `denom = amount_out`.

Both yield "B per A", so a consumer can compare prices across directions
directly:

```text
last_price (B per A) = last_price_num / last_price_denom
```

## Worked example

Start a pool with equal reserves and the default 30 bps fee:

```text
init_pool(a = 1_000_000, b = 1_000_000)
get_volume_stats() → { vol_a: 0, vol_b: 0, num: 0, denom: 0 }   // no swaps
```

Swap 10_000 of A for B. With a 30 bps fee on a balanced pool the output is
`amount_out = 9_871`:

```text
swap_a_for_b(10_000) → 9_871
get_volume_stats() → {
    cumulative_volume_a_in: 10_000,
    cumulative_volume_b_in: 0,
    last_price_num:   9_871,     // B out
    last_price_denom: 10_000,    // A in   → price ≈ 0.9871 B per A
}
```

Now swap 10_000 of B for A (output `amount_out = 10_068` against the shifted
reserves):

```text
swap_b_for_a(10_000) → 10_068
get_volume_stats() → {
    cumulative_volume_a_in: 10_000,
    cumulative_volume_b_in: 10_000,
    last_price_num:   10_000,    // B in
    last_price_denom: 10_068,    // A out  → price ≈ 0.9932 B per A
}
```

Both realized prices sit just below the balanced-pool spot of `1.0` B per A:
the 30 bps fee and floor-rounding mean a trader always receives slightly less
output than the frictionless rate, in either direction. The two prices are
directly comparable because both are expressed in the same B-per-A
convention.

> The exact integer outputs above come from the Uniswap-v2 formula
> `amount_out = (amount_in·(10_000−fee)·reserve_out) / (reserve_in·10_000 + amount_in·(10_000−fee))`
> and are reproduced by the tests in `src/volume_stats_test.rs`.

## Edge cases

- **No swaps yet.** All four fields are `0`. A `last_price_denom` of `0` is
  the sentinel for "undefined price" — consumers must not divide by it.
- **Dust swap with zero output.** A tiny input against a huge pool floors to
  `amount_out == 0`. Volume still records the input that flowed in, but the
  last price is **not** updated (it would otherwise store a `0` denominator
  for `swap_b_for_a`, or a meaningless `0/amount_in` for `swap_a_for_b`).
- **Saturation.** When a side's cumulative volume would exceed `i128::MAX`
  it saturates at `i128::MAX` and stays there; subsequent swaps still
  succeed and never panic. Saturation is per-side and independent.
- **Re-initialisation.** `init_pool` resets the volume counters and the
  last-price record to zero (consistent with how it resets fee accumulators).
- **Rejected swaps.** A swap rejected by the price-impact guard makes no
  state change at all, so the counters are left untouched.

## Testing

`src/volume_stats_test.rs` covers: zero/initial stats, a single swap on each
side, cumulative accumulation across many swaps, per-side independence, last
price after each direction, saturation at `i128::MAX` (per side), the
zero-output dust case, and reset on re-init.

```sh
cargo test -p stellarlend-amm volume_stats
```
