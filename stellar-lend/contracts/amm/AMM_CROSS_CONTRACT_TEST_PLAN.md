# AMM `swap_b_for_a` Cross-Contract Integration Test Plan

> **Status:** Design doc. The `amm` crate does not currently compile on `main`
> (pre-existing, unrelated breakage in `lib.rs`). This document specifies the
> stub-consumer design and the integration test now; the test
> (`swap_b_for_a_integration_test.rs`) will follow once the crate builds.

## Motivation

`StandaloneAmm` exposes `swap_a_for_b` and `swap_b_for_a` as `#[contract]`
entrypoints, but existing tests exercise the swap math **within** the crate. No
test registers the AMM and calls `swap_b_for_a` through its **generated contract
client from an external (consumer) contract** — which is how a real lending
integration consumes it. This plan covers that gap end-to-end.

## Stub consumer design

A minimal second contract in the test module that performs a cross-contract
invocation of the AMM via the generated client:

```rust
#[contract]
pub struct SwapConsumer;

#[contractimpl]
impl SwapConsumer {
    /// Calls `swap_b_for_a` on the AMM at `amm` and returns the AMM's output.
    /// Demonstrates the cross-contract invocation path a lending market uses.
    pub fn consume_swap_b_for_a(env: Env, amm: Address, amount_in: i128, fee_bps: i128) -> i128 {
        let client = StandaloneAmmClient::new(&env, &amm);
        client.swap_b_for_a(&amount_in, &fee_bps)
    }
}
```

The consumer holds **no AMM logic**; it only forwards through
`StandaloneAmmClient`, so the test validates the public ABI / client boundary,
not the in-crate math path.

## Invocation flow

1. `let env = Env::default();`
2. Register the AMM: `let amm_id = env.register(StandaloneAmm, ());`
3. Register the consumer: `let consumer_id = env.register(SwapConsumer, ());`
4. Init the pool through the AMM client: `amm.init_pool(&ra0, &rb0)`.
5. Snapshot `get_reserves()` and `get_accrued_fees()` before the swap.
6. Drive the swap **through the consumer**:
   `consumer.consume_swap_b_for_a(&amm_id, &amount_in, &fee_bps)`.
7. Read back `get_reserves()` and `get_accrued_fees()` from the AMM client.

All helpers carry NatSpec-style `///` doc comments.

## Expected math (constant product, B in -> A out)

With reserves `(ra, rb)`, fee `f` bps, input `amount_in` of B:

```
fee               = compute_fee(amount_in, f)            // floor
amount_in_w_fee   = amount_in * (10_000 - f)
amount_out (A)    = (amount_in_w_fee * ra) / (rb * 10_000 + amount_in_w_fee)   // floor
new_rb            = rb + amount_in
new_ra            = ra - amount_out
new_fee_b        += fee
```

`amount_out` floors (pool never over-pays), so assertions allow a `±1` rounding
band on derived quantities while reserves are checked exactly against the formula
above.

## Assertions

1. **Return value** — consumer's returned `amount_out` equals the formula's
   floored result.
2. **Reserve mutation** — post-swap `get_reserves() == (ra - amount_out, rb + amount_in)`.
3. **Constant-product** — `new_ra * new_rb >= ra * rb` (k non-decreasing, matching
   `assert_k_monotonic`).
4. **Fee accounting** — `get_accrued_fees()` B-component increases by exactly
   `compute_fee(amount_in, fee_bps)`; A-component unchanged.
5. **Reentrancy at the client boundary** — start a flash swap
   (`flash_swap_a_for_b`) and, while it is unpaired (`FlashActive == true`), the
   consumer's `swap_b_for_a` call is rejected with `ReentrantFlashSwap`. Use
   `try_*` on the client to assert the error rather than panicking the test.

## Edge cases

- **Minimal input** — `amount_in = 1`; output may floor to `0`; reserves still
  consistent and fee handled.
- **Large input near reserve** — `amount_in` close to `rb`; assert no overflow
  (checked arithmetic) and `amount_out < ra`.
- **Empty pool** — swap before `init_pool` / on a zeroed reserve returns
  `EmptyPool` at the client boundary.
- **Invalid fee** — `fee_bps` out of `[0, 9_999]` is rejected.

## File layout

`stellar-lend/contracts/amm/src/swap_b_for_a_integration_test.rs`, registered via
`#[cfg(test)] mod swap_b_for_a_integration_test;` in `lib.rs`. If the crate
prefers external integration tests, an equivalent `tests/` file using the
exported client works identically.
