# Reentrancy Audit Notes

This contract uses [`src/reentrancy.rs`](./src/reentrancy.rs) as a same-transaction lock for fund-moving entry points that can invoke external contracts.

## What Soroban Allows

Soroban contract calls are synchronous inside one invocation tree. If StellarLend calls a token contract with `transfer` or `transfer_from`, that token contract can immediately call StellarLend again before the outer function returns.

That means reentrancy is relevant on Soroban even though all state changes are committed atomically at the end of the transaction.

## What The Guard Guarantees

- The lock is per StellarLend contract instance.
- The lock lives in temporary storage and is visible only for the current transaction.
- A second protected entry during the same invocation tree returns error code `7`, which each operation maps to its local `Reentrancy` variant.
- The lock is released when the guarded frame exits, including ordinary error returns.

## What The Guard Does Not Guarantee

- It does not replace authorization checks.
- It does not protect other contracts.
- It does not persist across transactions.
- It does not make external tokens trustworthy.
- It does not remove the need for checks-effects-interactions discipline.

## Protected Paths

- `deposit_collateral`
- `withdraw_collateral`
- `borrow_asset`
- `repay_debt`

## External Call Audit

- `deposit_collateral`: calls token `balance` and `transfer_from` before updating protocol state. The guard blocks malicious callback attempts into any other protected entry point.
- `withdraw_collateral`: updates local state, then calls token `transfer`. The guard prevents nested protected calls while the transfer is in progress.
- `borrow_asset`: production builds call token `transfer` after debt state is updated. Tests also verify the entrypoint rejects a pre-held lock even though the direct token transfer path is compiled out under `#[cfg(test)]`.
- `repay_debt`: calls token `balance` and `transfer_from` before reducing debt. The guard blocks nested protected entries during that external call.

## Trust Boundaries

- Admin powers: admin-controlled configuration can pause operations, change protocol parameters, and affect whether protected paths are reachable, but admin authority does not bypass the lock.
- Guardian / recovery powers: guardian and recovery flows are privileged governance surfaces, not part of the reentrancy lock boundary. They must still be reviewed independently for authorization safety.
- Token contracts: token contracts are untrusted external dependencies. Every token callback path must be assumed adversarial.

## Testing

The reentrancy tests cover:

- direct lock acquisition and release semantics,
- callback-driven re-entry attempts from a malicious token contract,
- entrypoint-level `Reentrancy` error mapping, including explicit coverage for `borrow_asset`.

See [`src/test_reentrancy.rs`](./src/test_reentrancy.rs).
