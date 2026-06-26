# Mint Share Invariants

`calculate_mint_shares` protects existing LPs by minting new shares from the
smaller side of the deposit:

```text
liquidity_0 = amount_0 * total_supply / reserve_0
liquidity_1 = amount_1 * total_supply / reserve_1
minted      = min(liquidity_0, liquidity_1)
```

That rule means a successful deposit cannot reduce an existing holder's reserve
claim. For token 0, non-dilution is checked without floating point as:

```text
(reserve_0 + amount_0) / (total_supply + minted) >= reserve_0 / total_supply
```

Cross-multiplied:

```text
(reserve_0 + amount_0) * total_supply >= reserve_0 * (total_supply + minted)
```

The same check is applied to token 1.

## First Deposit Lock

On the first deposit, the pool mints:

```text
sqrt(amount_0 * amount_1)
```

`MINIMUM_LIQUIDITY` is permanently locked, and the user receives the remainder.
The property test asserts:

```text
locked == MINIMUM_LIQUIDITY
minted + locked == sqrt(amount_0 * amount_1)
```

## Worked Example

Given:

```text
total_supply = 10_000
reserve_0    = 10_000
reserve_1    = 20_000
amount_0     = 1_000
amount_1     = 4_000
```

The candidate share amounts are:

```text
liquidity_0 = 1_000 * 10_000 / 10_000 = 1_000
liquidity_1 = 4_000 * 10_000 / 20_000 = 2_000
minted      = min(1_000, 2_000) = 1_000
```

After minting:

```text
supply_after  = 11_000
reserve_0_after = 11_000
reserve_1_after = 24_000
```

Token 0 backing stays equal:

```text
11_000 / 11_000 == 10_000 / 10_000
```

Token 1 backing increases:

```text
24_000 / 11_000 > 20_000 / 10_000
```

The new proptest suite checks this over random pool states and deposit
sequences, while also covering lopsided deposits and the first-deposit
minimum-liquidity lock.
