# StellarLend AMM

A constant-product AMM contract for the Stellar Soroban platform, implementing
Uniswap-v2-style swap mechanics with configurable basis-point fees, LP share
minting, and flash swaps.

---

## Documentation Index

| Document | Description |
|---|---|
| [AMM_MATH.md](./AMM_MATH.md) | Constant-product formula derivation, fee model, and worked examples |
| [INVERSE_SWAP_MATH.md](./INVERSE_SWAP_MATH.md) | Inverse swap (`inverse_swap_in`) derivation, rounding direction, and fee-independence with code-verified examples |
| [FLASH_SWAP_PROTOCOL.md](./FLASH_SWAP_PROTOCOL.md) | **Flash-swap call sequence, verify-k invariant, reentrancy guard, rollback semantics** |
| [SWAP_BOUND_INVARIANTS.md](./SWAP_BOUND_INVARIANTS.md) | Output-bound and k-monotonicity invariants proven by property tests |
| [SWAP_SYMMETRY.md](./SWAP_SYMMETRY.md) | Forward/backward swap symmetry proofs |
| [FEE_ACCOUNTING.md](./FEE_ACCOUNTING.md) | Per-side fee accumulator model |
| [MINT_INVARIANTS.md](./MINT_INVARIANTS.md) | LP share minting edge cases and invariants |
| [SQRT_PRECISION.md](./SQRT_PRECISION.md) | Integer square root precision guarantees |

---

## Quick Start

```rust
// Initialize pool
client.init_pool(&1_000_i128, &1_000_i128);

// Regular swap A → B
let out = client.swap_a_for_b(&100_i128, &30_i128 /*fee_bps*/);

// Flash swap (two ops in a single multi-op transaction)
client.flash_swap_a_for_b(&100_i128, &30_i128, &Bytes::new(&env));
// … caller executes arbitrary logic …
client.repay_flash_swap(&amount_in_min);
```

For a complete walkthrough of the flash-swap call sequence, invariants, and
failure modes see [FLASH_SWAP_PROTOCOL.md](./FLASH_SWAP_PROTOCOL.md).

---

## Adding and Removing Liquidity

```rust
// (amount_a, amount_b, shares)
let (a, b, shares) = client.add_liquidity(
    &caller,
    &amount_a_desired, &amount_b_desired,
    &amount_a_min, &amount_b_min,
    &min_shares,
    &deadline, // ledger timestamp; reverts with DeadlineExpired when now > deadline
);

// (amount_a, amount_b)
let (a, b) = client.remove_liquidity(&caller, &shares, &amount_a_min, &amount_b_min, &deadline);
```

`add_liquidity` pulls only the *optimal* amounts (`liquidity_math::quote_optimal_amounts`,
rounded down), so off-ratio excess stays with the caller. Violating any
`amount_*_min` / `min_shares` / remove minimum reverts with `SlippageExceeded`
(18) and moves no tokens; an expired deadline reverts with `DeadlineExpired`
(19). Minimums must be `>= 0`.

**Worked example.** Pool reserves `(2_000_000 A, 4_000_000 B)`. A depositor passes
`desired = (100_000, 500_000)`: `b_optimal = 100_000 * 4_000_000 / 2_000_000 = 200_000 <= 500_000`,
so only `(100_000, 200_000)` is transferred. If a front-run swap skews the pool to
roughly `(3_000_000, 2_670_000)` first, `b_optimal` drops to about `89_000`; with
`amount_b_min = 198_000` the deposit reverts instead of donating value to existing LPs.

---

## Running Tests

```sh
cargo test -p stellarlend-amm
```

To run only flash-swap tests:

```sh
cargo test -p stellarlend-amm flash_swap
```

To run only the protocol doc-tests:

```sh
cargo test -p stellarlend-amm flash_swap_protocol
```
