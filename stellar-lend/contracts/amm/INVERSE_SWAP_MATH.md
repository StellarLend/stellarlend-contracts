# AMM Inverse-Swap Math (`inverse_swap_in`)

## Overview

This document derives `inverse_swap_in(ra, rb, amount_out, _fee_bps)` from the
constant-product invariant, states its rounding direction and proves why that
direction protects the pool, and pins the derivation to the implementation with
a code-verified worked example.

`inverse_swap_in` answers one question:

> Given reserves `(ra, rb)` and a desired output `amount_out` of token B, what is
> the **minimum** input `amount_in` of token A that keeps the constant product
> `k = ra · rb` non-decreasing?

It is the algebraic inverse of the **verify-k** condition used by the flash-swap
repayment path (`repay_flash_swap`), *not* a fee-inclusive forward-swap quote.
The fee-inclusive forward quote (the `amount_out` you receive for a given
`amount_in`) lives in [AMM_MATH.md](./AMM_MATH.md#swap-formula); the flash-swap
call sequence that consumes this bound lives in
[FLASH_SWAP_PROTOCOL.md](./FLASH_SWAP_PROTOCOL.md#minimum-repayment-formula).

> **Scope note.** `inverse_swap_in` is a `#[cfg(test)]`, `pub(crate)` helper. It
> exists so tests and the protocol docs can compute the exact minimum repayment;
> it is not part of the deployed contract surface. The production verify-k check
> enforces the same inequality directly on reserves.

---

## Derivation from `x · y = k`

A flash swap optimistically debits `amount_out` of token B, then requires the
receiver to return enough token A so the invariant does not shrink. The post-repay
reserves are:

```
(ra + amount_in,  rb − amount_out)
```

The verify-k condition requires the product to be at least its prior value:

```
(ra + amount_in) · (rb − amount_out)  ≥  ra · rb
```

Solving for `amount_in` (valid because `rb − amount_out > 0`):

```
ra + amount_in  ≥  ra · rb / (rb − amount_out)

amount_in  ≥  ra · rb / (rb − amount_out)  −  ra

amount_in  ≥  ra · [ rb − (rb − amount_out) ] / (rb − amount_out)

amount_in  ≥  ra · amount_out / (rb − amount_out)
```

So the real-valued minimum is:

```
amount_in_min(ℝ)  =  ra · amount_out / (rb − amount_out)
```

---

## Rounding direction: round **up** (ceil)

All reserves and amounts are `i128`. The exact ratio above is rarely an integer,
so the implementation rounds **up** using integer arithmetic:

```
amount_in_min  =  ⌈ ra · amount_out / (rb − amount_out) ⌉
              =  ( ra · amount_out  +  (rb − amount_out) − 1 )  /  (rb − amount_out)
```

which is exactly the code:

```rust
let rb_minus_out = rb - amount_out;
let numerator    = ra * amount_out;
(numerator + rb_minus_out - 1) / rb_minus_out   // ceil(numerator / rb_minus_out)
```

### Why up, and why it protects the pool

Rounding **down** would return a value strictly below the real minimum whenever
the ratio is fractional, so `(ra + amount_in) · (rb − amount_out)` would land
just **under** `ra · rb` — a k-decrease the verify-k check must reject, i.e. an
under-payment that drains value from LPs.

Rounding **up** guarantees `amount_in ≥ amount_in_min(ℝ)`, hence

```
(ra + amount_in) · (rb − amount_out)  ≥  ra · rb
```

always holds. Any excess above the real minimum (the ceil "overshoot") simply
grows `k`, which accrues to liquidity providers as larger reserves. The pool is
therefore never under-paid, and the returned value is the **smallest** integer
with that property (see the minimality check in the doc-example test).

---

## Role of `fee_bps`: none, by design

The fourth argument is named `_fee_bps` (leading underscore) and is **unused**.
The minimum repayment is **fee-independent**: the verify-k inequality contains no
fee term, so the bound that preserves `k` does not move with `fee_bps`.

The parameter is retained only so the helper's signature mirrors the forward
swap `swap_a_for_b(amount_in, fee_bps)`, which *callers* commonly read the fee
from. This matches the protocol note in
[FLASH_SWAP_PROTOCOL.md §Fee Handling](./FLASH_SWAP_PROTOCOL.md#fee-handling):
flash swaps do not currently charge the per-side fee accumulator, and `fee_bps`
is reserved for a future explicit-fee extension.

The doc-example test asserts this directly: `inverse_swap_in(1000, 1000, 100, 0)`
and `inverse_swap_in(1000, 1000, 100, 9999)` return the **same** value.

---

## Worked examples (code-verified)

Every row below is asserted against `inverse_swap_in` in
[`src/inverse_swap_doc_example_test.rs`](./src/inverse_swap_doc_example_test.rs),
so this table cannot drift from the implementation. Each row also lists the
verify-k product at the returned amount and at `amount − 1`, demonstrating
minimality.

| # | Case | `ra` | `rb` | `amount_out` | `fee_bps` | `amount_in` | k preserved at `amount_in` | k violated at `amount_in − 1` |
|---|------|-----:|-----:|-------------:|----------:|------------:|---|---|
| 1 | Canonical (30 bps) | 1000 | 1000 | 100 | 30 | **112** | 1112·900 = 1 000 800 ≥ 1 000 000 | 1111·900 = 999 900 < 1 000 000 |
| 2 | Zero fee | 1000 | 1000 | 100 | 0 | **112** | identical to #1 | identical to #1 |
| 3 | Max fee (9999) | 1000 | 1000 | 100 | 9999 | **112** | identical to #1 | identical to #1 |
| 4 | Tiny reserves | 2 | 3 | 1 | 30 | **1** | 3·2 = 6 ≥ 6 | 2·2 = 4 < 6 |
| 5 | Output near reserve limit | 1000 | 1000 | 999 | 30 | **999 000** | 1 000 000·1 = 1 000 000 ≥ 1 000 000 | 999 999·1 = 999 999 < 1 000 000 |
| 6 | Exact division (no rounding) | 1000 | 1100 | 100 | 30 | **100** | 1100·1000 = 1 100 000 ≥ 1 100 000 | 1099·1000 = 1 099 000 < 1 100 000 |

Rows 2 and 3 share row 1's reserves and output but vary `fee_bps` across its full
valid span `[0, 9999]`, yielding the identical result — the worked-example proof
that the bound is fee-independent.

### Canonical example, step by step (row 1)

```
amount_in_min = ⌈ ra · amount_out / (rb − amount_out) ⌉
             = ⌈ 1000 · 100 / (1000 − 100) ⌉
             = ⌈ 100 000 / 900 ⌉
             = ⌈ 111.11… ⌉
             = 112

Ceiling via integer arithmetic:
  (100 000 + 900 − 1) / 900 = 100 899 / 900 = 112   ✓
```

Verify-k at the returned amount:

```
(1000 + 112) · (1000 − 100) = 1112 · 900 = 1 000 800  ≥  1000 · 1000 = 1 000 000   ✓
```

One unit lower would break the invariant (so 112 is minimal):

```
(1000 + 111) · (1000 − 100) = 1111 · 900 =   999 900  <   1 000 000   ✗
```

---

## Cross-references

| Document | Relationship |
|---|---|
| [AMM_MATH.md](./AMM_MATH.md) | Forward (fee-inclusive) swap formula — the complement to this inverse bound |
| [FLASH_SWAP_PROTOCOL.md](./FLASH_SWAP_PROTOCOL.md#minimum-repayment-formula) | Protocol context: the verify-k repayment step that consumes `amount_in_min` |
| [SWAP_BOUND_INVARIANTS.md](./SWAP_BOUND_INVARIANTS.md) | k-monotonicity invariants proven by property tests |
| [`src/inverse_swap_doc_example_test.rs`](./src/inverse_swap_doc_example_test.rs) | Code-verified worked examples that keep this document truthful |
