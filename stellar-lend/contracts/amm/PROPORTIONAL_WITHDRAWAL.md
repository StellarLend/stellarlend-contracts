# Proportional LP withdrawal

Issue: [#1257](https://github.com/StellarLend/stellarlend-contracts/issues/1257)

## Problem

`remove_liquidity(caller, shares)` burns an absolute share count. Integrators
that want to exit a **percentage of a position** must do the bps math off-chain
and risk race conditions if the balance changes between quote and send.

This entrypoint moves the math on-chain with pool-favourable rounding and
explicit fee-counter settlement.

## API

```rust
pub fn remove_liquidity_proportional(
    env: Env,
    caller: Address,
    shares_bps: i128,          // 1..=10_000
) -> Result<(i128, i128), AmmPoolError>
```

### Formula

```text
burn_shares = floor(user_lp_balance × shares_bps / 10_000)
out_a       = floor(reserve_a × burn_shares / total_supply)
out_b       = floor(reserve_b × burn_shares / total_supply)
fee_a_out   = floor(KEY_FEE_A  × burn_shares / total_supply)
fee_b_out   = floor(KEY_FEE_B  × burn_shares / total_supply)
```

All divisions floor. That means the residual constant-product `k` never
increases relative to a pure proportional exit (pool-favourable rounding).

### Fee settlement

`KEY_FEE_A` / `KEY_FEE_B` are accounting counters for protocol fees that already
live inside the pool reserves (Uniswap-v2 style). On proportional exit the
caller's share of those counters is **debited** so remaining LPs' fee ownership
stays consistent. Token transfer amounts come only from the reserve burn
(fees are not double-paid).

### Events

Topic: `("liquidity_removed", caller)`  
Data: `(out_a, out_b, fee_a_out, fee_b_out, shares_bps)`

### Guards

- `assert_no_active_flash_swap` — same reentrancy guard as `remove_liquidity`
- `shares_bps ∈ 1..=10_000` else `SharesBpsOutOfRange`
- `burn_shares > 0` else `InvalidBurnAmount`
- `assert_k_monotonic(..., expect_increase=false)`

## Worked example

Pool state before exit:

| Field | Value |
|-------|-------|
| `reserve_a` | 1_000 |
| `reserve_b` | 2_000 |
| `total_supply` | 500 |
| user LP balance | 200 |
| `KEY_FEE_A` | 50 |
| `KEY_FEE_B` | 80 |

Caller exits `shares_bps = 5_000` (50 % of their position):

```text
burn_shares = floor(200 × 5000 / 10000) = 100
out_a       = floor(1000 × 100 / 500)   = 200
out_b       = floor(2000 × 100 / 500)   = 400
fee_a_out   = floor(50 × 100 / 500)     = 10
fee_b_out   = floor(80 × 100 / 500)     = 16
```

After:

| Field | Value |
|-------|-------|
| `reserve_a` | 800 |
| `reserve_b` | 1_600 |
| `total_supply` | 400 |
| user LP balance | 100 |
| `KEY_FEE_A` | 40 |
| `KEY_FEE_B` | 64 |

`k_before = 1_000 × 2_000 = 2_000_000`  
`k_after  = 800 × 1_600 = 1_280_000` ≤ `k_before` ✓

## Rounding proof sketch

Let `s` = burn shares, `S` = total supply, `R` = a reserve.
`out = ⌊R·s / S⌋` so `out ≤ R·s / S`, hence
`R' = R − out ≥ R − R·s/S = R·(S−s)/S`.
For both reserves the product satisfies
`R_a' · R_b' ≥ (R_a · R_b) · ((S−s)/S)²` when equality holds in the continuous
case; with floor on both sides residual dust stays in the pool, so
`k_after ≤ k_before` is preserved by `assert_k_monotonic`.

## Compatibility

`remove_liquidity` is unchanged. Existing monotonicity tests continue to pass.
