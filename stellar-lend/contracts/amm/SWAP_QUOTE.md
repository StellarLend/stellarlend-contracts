# `get_swap_quote` — Read-Only Swap Quotation

## Rationale

Off-chain clients (UIs, bots, arbitrage logic) need to preview swap outcomes
before committing a transaction.  Previewing by actually executing a swap and
rolling it back wastes gas and leaves a footprint in Soroban's resource-use
accounting.

`get_swap_quote` solves this by running the **exact same constant-product
math** that `swap_a_for_b` and `swap_b_for_a` execute — including the same
`compute_fee` call — but:

* **never writing to persistent storage**, so the pool state is unchanged, and
* **never emitting events**, so indexers do not record phantom swaps.

Because the formula is shared with the live path, the quoted `amount_out` is
guaranteed to match an actual swap to the unit, provided the pool state has not
changed between the quote and the swap.

---

## Function signature

```rust
pub fn get_swap_quote(
    env: Env,
    amount_in: i128,
    fee_bps: i128,
    a_for_b: bool,
) -> Result<SwapQuote, AmmPoolError>
```

| Parameter   | Type    | Description                                                    |
|-------------|---------|----------------------------------------------------------------|
| `amount_in` | `i128`  | Positive token amount being offered.                           |
| `fee_bps`   | `i128`  | Fee in basis points (e.g. 30 = 0.30 %). Use `get_fee_bps()` to pass the current admin-configured value. |
| `a_for_b`   | `bool`  | `true` → quote a A→B swap; `false` → quote a B→A swap.        |

### Return value — `SwapQuote`

```rust
pub struct SwapQuote {
    pub amount_out:      i128,  // tokens the caller would receive
    pub fee:             i128,  // fee deducted from amount_in
    pub reserve_a_after: i128,  // projected reserve_a (not persisted)
    pub reserve_b_after: i128,  // projected reserve_b (not persisted)
}
```

---

## Formula (Uniswap-v2 constant product)

```
fee               = amount_in × fee_bps / 10 000           (floor)
fee_adj           = 10 000 − fee_bps
amount_in_net     = amount_in × fee_adj
amount_out        = (amount_in_net × reserve_out)
                  / (reserve_in × 10 000 + amount_in_net)  (floor)
```

This is identical to the live `swap_a_for_b` / `swap_b_for_a` paths.

---

## Worked numeric example

Pool state:

| reserve_a | reserve_b |
|-----------|-----------|
| 1 000 000 | 2 000 000 |

Quote: swap **50 000 A → B**, fee = **30 bps** (0.30 %).

```
fee               = 50 000 × 30 / 10 000 = 150
fee_adj           = 10 000 − 30 = 9 970
amount_in_net     = 50 000 × 9 970 = 498 500 000
amount_out        = (498 500 000 × 2 000 000)
                  / (1 000 000 × 10 000 + 498 500 000)
                = 997 000 000 000 000
                  / 10 498 500 000
                ≈ 94 966   (floor)

reserve_a_after = 1 000 000 + 50 000 = 1 050 000
reserve_b_after = 2 000 000 − 94 966 = 1 905 034
```

A live `swap_a_for_b(50_000)` on the same pool produces `amount_out = 94 966`
and leaves reserves `(1 050 000, 1 905 034)` — identical to the quote.

---

## Error cases

| Error                     | Condition                                             |
|---------------------------|-------------------------------------------------------|
| `AmmPoolError::NonPositiveAmount` | `amount_in <= 0`                            |
| `AmmPoolError::EmptyPool`         | Either reserve is zero at query time. Returns a typed error instead of panicking (unlike the live swap path). |
| `AmmPoolError::Overflow`          | An intermediate `checked_mul` / `checked_add` would overflow `i128`. |

---

## Edge-case notes

### Zero reserves
The live swap paths panic on an empty pool.  `get_swap_quote` returns
`Err(AmmPoolError::EmptyPool)` instead, allowing callers to handle the
condition without catching a Soroban contract-abort signal.

### Large amounts near pool depletion
The Uniswap-v2 constant-product formula guarantees
`amount_out < reserve_out`, so `reserve_out_after >= 1` even when
`amount_in` approaches the size of `reserve_in`.  No special clamping is
needed.

### Fee of 0 bps
`fee = 0`, `fee_adj = 10 000`, and the formula reduces to the fee-free
constant-product output.  This is identical to what the live path produces
when the admin has called `set_fee_bps(0)`.

### Staleness
`get_swap_quote` reads the current pool reserves from storage at the time
of the call.  If another transaction changes the pool between the quote and
the swap, the quoted `amount_out` will differ from the actual result.
Callers should use the slippage guard (`amount_out_min`) on the live swap
to protect against staleness.

### No price-impact guard
The live `swap_a_for_b` path enforces the admin-configured `max_impact_bps`
guard and reverts if the impact is exceeded.  `get_swap_quote` does **not**
apply this guard; it returns the raw constant-product projection regardless
of configured impact limits.  This is intentional: the quote is a pure math
projection, not an authorization check.
