# Reentrancy Guarantees

## Overview

StellarLend's lending contract uses a `DataKey::FlashActive` boolean flag in
instance storage to provide reentrancy protection during flash-loan callbacks.
This prevents malicious cross-contract callbacks from manipulating protocol
state during the window between the external `on_flash_loan` call and the
post-callback validation.

## Mechanism

The guard uses a simple boolean flag in Soroban instance storage:

1. **Before the callback**: `flash_loan` sets `FlashActive = true`.
2. **All state-mutating operations** call `require_no_active_flash_loan()` at
   entry, which reads the flag and panics with `"FlashLoanReentrancy"` if set.
3. **After the callback**: `flash_loan` sets `FlashActive = false`.
4. **On revert**: If the callback panics, Soroban's atomic transaction rollback
   restores all storage mutations — including the `FlashActive` flag — to their
   pre-transaction state (`false`). This means the protocol is never permanently
   locked by a failed flash loan.

## Covered Operations

The `require_no_active_flash_loan` guard is enforced on all state-mutating
operations, and `liquidate` additionally wraps its body in the reentrancy lock
so the same transaction cannot re-enter the entrypoint from a flash-loan
callback or another internal call path:

- `deposit`
- `withdraw`
- `borrow`
- `borrow_against_collateral`
- `repay`
- `repay_against_collateral`
- `liquidate`
- `flash_loan` (prevents nesting)

## Flash Loan Specifics

For `flash_loan`, the guard provides two layers of protection:

1. **Self-nesting prevention**: `flash_loan` itself calls
   `require_no_active_flash_loan()` before setting the flag, blocking nested
   flash loans from inside a callback.
2. **Post-callback validation**: After the callback returns and before clearing
   `FlashActive`, the protocol verifies the treasury balance has increased by at
   least the required fee. This prevents under-repayment even if the callback
   completes without panicking.

## Security Assumptions

1. **Soroban Atomicity**: All storage changes within a transaction are committed
   atomically. A panicking callback rolls back *all* mutations made during that
   transaction, including `FlashActive = true`. This means no cleanup code is
   needed for the failure path.
2. **Instance Storage**: The flag lives in instance storage, scoped to the
   contract instance. It is not shared with other contracts and cannot be
   tampered with by external callers.
3. **Checks-Effects-Interactions (CEI)**: While the guard provides safety even
   when CEI is not perfectly followed, the contract's state-mutating functions
   generally apply CEI discipline as well.

## Testing

Integration tests in `tests/reentrancy_guard_test.rs` register malicious
receiver contracts that attempt to call back into every protected operation
during the `on_flash_loan` callback. Each test verifies:

- The outer `try_flash_loan` returns an error.
- Treasury balance is unchanged (rolled back).
- `FlashActive` is not stuck `true`.
- Subsequent operations are not blocked.
