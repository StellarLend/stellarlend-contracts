# AMM Math Reference

## Overview

This document describes the mathematical model used by the AMM contract.

The implementation follows a constant-product automated market maker (AMM) model similar to Uniswap V2. Swap calculations include configurable basis-point fees, liquidity minting follows proportional ownership, and the implementation enforces invariants to protect pool integrity.

---

## Constant Product Invariant

The pool maintains the invariant:

```
x * y = k
```

Where:

* `x` = reserve of token A
* `y` = reserve of token B
* `k` = constant product

Swaps should never decrease the invariant after fees are applied.

---

## Swap Formula

Given:

```
reserve_in
reserve_out
amount_in
fee_bps
```

Fee-adjusted input:

```
amount_in_with_fee = amount_in * (10000 - fee_bps)
```

Output amount:

```
amount_out =
(amount_in_with_fee * reserve_out)
/
(reserve_in * 10000 + amount_in_with_fee)
```

After the swap:

```
reserve_in += amount_in
reserve_out -= amount_out
```

The implementation verifies that the constant-product invariant has not decreased.

---

## Fee Model

Fees are expressed in basis points.

```
10000 bps = 100%
30 bps = 0.30%
```

The fee-adjusted multiplier is:

```
10000 - fee_bps
```

Example:

```
fee_bps = 30

amount_in_with_fee =
amount_in × 9970
```

---

## Worked Example

Initial reserves:

```
reserve_a = 1000
reserve_b = 2000
amount_in = 100
fee = 30 bps
```

Fee-adjusted input:

```
100 × 9970 = 997000
```

Numerator:

```
997000 × 2000 = 1,994,000,000
```

Denominator:

```
1000 × 10000 + 997000
=
10,997,000
```

Output:

```
amount_out =
1,994,000,000 / 10,997,000
≈ 181
```

Updated reserves:

```
reserve_a = 1100
reserve_b = 1819
```

The invariant is checked after the swap.

---

## k Invariant

The contract verifies that:

```
k_before = reserve_a × reserve_b
k_after  = new_reserve_a × new_reserve_b
```

For swaps and liquidity additions:

```
k_after >= k_before
```

For liquidity removal:

```
k_after <= k_before
```

Any violation causes the transaction to panic.

---

## Liquidity Minting

For the first liquidity provider:

```
shares =
sqrt(amount_0 × amount_1)
-
MINIMUM_LIQUIDITY
```

The minimum liquidity is permanently locked.

For later deposits:

```
shares =
min(
amount_0 × total_supply / reserve_0,
amount_1 × total_supply / reserve_1
)
```

---

## MINIMUM_LIQUIDITY

The implementation permanently locks:

```
MINIMUM_LIQUIDITY = 1000
```

This prevents donation attacks that could otherwise inflate LP token value and cause future liquidity providers to receive zero shares because of integer truncation.

---

## Donation Attack Protection

Without permanently locked liquidity, an attacker could:

1. Make a very small initial deposit.
2. Donate a large amount of tokens directly to the pool.
3. Inflate the LP share value.
4. Cause later deposits to mint zero shares because of integer division.

Locking `MINIMUM_LIQUIDITY` increases the attack cost and prevents this exploit.

---

## Summary

The AMM implementation provides:

* Constant-product pricing
* Basis-point fee calculation
* Monotonic invariant checking
* Safe liquidity minting
* Protection against donation attacks using permanently locked minimum liquidity
