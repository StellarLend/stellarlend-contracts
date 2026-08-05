# Flash-Swap Caller Binding

## Overview

This document describes the security fix that binds a flash swap to its
initiator, preventing third-party interference during the in-flight window.

## Problem

Before this fix, the `repay_flash_swap` entry point could be called by
**any** address.  A third party observing an in-flight flash swap could
call `repay_flash_swap` (or otherwise interfere) within the same Soroban
transaction.

### Attack scenario

```text
1. Alice calls AMM.flash_swap_a_for_b(200, 30)
   - reserve_b debited by 200
   - FlashActive = true

2. Bob (malicious) calls AMM.repay_flash_swap(manipulated_amount_in)
   - If amount_in is chosen to satisfy verify-k, Bob completes Alice's swap
   - Alice never receives the debited tokens
   - Bob gains control of the repayment slot
```

In a multi-operation Soroban transaction, Bob's operation could execute
between Alice's `flash_swap_a_for_b` and her intended
`repay_flash_swap`, front-running or griefing the swap.

## Solution

Persist the **initiator address** alongside the `FlashActive` flag:

1. `flash_swap_a_for_b` records `env.current_contract_address()` as the
   initiator.
2. `repay_flash_swap` checks that `env.current_contract_address()` matches
   the recorded initiator **and** requires explicit authorization from that
   address.
3. On successful repay, the initiator address is removed from storage.

## Storage layout

| Key                        | Type    | Purpose                                         |
|----------------------------|---------|-------------------------------------------------|
| `("pool", "flash_active")` | `bool`  | Reentrancy guard (unchanged)                    |
| `("pool", "flash_k_before")`| `i128` | Pre-debit k invariant (unchanged)               |
| `("pool", "flash_initiator")`| `Address`| **NEW** — address that called `flash_swap_a_for_b` |

## API changes

### New error variant

```rust
#[contracterror]
pub enum AmmPoolError {
    // ... existing variants ...
    /// Caller is not the flash-swap initiator
    UnauthorizedCaller = 7,
}
```

### `flash_swap_a_for_b` (modified)

No signature change.  Records the caller's address as the initiator
after setting `FlashActive = true`.

```text
// After setting FlashActive = true:
env.storage()
    .instance()
    .set(&KEY_FLASH_INITIATOR, &env.current_contract_address());
```

### `repay_flash_swap` (modified)

No signature change.  Adds two checks before the verify-k logic:

```text
1. Load initiator from storage.
2. If env.current_contract_address() != initiator → UnauthorizedCaller.
3. initiator.require_auth().
```

After a successful repay the initiator is removed from storage:

```text
env.storage().instance().remove(&KEY_FLASH_INITIATOR);
```

## Worked example

### Normal single-caller flow (unchanged)

```text
Alice calls flash_swap_a_for_b(200, 30, params):
  - reserve_b debited by 200
  - initiator = Alice
  - FlashActive = true

Alice calls repay_flash_swap(exact_amount_in):
  - current_contract_address() == Alice == initiator ✓
  - Alice.require_auth() ✓
  - verify-k: (ra + amount_in) * rb >= k_before ✓
  - reserve_a credited
  - FlashActive = false
  - initiator cleared
```

### Interloper attempt (blocked)

```text
Alice calls flash_swap_a_for_b(200, 30, params):
  - initiator = Alice
  - FlashActive = true

Bob calls repay_flash_swap(amount_in):
  - current_contract_address() == Bob != Alice → UnauthorizedCaller ✗
  - Transaction reverts; Alice's flash swap is rolled back atomically.
```

### Proxy-initiated flow

```text
Proxy contract calls flash_swap_a_for_b(200, 30, params):
  - current_contract_address() = Proxy (not the human caller)
  - initiator = Proxy

Human calls repay_flash_swap(amount_in):
  - current_contract_address() = Human != Proxy → UnauthorizedCaller ✗

Proxy calls repay_flash_swap(amount_in):
  - current_contract_address() = Proxy == Proxy ✓
  - Proxy.require_auth() ✓
  - verify-k ✓
  - Swap completes; initiator cleared
```

## Edge cases

| Case                                    | Behavior                              |
|-----------------------------------------|---------------------------------------|
| `repay` by non-initiator                | `UnauthorizedCaller` error            |
| `repay` without prior `flash_swap`      | `InvariantViolation` (no active swap) |
| Nested `flash_swap` during active swap  | `ReentrantFlashSwap` (unchanged)      |
| `repay` by initiator with wrong amount  | `InvariantViolation` (k decreased)    |
| Initiator cleared after successful repay| Next `flash_swap` binds fresh caller  |
| Rolled-back failed repay                | Initiator cleared (entire TX reverted)|

## Backward compatibility

- **No breaking changes** to the public API signature.
- Existing callers that initiate and repay in the same transaction are
  unaffected (the initiator check passes because `current_contract_address`
  matches).
- The new `UnauthorizedCaller` error variant is additive.

## Test coverage

All tests are in `flash_swap_caller_binding_test.rs`:

| Test                                          | Lines covered                          |
|-----------------------------------------------|----------------------------------------|
| `test_initiator_can_repay`                    | Happy path: initiator repays           |
| `test_non_initiator_rejected`                 | UnauthorizedCaller path                |
| `test_initiator_cleared_on_success`           | Storage cleanup after repay            |
| `test_reentrancy_blocks_flash`                | ReentrantFlashSwap still enforced      |
| `test_k_invariant_preserved`                  | Verify-k still enforced on under-repay |
| `test_initiator_via_proxy_matches_proxy`      | Cross-contract initiator binding       |
| `test_consecutive_swaps_same_initiator`       | Multiple sequential swaps              |
