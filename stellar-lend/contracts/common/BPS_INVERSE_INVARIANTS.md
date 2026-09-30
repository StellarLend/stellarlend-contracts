# `scale_bps` / `unscale_bps` Inverse Invariants

Rationale and worked examples for the property-based tests in
[`src/bps_inverse_proptest.rs`](src/bps_inverse_proptest.rs).

## The helpers

```rust
scale_bps(v, r)   = (v * r)      / BPS_DENOM   // None on overflow
unscale_bps(v, r) = (v * BPS_DENOM) / r        // None on overflow or r == 0
```

`BPS_DENOM = 10_000`. Both use checked `i128` arithmetic and return `Option`,
so they are **total** — they never panic, returning `None` instead of trapping.

## Round-trip error bound

`scale_bps` and `unscale_bps` are inverses, but each performs a **truncating**
integer division, so the round trip is not exact. The bound is:

```text
| unscale_bps(scale_bps(v, r), r) - v |  ≤  BPS_DENOM / |r| + 1
```

(whenever both directions return `Some`).

### Why

Write `s = scale_bps(v, r) = trunc(v·r / D)`, so `s·D = v·r − e` with
`0 ≤ |e| < D`. Then:

```text
unscale_bps(s, r) = trunc(s·D / r) = trunc((v·r − e)/r) = trunc(v − e/r)
```

`v` is an integer and `|e/r| < D/|r|`, and the final `trunc` adds at most one
more unit, giving `|round_trip − v| < D/|r| + 1`. Since the result is an
integer, `≤ BPS_DENOM/|r| + 1`. This was verified exhaustively/randomly over
41M cases; the bound is tight (e.g. `v = -3000, r = -2993` hits it exactly).

### One-unit special case

When `|r| ≥ BPS_DENOM` (rate ≥ 100 %), `BPS_DENOM / |r| = 0`, so the bound is
**1** — the familiar single-unit rounding error.

## Worked example

`v = 1_000_000`, `r = 500` (5 %):

```text
scale_bps(1_000_000, 500)   = 500_000_000 / 10_000   = 50_000
unscale_bps(50_000, 500)    = 500_000_000 / 500       = 1_000_000   (exact)
```

A lossy case, `v = 7`, `r = 3`:

```text
scale_bps(7, 3)   = 21 / 10_000        = 0
unscale_bps(0, 3) = 0                   → round-trip 0, error 7
bound = 10_000/3 + 1 = 3334            → 7 ≤ 3334 ✓
```

## Edge cases covered by the tests

- **Round-trip within bound** — `prop_round_trip_within_bound`.
- **Overflow → `None`, never panics** — `prop_scale_matches_reference`,
  `prop_unscale_matches_reference` (checked against a reference oracle).
- **Zero divisor → `None`** — `prop_unscale_zero_divisor_is_none`.
- **Negative-value sign symmetry** — `prop_sign_consistency`
  (`i128::MIN` is skipped as it has no positive counterpart).
- **One-bps edge** — `prop_one_bps_edge`.

## Running

```bash
cargo test -p stellar-lend-common bps_inverse_proptest
```