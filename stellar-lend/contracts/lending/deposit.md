# Deposit Collateral

## Overview

`deposit` lets users deposit assets as collateral into the StellarLend protocol. It enforces a protocol-wide cap on total deposits and maintains the `TotalDeposits` accounting invariant.

## Function Signature

```rust
pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError>
```

## Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Contract environment |
| `user` | `Address` | Depositor (must authorize via `require_auth`) |
| `amount` | `i128` | Deposit amount, must be > 0 |

## Returns

`Ok(i128)` — user's updated collateral balance  
`Err(LendingError)` — on any validation or cap failure

## Error Types

| Error | Code | Description |
|-------|------|-------------|
| `InvalidAmount` | 1004 | `amount <= 0` |
| `DepositCapExceeded` | 2002 | `TotalDeposits + amount > DepositCap` |
| `Overflow` | 2003 | Arithmetic overflow in checked_add |

## Deposit Cap

The protocol keeps a single `TotalDeposits: i128` counter in persistent storage. On every deposit:

```
new_total = TotalDeposits + amount
if new_total > deposit_cap → Err(DepositCapExceeded)
```

The cap defaults to `DEFAULT_DEPOSIT_CAP = 1_000_000_000_000` and can be overridden via `DataKey::DepositCap` in persistent storage.

The check is **strict** (`>`): depositing exactly up to the cap is allowed; depositing 1 unit over is rejected.

## TotalDeposits Invariant

```
TotalDeposits = Σ collateral(user) for all users
```

Both `deposit` and `withdraw` maintain this invariant atomically:

- `deposit`: `TotalDeposits += amount` (after cap check)
- `withdraw`: `TotalDeposits -= amount` (after balance check)

## Storage Keys

| Key | Durability | Description |
|-----|-----------|-------------|
| `DataKey::Collateral(Address)` | Persistent | Per-user collateral balance |
| `DataKey::TotalDeposits` | Persistent | Protocol-wide total deposits |
| `DataKey::DepositCap` | Persistent | Maximum allowed total deposits |

## Deposit Cap Test Coverage

Tests live in `src/deposit_accounting_test.rs` and cover the following scenarios:

| Test | Scenario |
|------|----------|
| `test_deposit_exactly_at_cap_is_allowed` | Cap boundary: `total + amount == cap` is allowed |
| `test_deposit_one_over_cap_is_rejected` | Cap boundary: `total + amount == cap + 1` is rejected |
| `test_deposit_exactly_one_over_cap_after_partial_fill_is_rejected` | Partial fill to 999/1000 then +2 rejected |
| `test_two_users_deposits_sum_to_cap` | Two users fill cap; both blocked on next deposit |
| `test_withdraw_restores_headroom_for_new_deposit` | Withdraw frees room; new deposit fits again |
| `test_withdraw_to_zero_resets_total_deposits` | Full withdrawal sets TotalDeposits back to 0 |
| `test_withdraw_more_than_deposited_is_rejected` | Over-withdraw rejected; TotalDeposits unchanged |
| `test_total_deposits_conserved_across_interleaved_ops` | Three users interleaved deposit/withdraw cycle ends at 0 |
| `test_default_cap_allows_large_deposit` | Single deposit of exactly DEFAULT_DEPOSIT_CAP succeeds |
| `test_default_cap_blocks_deposit_exceeding_cap` | DEFAULT_DEPOSIT_CAP + 1 rejected |

### Key Invariants Verified

1. **Strict cap check**: `new_total > cap` rejects; `new_total == cap` allows.
2. **No partial write**: a rejected deposit leaves `TotalDeposits` unchanged.
3. **Withdraw headroom**: `withdraw(amount)` reduces `TotalDeposits` by exactly `amount`, re-opening deposit capacity.
4. **Conservation**: after a full deposit/withdraw round-trip across N users, `TotalDeposits == 0`.

## Security Considerations

1. **Authorization**: `user.require_auth()` prevents unauthorized deposits.
2. **Overflow protection**: `checked_add` / `checked_sub` used throughout.
3. **Atomic accounting**: cap check and balance update happen in the same contract invocation; no partial state.
4. **Reentrancy guard**: `FlashActive` flag blocks deposits during active flash loans.
5. **Emergency state**: `check_emergency_status` blocks deposits during `Shutdown` or `Recovery`.
