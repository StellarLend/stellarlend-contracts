# StellarLend AMM

A constant-product AMM contract for the Stellar Soroban platform, implementing
Uniswap-v2-style swap mechanics with configurable basis-point fees, LP share
minting, and flash swaps.

---

## Documentation Index

| Document | Description |
|---|---|
| [AMM_MATH.md](./AMM_MATH.md) | Constant-product formula derivation, fee model, and worked examples |
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
