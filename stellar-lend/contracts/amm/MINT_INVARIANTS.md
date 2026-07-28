# Mint-share invariant documentation

## Overview

`calculate_mint_shares` computes the number of LP (liquidity provider) shares
to mint for a deposit.  Two regimes exist:

1. **First deposit** (`total_supply == 0`): the shares are derived from
   `sqrt(amount_0 × amount_1)`.  A fixed amount (`MINIMUM_LIQUIDITY = 1000`)
   is permanently locked in the pool to mitigate the donation/inflation attack.
2. **Subsequent deposits** (`total_supply > 0`): the shares equal
   `min(liquidity_0, liquidity_1)` where
   `liquidity_i = amount_i × total_supply / reserve_i` (integer floor division).

---

## Invariants

### I-1 — First-deposit minimum-liquidity lock

On the first deposit:

- `locked = MINIMUM_LIQUIDITY`
- `shares = sqrt(amount_0 × amount_1) - MINIMUM_LIQUIDITY`
- If `sqrt(amount_0 × amount_1) ≤ MINIMUM_LIQUIDITY`, the deposit is rejected
  with `Err(InsufficientLiquidityMinted)`.

**Rationale**
Without a minimum locked supply, an attacker could deposit 1 wei of each token
(receiving 1 share), then donate a massive amount to the pool.  The share price
inflates such that subsequent depositors receive 0 shares due to integer
truncation, allowing the attacker to steal their deposits.  Locking 1000
shares raises the attacker's cost to make a victim's deposit round to 0 by
a factor of 1000, making the attack economically unviable.

### I-2 — `min(liquidity_0, liquidity_1)` rule

For every subsequent deposit:

```
shares      = min(liquidity_0, liquidity_1)
locked      = 0
liquidity_0 = amount_0 × total_supply / reserve_0    [floor]
liquidity_1 = amount_1 × total_supply / reserve_1    [floor]
```

If both `liquidity_0` and `liquidity_1` are zero, the deposit is rejected
as `Err(InsufficientLiquidityMinted)`.  If either reserve is zero, it is
rejected as `Err(ZeroReserve)`.

### I-3 — Non-dilution (per-share backing non-decreasing)

A deposit must never reduce an existing LP's per-share claim on reserves.
Formally, for each asset `i ∈ {0, 1}`:

```
reserve_i           (reserve_i + amount_i)
——————————   ≤     ————————————————————————
total_supply        (total_supply + shares)
```

Cross-multiplying (all values are non-negative):

```
shares × reserve_i  ≤  amount_i × total_supply
```

The right-hand side is exactly `liquidity_i × reserve_i` (by definition of
`liquidity_i`), so the inequality is equivalent to:

```
shares  ≤  liquidity_i
```

Since `shares = min(liquidity_0, liquidity_1)`, the inequality holds for
both assets.  The property-based tests verify this directly over random
`(total_supply, amount_0, amount_1, reserve_0, reserve_1)` tuples.

---

## Worked example

| Variable | Value |
|---|---|
| `total_supply` | 1 000 000 |
| `reserve_0` | 100 000 |
| `reserve_1` | 100 000 |
| `amount_0` | 1 000 000 000 |
| `amount_1` | 1 |

**Step 1 — compute liquidity values**

```
liquidity_0 = 1_000_000_000 × 1_000_000 / 100_000 = 10_000_000_000
liquidity_1 = 1 × 1_000_000 / 100_000 = 10
```

**Step 2 — pick the minimum**

```
shares = min(10_000_000_000, 10) = 10
```

**Step 3 — verify non-dilution**

Asset 0:
```
10 × 100_000  ≤  1_000_000_000 × 1_000_000
1_000_000     ≤  1_000_000_000_000_000    ✓
```

Asset 1:
```
10 × 100_000  ≤  1 × 1_000_000
1_000_000     ≤  1_000_000                  ✓  (tight)
```

Both inequalities hold: existing LP's per-share backing does not decrease.

---

## Property-based test strategy

The file `mint_shares_proptest.rs` uses `proptest` to assert:

1. **I-1** — lock and shares formula for random `(amount_0, amount_1)` on a
   first deposit.
2. **I-2** — `min(liquidity_0, liquidity_1)` formula for random
   `(total_supply, amount_0, amount_1, reserve_0, reserve_1)`.
3. **I-3** — `shares × reserve_i ≤ amount_i × total_supply` over the same
   random tuples.

Edge cases (minimum boundary, lopsided deposits, truncation-to-zero, tight
non-dilution bound) are covered by deterministic unit tests in the same file.

---

## Coverage

- 100 % of `calculate_mint_shares` branch outcomes (success, overflow,
  zero-reserve, insufficient-liquidity) are exercised.
- All `proptest` strategies use domain-sampled ranges that respect the
  `i128` arithmetic limits of the contract.
