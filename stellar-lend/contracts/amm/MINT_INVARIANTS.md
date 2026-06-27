# AMM mint invariants

## Subsequent-deposit share math

For any deposit after the first one, the share formula uses the existing pool reserves:

- shares = min(amount_0 * total_supply / reserve_0, amount_1 * total_supply / reserve_1)

This calculation must never divide by zero. If either reserve is zero, the pool is in a drained or otherwise invalid state for share minting, so minting must fail with a typed error instead of trapping.

### Worked example

If a pool has `total_supply = 10_000`, reserves `(10_000, 10_000)`, and a depositor adds `(2_000, 2_000)`, then:

- liquidity_0 = 2_000 * 10_000 / 10_000 = 2_000
- liquidity_1 = 2_000 * 10_000 / 10_000 = 2_000
- shares = min(2_000, 2_000) = 2_000

If either reserve is zero, the function returns `ZeroReserve` rather than panicking.

## First deposit behavior

The first deposit still preserves the existing minimum-liquidity guard to mitigate donation attacks. The minimum share lock remains unchanged.
