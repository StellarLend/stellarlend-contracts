# Reentrancy Audit Notes

This contract uses a `DataKey::FlashActive` flag in instance storage to prevent
reentrancy during flash-loan callbacks.

## What Soroban Allows

Soroban contract calls are synchronous inside one invocation tree. If StellarLend
calls a token contract with `transfer` or `transfer_from`, that token contract
can immediately call StellarLend again before the outer function returns.

That means reentrancy is relevant on Soroban even though all state changes are
committed atomically at the end of the transaction.

## What The Guard Guarantees

- The guard is per StellarLend contract instance.
- The guard lives in **instance storage** via `DataKey::FlashActive`.
- When `flash_loan` is entered, it sets `FlashActive = true` before the
  external `on_flash_loan` callback and clears it after the callback returns.
- All state-mutating operations call `require_no_active_flash_loan()` at entry,
  which panics with `"FlashLoanReentrancy"` if the flag is set.
- On revert (panicking callback), Soroban's atomic transaction rollback restores
  the flag to `false`, so the protocol is never permanently locked.

## What The Guard Does Not Guarantee

- It does not replace authorization checks.
- It does not protect other contracts.
- It does not persist across transactions.
- It does not make external tokens trustworthy.
- It does not remove the need for checks-effects-interactions discipline.

## Protected Paths

All state-mutating user operations are protected:

- `deposit`
- `withdraw`
- `borrow`
- `borrow_against_collateral`
- `repay`
- `repay_against_collateral`
- `liquidate`
- `flash_loan` (prevents nesting)

## External Call Audit

- `deposit`: calls `require_no_active_flash_loan` before any state mutations.
- `withdraw`: calls `require_no_active_flash_loan` before any state mutations.
- `borrow`: calls `require_no_active_flash_loan` after pause/emergency checks.
- `borrow_against_collateral`: same as `borrow`.
- `repay` / `repay_against_collateral`: calls `require_no_active_flash_loan` before auth and debt mutation.
- `liquidate`: calls `require_no_active_flash_loan` after auth and valuation price checks.
- `flash_loan`: calls `require_no_active_flash_loan` to block nested flash loans, then sets `FlashActive = true` before the callback.

## Trust Boundaries

- Admin powers: admin-controlled configuration can pause operations, change
  protocol parameters, and affect whether protected paths are reachable, but
  admin authority does not bypass the lock.
- Guardian / recovery powers: guardian and recovery flows are privileged
  governance surfaces, not part of the reentrancy lock boundary. They must
  still be reviewed independently for authorization safety.
- Token contracts: token contracts are untrusted external dependencies. Every
  token callback path must be assumed adversarial.

## Implementation

The guard is implemented as a single helper function in `src/lib.rs`:

```rust
fn require_no_active_flash_loan(env: &Env) {
    let active: bool = env
        .storage()
        .instance()
        .get(&DataKey::FlashActive)
        .unwrap_or(false);
    if active {
        panic!("FlashLoanReentrancy");
    }
}
```

## Testing

The reentrancy tests cover:

- Every protected operation attempted from inside an `on_flash_loan` callback
  (all must be rejected with `FlashLoanReentrancy`).
- Nested flash loan attempts.
- Verification that `FlashActive` is cleared after a failed callback
  (not stuck permanently).
- Verification that normal operations resume after a blocked reentry attempt.

See `tests/reentrancy_guard_test.rs`.
