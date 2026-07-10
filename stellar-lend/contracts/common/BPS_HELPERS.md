# BPS Helpers — Rounding Contract

`scale_bps` and `unscale_bps` in `stellar-lend/contracts/common/src/lib.rs` are
the universal basis-point arithmetic helpers used across every interest, fee, and
rate computation in the protocol.

## Definitions

```rust
pub const BPS_DENOM: i128 = 10_000;  // 100% = 10_000 bps

/// scale_bps(v, r) = (v × r) / 10_000
pub fn scale_bps(value: i128, rate_bps: i128) -> Option<i128>;

/// unscale_bps(v, r) = (v × 10_000) / r   (inverse of scale_bps)
pub fn unscale_bps(value: i128, rate_bps: i128) -> Option<i128>;
```

## Rounding Behaviour

Both functions use **integer truncation** (Rust's integer division). There is no
round-half-up; the fractional part is always discarded.

> **Worked example:**
> `scale_bps(1, 1)` = `(1 × 1) / 10_000` = `0`
> (1 × 1 bps = 0.01%, truncated to zero)

## Round-Trip Guarantee

```
unscale_bps(scale_bps(v, r), r) ∈ {v - 1, v, v + 1}
```

The round-trip loss is **at most one unit** in either direction because:

1. `scale_bps(v, r)` = ⌊v·r / 10000⌋ — truncation loss < 1 unit of the result
2. `unscale_bps(s, r)` = ⌊s·10000 / r⌋ — after multiplying back, the mid-rounding
   never accumulates more than 1 unit of drift

> **Counter-example (why it's ≤1, not 0):**
> - `scale_bps(1, 1)` = `0` (1 bps of 1 = 0 after truncation)
> - `unscale_bps(0, 1)` = `0` ≠ `1`
> - Loss = 1 unit ✓

## Overflow Safety

Both functions return `None` on overflow rather than wrapping (via `checked_mul`
and `checked_div`). `unscale_bps` additionally returns `None` when `rate_bps == 0`
to prevent division by zero.

## Edge Cases Covered by Tests

| Input | Behaviour |
|-------|-----------|
| `v = 0` | Returns `Some(0)` for any non-zero rate |
| `r = 0` (unscale) | Returns `None` |
| `r = BPS_DENOM` (100%) | Identity — returns `v` |
| `r = 1` (1 bps) | Maximum truncation risk |
| `v = i128::MAX, r > 0` | Returns `None` (overflow) |
| `v = i128::MIN` | Returns `None` (overflow) |
| Negative values | Fully symmetric with positive |
