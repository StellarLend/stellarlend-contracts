# AMM Swap Fee Accounting

## Overview

The StellarLend AMM tracks protocol swap fees as a first-class, queryable quantity
instead of silently folding them into reserves. Every swap increments a per-side
fee accumulator that can be read via `get_accrued_fees()`.

## Storage Keys

| Key             | Type     | Description                                              |
|-----------------|----------|----------------------------------------------------------|
| `KEY_FEE_A`     | `i128`   | Cumulative fees earned from `swap_a_for_b` (token A side) |
| `KEY_FEE_B`     | `i128`   | Cumulative fees earned from `swap_b_for_a` (token B side) |

Both keys use Soroban **persistent** storage so they survive contract
instance upgrades and are visible to off-chain indexers.

## Accrual Formula

For each swap the fee is computed as:

```text
fee = amount_in * fee_bps / 10_000
```

- `amount_in` — the quantity of the input token supplied by the trader.
- `fee_bps` — the protocol fee in basis points (0 ≤ fee_bps ≤ 9,999).
- Division is **floor** (integer truncation), matching the Uniswap-v2
  convention already used for swap outputs.

### Direction Mapping

| Swap direction | Input token | Accumulator updated |
|----------------|-------------|---------------------|
| `swap_a_for_b` | A           | `KEY_FEE_A`         |
| `swap_b_for_a` | B           | `KEY_FEE_B`         |

The fee portion of `amount_in` is kept by the pool and recorded in the
corresponding accumulator. The swap output math is unchanged.

## Monotonicity Guarantees

1. **Non-decreasing:** Accumulators are only ever incremented; they are
   never decremented.
2. **Upper bound:** Because `fee_bps < 10,000` and division floors,
   `fee ≤ amount_in * 9,999 / 10,000 < amount_in`. The accumulator for
   any side therefore never exceeds the sum of `amount_in` values for
   swaps on that side.
3. **Overflow guard:** All arithmetic uses `checked_` operations. An
   overflow in accumulator update will panic rather than silently wrap.

## View: `get_accrued_fees(env) -> (i128, i128)`

Returns the current accumulated fees as `(fee_a, fee_b)`.

```rust
let (fee_a, fee_b) = client.get_accrued_fees();
```

## Worked Example

Start with a balanced pool: `reserve_a = 100,000`, `reserve_b = 100,000`.

### Swap 1: A → B

Trader swaps `1,000 A` with `fee_bps = 30`.

```text
fee = 1,000 * 30 / 10,000 = 3
```

After the swap:
- `KEY_FEE_A = 3`
- Reserves: `reserve_a = 101,000`, `reserve_b` decreased by `amount_out`.

### Swap 2: B → A

Trader swaps `500 B` with `fee_bps = 30`.

```text
fee = 500 * 30 / 10,000 = 1
```

After the swap:
- `KEY_FEE_B = 1`
- Reserves: `reserve_b` increased by `500`, `reserve_a` decreased by `amount_out`.

### Read State

```text
get_accrued_fees() => (3, 1)
```

The protocol has earned 3 units of token A and 1 unit of token B in swap
fees. These values are auditable and can be routed to a treasury via a
future protocol-fee withdrawal endpoint.

## Edge Cases

| Scenario                      | Behavior                                     |
|-------------------------------|----------------------------------------------|
| `fee_bps = 0`                 | `fee = 0`; accumulator unchanged             |
| `fee_bps = 9,999`             | `fee = amount_in * 9,999 / 10,000` (max fee) |
| Multiple consecutive swaps    | Accumulator = sum of individual fees         |
| `init_pool`                   | Both accumulators reset to `0`               |
| `add_liquidity` / `remove_liquidity` | Accumulators unaffected (no swap fee) |

## Interaction with Flash Swaps

Flash swaps (`flash_swap_a_for_b` / `repay_flash_swap`) do **not** accrue
fees into `KEY_FEE_A` / `KEY_FEE_B`. Fee accounting for flash swaps is
deferred to a future extension; the current accumulator only tracks
standard `swap_a_for_b` and `swap_b_for_a` calls.
