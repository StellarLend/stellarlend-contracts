# Dust-Swap Guard

## Overview

Constant-product AMMs compute swap output with **integer floor division**.
For a sufficiently small `amount_in`, the formula can return **`amount_out = 0`**
while still absorbing the input into the input-side reserve:

```text
amount_in_with_fee = amount_in * (10_000 - fee_bps)
amount_out         = (amount_in_with_fee * reserve_out)
                   / (reserve_in * 10_000 + amount_in_with_fee)   # floor
```

When `amount_out == 0`:

- the caller's tokens are still credited to `reserve_in`
- `reserve_out` is unchanged
- `k = reserve_a × reserve_b` **increases** (so `assert_k_monotonic` passes)
- fee counters may still tick (saturating)

An attacker can therefore grind pool state with repeated dust swaps that
transfer no economic value to the victim and leave no typed error for
callers / indexers to key on.

This guard closes that vector.

---

## Guard Behaviour

### 1. Always-on zero-output rejection

After the constant-product math, both live swap paths check:

```rust
if amount_out == 0 {
    return Err(AmmPoolError::ZeroOutput); // code 15
}
```

Applied in:

- `swap_a_for_b`
- `swap_b_for_a`

Any swap that already yields a non-zero output is **unchanged**.

`get_swap_quote` is intentionally **not** gated — quotes may still return
`amount_out = 0` so front-ends can surface "too small" before submitting.

### 2. Optional admin `min_swap_in` floor

Admins may raise a hard floor via `set_min_swap_in(admin, min_swap_in)`:

| Path | Compared against |
|------|------------------|
| `swap_a_for_b` / `swap_b_for_a` | `amount_in` |
| `flash_swap_a_for_b` | `amount_out` (flash is amount-out driven) |

- Default: `DEFAULT_MIN_SWAP_IN = 0` (floor disabled).
- When `min_swap_in > 0` and the compared amount is strictly below the floor,
  the call returns `AmmPoolError::AmountBelowMinSwapIn` (code 16) **before**
  any reserve math runs.
- Negative floors are rejected with `NonPositiveAmount`.

Flash swaps already require `amount_out > 0`, so the classic zero-output
grinding vector does not apply there; the floor still prevents dust flash
sessions from opening and locking the pool via `FlashActive`.

---

## Worked Example

| Parameter   | Value     |
|-------------|-----------|
| `reserve_a` | 1 000 000 |
| `reserve_b` | 1 000 000 |
| `fee_bps`   | 0         |
| `amount_in` | 1         |

```text
amount_out = 1 * 1_000_000 / (1_000_000 + 1)
           = 1_000_000 / 1_000_001
           = 0   (floor)
```

**Before the guard:** reserves become `(1_000_001, 1_000_000)`, k rises,
caller receives nothing.

**After the guard:** `Err(ZeroOutput)`, storage untouched.

Smallest allowed input for a 1-unit output (fee = 0, equal reserves R):

```text
amount_in >= ceil(R / (R - 1)) = 2   for R > 1
```

So `amount_in = 2` succeeds with `amount_out = 1`.

---

## Error Codes

| Code | Variant                 | When |
| :--- | :---                    | :--- |
| 15   | `ZeroOutput`            | Computed swap output floors to zero. |
| 16   | `AmountBelowMinSwapIn`  | Input (or flash `amount_out`) is below the admin floor. |

See also [ERROR_CODES.md](./ERROR_CODES.md).

---

## API Surface

| Function | Role |
|----------|------|
| `set_min_swap_in(admin, min_swap_in)` | Admin-only floor setter (`min_swap_in >= 0`). |
| `get_min_swap_in()` | Read current floor (default `0`). |
| `swap_a_for_b` / `swap_b_for_a` | Zero-output + floor checks. |
| `flash_swap_a_for_b` | Floor check on `amount_out`. |

---

## Tests

See `src/dust_swap_guard_test.rs`:

- zero-output dust rejection on both swap directions
- smallest one-unit-output allowance on both directions
- no reserve / fee mutation on rejection
- admin floor reject / allow
- flash-swap floor enforcement
- stable error codes 15 / 16
- mid-size non-dust output regression (`amount_in=1000 → out=909` at R=10k, fee=0)

Run:

```bash
cargo test -p stellarlend-amm dust_swap
```

---

## Non-goals

- Does **not** change the constant-product formula or fee accounting for
  any swap that already produced non-zero output.
- Does **not** replace the price-impact guard (`PRICE_IMPACT_GUARD.md`) or
  k-monotonicity checks — those remain complementary.
- Does **not** gate read-only quotes (`get_swap_quote`).
