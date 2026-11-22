# `scale_bps` / `unscale_bps` — Rounding Contract

## Formula

```rust
scale_bps(v, r)   = (v * r)      / BPS_DENOM   // None on overflow
unscale_bps(v, r) = (v * BPS_DENOM) / r        // None on overflow or r == 0
```

Where `BPS_DENOM = 10_000` (one hundred percent expressed in basis points).

Both helpers return `Option<i128>`. They **never panic** — overflow and
division-by-zero produce `None` instead of a trap.

## Round-trip guarantee

When both directions succeed, the composition

```text
round_trip = unscale_bps(scale_bps(v, r), r)
```

recovers `v` within a bounded rounding loss. The bound depends on `|r|`:

| Rate range                | Bound on `|round_trip - v|` |
|---------------------------|-----------------------------|
| `|r| ≥ BPS_DENOM` (≥100%) | **≤ 1** (one unit)          |
| `|r| < BPS_DENOM` (<100%) | `≤ BPS_DENOM / |r| + 1`    |

### Why the bound is one when |r| ≥ BPS_DENOM

Write `s = scale_bps(v, r) = trunc(v · r / D)`.

Then `s · D = v · r − e` with `0 ≤ |e| < D`.

Unscaling:

```text
unscale_bps(s, r) = trunc(s · D / r)
                  = trunc((v · r − e) / r)
                  = trunc(v − e / r)
```

Since `|e / r| < D / |r| ≤ 1` when `|r| ≥ D`, the first truncation shifts `v`
by less than one unit toward zero, and the final `trunc` adds at most one more
unit. The combined error is therefore an integer with magnitude **≤ 1**.

### Tightness

The bound is tight. Example: `v = 5, r = 10001` (100.01 %):

```text
scale_bps(5, 10001)   = trunc(50005 / 10000) = 5
unscale_bps(5, 10001) = trunc(50000 / 10001) = 4
|4 − 5| = 1 ✓
```

## Worked example

### Exact round-trip

`v = 1_000_000`, `r = 500` (5 %):

```text
scale_bps(1_000_000, 500)   = 500_000_000 / 10_000   = 50_000
unscale_bps(50_000, 500)    = 500_000_000 / 500       = 1_000_000   (exact)
```

### Lossy round-trip (still ≤ 1)

`v = 7`, `r = 20000` (200 %):

```text
scale_bps(7, 20000)   = 140_000 / 10_000   = 14
unscale_bps(14, 20000) = 140_000 / 20_000   = 7           (exact)
```

`v = 5`, `r = 10001` (100.01 %):

```text
scale_bps(5, 10001)   = 50_005 / 10_000   = 5
unscale_bps(5, 10001) = 50_000 / 10_001   = 4
loss = 1 ✓
```

## Overflow contract

| Input                                  | `scale_bps` | `unscale_bps` |
|----------------------------------------|-------------|---------------|
| `value = i128::MAX, rate = 2`          | `None`      | —             |
| `value = i128::MAX, rate = 1`          | `Some(..)`  | `None`        |
| `value = any, rate = 0`                | `Some(0)`   | `None`        |

Both helpers use `checked_mul` and `checked_div`, so overflow never wraps.

## Edge cases covered by `bps_roundtrip_test.rs`

- Round-trip loss ≤ 1 for `|r| ≥ BPS_DENOM` (various values and signs)
- Zero value, zero rate (`unscale_bps` returns `None`)
- One-bps rate (`r = 1`, the finest granularity)
- `i128::MAX` / `i128::MIN` boundary inputs
- Negative values and negative rates
- Determinism (repeated calls produce identical results)

## Running

```bash
cargo test -p stellar-lend-common bps_roundtrip_test
```

## Related

- [`bps_inverse_proptest`](src/bps_inverse_proptest.rs) — property-based tests
  that verify the general bound `D / |r| + 1` over randomised inputs.
- [`BPS_INVERSE_INVARIANTS.md`](BPS_INVERSE_INVARIANTS.md) — derivation and
  worked examples for the property-based invariants.
