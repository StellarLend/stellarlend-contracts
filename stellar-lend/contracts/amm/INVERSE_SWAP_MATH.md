# Inverse Swap Math — `inverse_swap_in` Derivation

`inverse_swap_in` in `stellar-lend/contracts/amm/src/lib.rs` computes the
**minimum input** of asset A required to obtain a desired output of asset B
from the constant-product pool.

## Constant-Product Invariant

The AMM pool maintains the invariant:

```
x * y = k
```

where **x = reserve A** (`ra`), **y = reserve B** (`rb`), and **k** is constant
before any swap.

## Derivation

### Step 1 — State after removing `amount_out` of B

After the user withdraws `amount_out` of asset B, the new reserve of B is:

```
rb' = rb - amount_out
```

To satisfy the invariant, the new reserve of A must be:

```
k = ra * rb          (by definition of k)
ra' = k / rb'        (the only value satisfying ra' * rb' = k)
ra' = (ra * rb) / (rb - amount_out)
```

### Step 2 — Required input of A

```
amount_in = ra' - ra
          = (ra * rb) / (rb - amount_out) - ra
```

Multiply `ra` by `(rb - amount_out) / (rb - amount_out)` and combine:

```
amount_in = [ra * rb - ra * (rb - amount_out)] / (rb - amount_out)
          = [ra * rb - ra * rb + ra * amount_out] / (rb - amount_out)
          = ra * amount_out / (rb - amount_out)
```

**Final formula:**

```
amount_in = (ra * amount_out) / (rb - amount_out)
```

### Step 3 — Ceil Rounding (Pool-Favouring)

Integer division in Rust truncates toward zero, which would **under-pay** the
pool and decrease k. To guarantee k does not decrease, the implementation uses
**ceil division**:

```
amount_in = ⌈ ra * amount_out / (rb - amount_out) ⌉
          = (ra * amount_out + rb_minus_out - 1) / rb_minus_out
```

This rounds **up** to the nearest integer, ensuring the pool is never underpaid
and the invariant k never decreases.

## Role of `fee_bps`

The function signature includes `_fee_bps: i128` but the leading underscore
indicates it is **currently unused**. The parameter is reserved for future
dynamic-fee integration. When activated, the fee would scale the numerator:

```
amount_in_fee = amount_in * BPS_DENOM / (BPS_DENOM - fee_bps)
```

This would make the user pay slightly more to account for the protocol fee,
but is not yet wired in.

## Implementation Reference

```rust
/// Inverse swap: compute the minimum amount of asset A required to obtain
/// `amount_out` of asset B from the constant-product pool.
///
/// # Parameters
/// * `ra` — current reserve of asset A
/// * `rb` — current reserve of asset B
/// * `amount_out` — desired withdrawal of asset B
/// * `_fee_bps` — reserved for future dynamic fee (currently unused)
///
/// # Returns
/// Minimum `amount_in` of asset A, rounded up (ceil) to favour the pool.
///
/// # Panics
/// * `"amount_out >= rb"` — requested output exceeds available reserves.
/// * `"inverse_swap_in overflow"` — intermediate computation overflows i128.
#[cfg(test)]
pub(crate) fn inverse_swap_in(ra: i128, rb: i128, amount_out: i128, _fee_bps: i128) -> i128 {
    let rb_minus_out = rb.checked_sub(amount_out).expect("amount_out >= rb");
    let numerator = ra.checked_mul(amount_out).expect("inverse_swap_in overflow");
    (numerator + rb_minus_out - 1) / rb_minus_out
}
```

## Worked Example

| Variable | Value |
|----------|-------|
| Reserve A (`ra`) | 1,000,000 |
| Reserve B (`rb`) | 500,000 |
| Desired output B | 1,000   |

```
rb_minus_out = 500,000 - 1,000 = 499,000
numerator    = 1,000,000 * 1,000 = 1,000,000,000
amount_in    = ⌈1,000,000,000 / 499,000⌉ = ⌈2004.008…⌉ = 2005
```

**Verification:**
```
ra' = 1,000,000 + 2,005 = 1,002,005
rb' = 500,000 - 1,000   = 499,000
k'  = 1,002,005 * 499,000 = 500,000,495,000
k   = 1,000,000 * 500,000 = 500,000,000,000
k' ≥ k ✓   (pool invariant preserved)
```

## Related Documents

- [FLASH_SWAP_PROTOCOL.md](../FLASH_SWAP_PROTOCOL.md) — how `inverse_swap_in` is used in flash-loan repayment
- `swap_bounds_proptest` — fuzz tests verifying K monotonicity
- `inverse_swap_proptest` — property tests for the inverse function
