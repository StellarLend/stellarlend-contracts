# AMM Minimum-Liquidity Floor

## Rationale

In a constant-product AMM (`x * y = k`), when either reserve approaches
zero the pricing function becomes numerically fragile:

- A tiny swap can move the price by several orders of magnitude.
- Rounding errors in integer arithmetic become proportionally large.
- The pool can be manipulated into near-zero states that are expensive
  or impossible to recover from.

The minimum-liquidity floor prevents reserves from being drained to dust
by rejecting any `remove_liquidity` or `swap_*` operation that would leave
either reserve below the configured threshold.

## Default Behaviour

The floor defaults to `0`, which preserves the current behaviour with no
restrictions. Pools that want protection must explicitly call
`set_min_liquidity` (admin only).

## Worked Example

**Setup:**
- Pool: reserve A = 1 000 000, reserve B = 2 000 000
- Minimum-liquidity floor = 100 000

**Allowed removal:**
- User wants to remove 900 000 A and 1 900 000 B.
- New reserves: A = 100 000, B = 100 000
- Both are ≥ 100 000 → **allowed**.

**Rejected removal:**
- User wants to remove 950 000 A and 1 900 000 B.
- New reserve A = 50 000 < 100 000 → **rejected with `BelowMinLiquidity`**.

**Swap rejection (A→B):**
- User swaps in 500 000 A.
- Output B ≈ (2 000 000 × 500 000) / (1 000 000 + 500 000) ≈ 666 667.
- New reserve B = 2 000 000 − 666 667 = 1 333 333.
- 1 333 333 ≥ 100 000 → allowed in this case.
- If the input amount were high enough to push B below 100 000,
  the swap would be rejected.

## Edge Cases

### Floor = 0 (default)
- No restriction. Full withdrawal and swaps that drain reserves are
  permitted. This is **backward compatible** with existing contracts.

### Floor = current reserve (or higher)
- `remove_liquidity` is completely blocked because any positive removal
  would leave the reserve below the floor.
- Swaps are blocked because any swap that reduces the outgoing reserve
  would violate the floor.

### Floor set very high (e.g. > 50% of reserves)
- The pool becomes effectively frozen for withdrawal and swap operations
  that reduce that reserve. Use with caution; the admin can always lower
  the floor again.

### Floor only protects the *outgoing* reserve on swaps
- For `swap_exact_a_for_b`, only the B reserve is checked (the reserve
  that decreases). The A reserve increases, so it cannot fall below the
  floor.
- For `swap_exact_b_for_a`, only the A reserve is checked.
- For `remove_liquidity`, **both** reserves are checked because both
  decrease.

### Overflow protection
- All arithmetic uses `checked_add`, `checked_sub`, `checked_mul`, and
  `checked_div` from the Rust standard library. If any intermediate
  computation would overflow, the operation returns `Overflow` rather
  than silently wrapping.

## API Reference

| Function | Role | Auth |
|---|---|---|
| `set_min_liquidity(floor)` | Set the floor | Admin only |
| `get_min_liquidity()` | Read the floor | Anyone |
| `remove_liquidity(to, a, b)` | Withdraw liquidity | Caller auth |
| `swap_exact_a_for_b(to, a_in, b_min)` | Swap A → B | Caller auth |
| `swap_exact_b_for_a(to, b_in, a_min)` | Swap B → A | Caller auth |

## Related Issues

- **Numerical fragility at near-zero reserves**: Without a floor, a pool
  with reserves of (1, 1 000 000) has a price of 1 000 000 per unit A.
  Adding or removing a single unit of A would swing the price by ~50%.
  The floor prevents this regime entirely.
