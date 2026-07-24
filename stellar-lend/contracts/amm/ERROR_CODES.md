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

## Example: Invariant Violation

The AMM maintains the constant product invariant $k = \text{reserve}_a \times \text{reserve}_b$.
- **During a swap:** The product $k$ must not decrease: $k_{after} \ge k_{before}$.
- **During liquidity removal:** The product $k$ must not increase: $k_{after} \le k_{before}$.

If either of these conditions is failed, the contract returns `InvariantViolation` (Code 5) and rolls back the transaction.
