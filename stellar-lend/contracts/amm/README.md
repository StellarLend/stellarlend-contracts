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
| [TWAP.md](./TWAP.md) | On-chain TWAP oracle: ring buffer, `consult`, overflow and manipulation properties |

---

## Quick Start

```rust
// Recommended: Initialize pool configuration (one-shot, fee in basis points e.g. 30 = 0.3%)
client.initialize(&admin, &token_a, &token_b, &30);

// Bootstrap reserves with real token deposits via add_liquidity
client.add_liquidity(
    &admin,
    &1_000_000, &1_000_000,
    &1_000_000, &1_000_000,
    &0,
    &deadline,
);

// Deprecated: init_pool(admin, a, b, token_a, token_b) is one-shot and pulls
// a and b from admin via real token transfers. Subsequent calls return AlreadyInitialized.
// client.init_pool(&admin, &1_000_i128, &1_000_i128, &token_a, &token_b);

// Regular swap A → B: `trader` authorizes, `amount_in` of token A is pulled
// from the trader and the output of token B is paid back. Fails with
// `SlippageExceeded` if the output is below `min_amount_out`.
// Reserves equal the pool's token balances (fees stay inside the reserves);
// fee-on-transfer tokens are unsupported.
let out = client.swap_a_for_b(&trader, &100_i128, &min_amount_out);

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
