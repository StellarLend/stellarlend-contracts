# Reentrancy Guarantees

## Overview
StellarLend contracts utilize a standard `ReentrancyGuard` implemented in `src/reentrancy.rs`. This guard provides robust, environment-level protection against malicious cross-contract callbacks that attempt to manipulate the protocol's state synchronously inside a single transaction.

## Mechanism
The guard leverages Soroban's Temporary Storage feature to create a lock. 
When a protected function is entered:
1. It attempts to read a `Symbol("REENTRANCY_LOCK")` from temporary storage.
2. If the lock exists, it means the current call stack already contains an operation within the protected scope. The guard immediately aborts the call by returning a standardized `Reentrancy = 7` error.
3. If the lock does not exist, the guard writes `true` to temporary storage and yields a `ReentrancyGuard` instance. 

## The `Drop` Pattern
We use Rust's `Drop` trait to ensure the lock is always released when the function exits, whether it succeeds or fails. As soon as the `_guard` variable goes out of scope, the `drop` method removes the `REENTRANCY_LOCK` from temporary storage.

## Covered Operations
The reentrancy guard is strictly enforced on all token-interacting operations that handle user funds or mutate critical protocol state:
- `deposit_collateral`
- `withdraw_collateral`
- `borrow_asset`
- `repay_debt`

## Security Assumptions
1. **Checks-Effects-Interactions (CEI)**: While we endeavor to apply the CEI pattern throughout the contract, the `ReentrancyGuard` guarantees safety even if state updates happen after external calls. By barring nested entries, all state transitions act atomically from the caller's perspective.
2. **Temporary Storage Isolation**: Temporary storage in Soroban is strictly isolated to the current contract instance and lifecycle of the transaction. It is discarded identically alongside the transaction's end, making it resilient to panics or out-of-gas errors.
3. **External Contracts**: Any malicious token or bridge attempting `transfer_from()` or `transfer()` manipulation simply trips the guard if it invokes StellarLend operations again. The overarching transaction will fail and State Changes will securely revert.
