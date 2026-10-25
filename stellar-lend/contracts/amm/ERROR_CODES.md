# AMM Pool Error Codes

This document maps the `AmmPoolError` discriminants to their causes. These codes are stable and should be used by callers to handle errors programmatically.

| Code | Error Variant | Cause |
| :--- | :--- | :--- |
| 1 | `EmptyPool` | One or both of the pool reserves are zero, making swaps impossible. |
| 2 | `NonPositiveAmount` | An input amount (e.g., `amount_in` or `amount_out`) was zero or negative. |
| 3 | `InsufficientReserves` | The pool does not have enough reserves to satisfy the requested removal or swap. |
| 4 | `Overflow` | An arithmetic operation overflowed or underflowed. |
| 5 | `InvariantViolation` | A core pool invariant was breached (e.g., $k$ decreased during a swap or increased during liquidity removal). |
| 6 | `ReentrantFlashSwap` | A state-mutating operation was attempted while a flash swap was already in flight. |
| 7 | `UnauthorizedCaller` | Caller is not the flash-swap initiator. |
| 8 | `FeeBpsOutOfRange` | `fee_bps` is outside `0..=MAX_FEE_BPS`. |
| 9 | `InsufficientLiquidityMinted` | LP shares minted would be zero (deposit too small). |
| 10 | `ZeroSupply` | Pool has zero LP supply (cannot burn). |
| 11 | `BurnExceedsSupply` | Burn amount exceeds total LP supply. |
| 12 | `InvalidBurnAmount` | Invalid burn amount (non-positive). |
| 13 | `ZeroReserve` | Pool reserves are zero (cannot compute share ratio). |
| 14 | `InsufficientLpBalance` | Caller has insufficient LP balance for requested burn. |
| 15 | `ZeroOutput` | Computed swap output floors to zero after fees — dust input rejected. See [DUST_SWAP_GUARD.md](./DUST_SWAP_GUARD.md). |
| 16 | `AmountBelowMinSwapIn` | Input (or flash `amount_out`) is below the admin-configured `min_swap_in` floor. |
| 17 | `BelowMinLiquidity` | Operation would leave a reserve below the admin-configured `min_liquidity` floor. See [MIN_LIQUIDITY.md](./MIN_LIQUIDITY.md). |
| 18 | `SlippageExceeded` | `add_liquidity` / `remove_liquidity` result is below `amount_*_min` or `min_shares`, or a swap output is below `min_amount_out`. |
| 19 | `DeadlineExpired` | Ledger timestamp is past the caller-supplied `deadline`. |
| 20 | `PriceImpactExceeded` | Swap would move the spot price by more than `max_impact_bps`. See [PRICE_IMPACT_GUARD.md](./PRICE_IMPACT_GUARD.md). |
| 21 | `InvalidTrader` | The trader is the pool contract itself. |
| 22 | `TwapInsufficientHistory` | `consult` requested a window longer than the retained TWAP history. See [TWAP.md](./TWAP.md). |
| 23 | `TwapObservationNotFound` | TWAP ring slot is out of range or has never been written. See [TWAP.md](./TWAP.md). |
| 24 | `AlreadyInitialized` | The pool has already been initialized (one-shot initialization). |
| 25 | `IdenticalTokens` | `token_a` and `token_b` must be distinct token addresses. |

## Example: Invariant Violation

The AMM maintains the constant product invariant $k = \text{reserve}_a \times \text{reserve}_b$.
- **During a swap:** The product $k$ must not decrease: $k_{after} \ge k_{before}$.
- **During liquidity removal:** The product $k$ must not increase: $k_{after} \le k_{before}$.

If either of these conditions is failed, the contract returns `InvariantViolation` (Code 5) and rolls back the transaction.
