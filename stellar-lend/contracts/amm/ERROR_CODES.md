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

## Example: Invariant Violation

The AMM maintains the constant product invariant $k = \text{reserve}_a \times \text{reserve}_b$.
- **During a swap:** The product $k$ must not decrease: $k_{after} \ge k_{before}$.
- **During liquidity removal:** The product $k$ must not increase: $k_{after} \le k_{before}$.

If either of these conditions is failed, the contract returns `InvariantViolation` (Code 5) and rolls back the transaction.
