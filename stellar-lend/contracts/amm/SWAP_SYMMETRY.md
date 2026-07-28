# AMM Swap Symmetry

Fixes issue #1111.

## Overview

The AMM contract exposes two swap directions on the same constant-product pool:

| Function | Direction | Reserve in | Reserve out |
|----------|-----------|-----------|-------------|
| `swap_a_for_b` | A → B | `reserve_a` | `reserve_b` |
| `swap_b_for_a` | B → A | `reserve_b` | `reserve_a` |

Both paths use the same Uniswap-v2 fee formula and assert the same
k-monotonicity invariant, so neither direction provides an arbitrage advantage
over the other.

## Fee Model

```
amount_in_with_fee = amount_in × (10_000 − fee_bps)

amount_out = floor(
    amount_in_with_fee × reserve_out
  ─────────────────────────────────────────────────────
  reserve_in × 10_000 + amount_in_with_fee
)
```

`fee_bps` is specified by the caller (e.g. 30 = 0.30 %).  The fee is
collected implicitly — it stays in the pool and increases k slightly on
every swap.

`amount_out` uses **floor** division so the pool never pays out more than
the invariant permits.

## k-Monotonicity Invariant

After every swap:

```
k_after = reserve_a_new × reserve_b_new  ≥  k_before = reserve_a × reserve_b
```

This is enforced by `assert_k_monotonic(..., true)` immediately before
writing the new reserves to storage.  The assertion panics if k would
decrease, making an exploitative swap impossible.

## Worked Example

Initial pool: `reserve_a = 10 000`, `reserve_b = 10 000`, `k = 100 000 000`.

**Swap 1 000 B for A, fee_bps = 30:**

```
amount_in_with_fee = 1 000 × (10 000 − 30) = 1 000 × 9 970 = 9 970 000

numerator   = 9 970 000 × 10 000 = 99 700 000 000
denominator = 10 000 × 10 000 + 9 970 000 = 109 970 000

amount_out  = floor(99 700 000 000 / 109 970 000) = 906   (rounds down)
```

New reserves: `reserve_a = 9 094`, `reserve_b = 11 000`.

```
k_after = 9 094 × 11 000 = 100 034 000  ≥  100 000 000  ✓
```

## Round-Trip Property

Because every swap charges a fee:

```
swap_a_for_b(X)  →  Y   (Y < X if reserves are balanced)
swap_b_for_a(Y)  →  Z   (Z ≤ X)
```

A trader who performs a full round-trip always ends with **at most** what
they started with — confirmed by `test_round_trip_trader_does_not_profit`.

## Symmetry Property

With a perfectly balanced pool (`reserve_a == reserve_b`) swapping `X` of
A gives the same output as swapping `X` of B — confirmed by
`test_symmetric_output_equal_reserves`.
