# AMM Swap Output-Bound Invariants

Property-based invariants proven by `swap_bounds_proptest.rs` (issue #1132).

## Formula

Uniswap-v2 constant-product swap (A → B):

```
amount_in_adj = amount_in × (10_000 − fee_bps)
amount_out    = ⌊ (amount_in_adj × reserve_b)
                  / (reserve_a × 10_000 + amount_in_adj) ⌋
```

All divisions use integer floor truncation.

## Invariants

### I-1 — Output bound

```
0 ≤ amount_out < reserve_b
```

A swap can never drain the full output reserve, and never yields negative output.

**Worked example** (`ra=1 000`, `rb=1 000`, `amt=500`, `fee=30`):
```
adj  = 500 × 9 970 = 4 985 000
out  = (4 985 000 × 1 000) / (1 000 × 10 000 + 4 985 000)
     = 4 985 000 000 / 14 985 000
     = 332   → 0 ≤ 332 < 1 000 ✓
```

### I-2 — Fee monotonicity

For fixed `(reserve_a, reserve_b, amount_in)`:

```
fee_high > fee_low  ⟹  swap_out(fee_high) ≤ swap_out(fee_low)
```

A higher fee always yields equal or less output. Rounding can make consecutive
fee values produce the same integer result, so the bound is `≤` not `<`.

### I-3 — No round-trip arbitrage

Starting with `x` of asset A, after A→B (at the post-leg-1 pool state) then B→A:

```
amount_back ≤ x
```

Integer floor truncation on each leg ensures the protocol always takes a spread;
the trader can never extract more than they put in.

### I-4 — k-monotonicity

```
k_after = (reserve_a + amount_in) × (reserve_b − amount_out)
        ≥ reserve_a × reserve_b  = k_before
```

The fee creates a spread: the denominator grows faster than the numerator shrinks,
so the product `k` never decreases.

## Edge Cases Covered

| Scenario | Expected behaviour |
|----------|--------------------|
| `fee_bps = 0` | Maximum output; I-1 and I-4 still hold |
| `fee_bps = 9 999` | Output rounds to 0; no drain |
| `reserve_a = reserve_b = 1` | Integer division yields 0 output |
| `amount_in >> reserve_a` | Output approaches `reserve_b − 1` but never reaches it |
| Round-trip `amt=1`, tiny reserves | `amount_back = 0 ≤ 1` ✓ |
