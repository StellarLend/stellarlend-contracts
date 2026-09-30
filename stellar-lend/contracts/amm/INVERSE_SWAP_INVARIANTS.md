# AMM Inverse Swap Invariants

This document describes the formal properties tested in `src/inverse_swap_proptest.rs`
for the `inverse_swap_in(ra, rb, amount_out, fee_bps)` function.

## Function Contract

`inverse_swap_in(ra, rb, amount_out, fee_bps)` returns the **minimum** amount of
asset A required to receive `amount_out` units of asset B from the pool, satisfying
the constant-product invariant `k = ra * rb`.

**Formula** (ceiling division to prevent under-payment by rounding):

```
amount_in_min = ceil(ra * amount_out / (rb - amount_out))
```

The `fee_bps` parameter mirrors the forward swap signature but the k-only formula
is fee-independent — it enforces k-monotonicity, not the fee-discount curve.

## Invariants

### INV-1: Round-Trip Consistency

For any valid `(ra, rb, amount_out, fee_bps)` with `0 < amount_out < rb`:

```
forward_swap_out(ra, rb, inverse_swap_in(ra, rb, amount_out, fee_bps), fee_bps) >= amount_out
```

The forward swap with the minimum computed input produces at least the requested
output. Any overshoot is bounded by a single unit of ceiling-rounding slack.

### INV-2: Monotonicity in amount_out

For any valid inputs with `out1 <= out2 < rb`:

```
inverse_swap_in(ra, rb, out1, fee_bps) <= inverse_swap_in(ra, rb, out2, fee_bps)
```

A larger desired output always requires at least as much input. This is essential
for correct price discovery and arbitrage-free pool operation.

### INV-3: Drain Rejection

`inverse_swap_in` panics (rejects) when `amount_out >= rb`. Draining the entire
reserve B is impossible by construction:

- `amount_out == rb` → denominator `rb - amount_out = 0` → division by zero → panic
- `amount_out > rb` → negative denominator → panic

### INV-4: Positive Result

For all valid inputs (`amount_out >= 1`, `amount_out < rb`, reserves > 0):

```
inverse_swap_in(ra, rb, amount_out, fee_bps) >= 1
```

A zero result would incorrectly imply that no input is needed to obtain a non-zero
output, violating the k-invariant.

### INV-5: k-Invariant Satisfaction After Repayment

After a flash swap debits `amount_out` from reserve B, repaying `inverse_swap_in(...)`
units of asset A keeps k non-decreasing:

```
(ra + amount_in_min) * (rb - amount_out) >= ra * rb
```

This is the verify-k check enforced by `repay_flash_swap`.

## Edge Cases Covered

| Case | Description |
|------|-------------|
| `amount_out = 1` | Smallest valid output — should require positive but minimal input |
| `amount_out = rb - 1` | Largest valid output — requires `ra * (rb - 1)` input (full pool depth) |
| `fee_bps = 0` | No fee discount — maximum output from forward swap |
| `fee_bps = 9_999` | Maximum fee — inverse is fee-independent, k-invariant still satisfied |
| `ra = rb = 1` | Minimal reserves — integer division always produces 0 or 1 |
| `amount_out >= rb` | Drain attempt — must panic |

## Running the Tests

```bash
cargo test -p stellarlend-amm
```

To run only the inverse swap property tests:

```bash
cargo test -p stellarlend-amm inverse_swap_proptest
```
