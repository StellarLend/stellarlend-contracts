# Contract Initialization Test Suite Documentation

## Overview

This test suite provides comprehensive coverage for the StellarLend lending
contract initialization process.  The key security property is:

> **Every state-mutating entry point must return `LendingError::NotInitialized`
> when called before `LendingContract::initialize`.**

The guard is implemented by `require_initialized(&env)` in `src/lib.rs`, which
checks for the presence of `DataKey::Admin` in instance storage.

---

## Initialization Guard (`src/initialization_guard_test.rs`)

The dedicated guard test module (`initialization_guard_test.rs`) exercises every
protected entry point *before* initialization and verifies the expected error is
returned.  It also tests post-init happy paths and view-function safety.

### Test categories

| Category | Tests | Description |
|---|---|---|
| **initialize: once-only** | 3 | First call succeeds; second call (same or different admin) → `AlreadyInitialized` |
| **deposit / withdraw** | 2 | Return `NotInitialized` before init |
| **borrow / repay** | 4 | `borrow`, `repay`, `borrow_against_collateral`, `repay_against_collateral` all return `NotInitialized` before init |
| **liquidate** | 1 | Returns `NotInitialized` before init |
| **flash_loan / repay_flash_loan** | 2 | Panic with `"NotInitialized"` before init |
| **admin setters** | 14 | All admin-only setters return / panic with `NotInitialized` before init |
| **cross-asset entry points** | 5 | `set_asset_params`, `deposit/borrow/repay/withdraw_asset` return `NotInitialized` before init |
| **view functions** | 9 | Read-only views return safe defaults (0 / `None`) without panicking |
| **post-init happy path** | 1 | After `initialize`, the full deposit → borrow → repay → withdraw cycle succeeds |

Total: **41 tests** in the guard suite.

---

## Covered Entry Points

### State-mutating entry points (all guarded)

| Entry point | Guard added |
|---|---|
| `deposit` | ✅ |
| `withdraw` | ✅ |
| `borrow` | ✅ |
| `repay` | ✅ |
| `borrow_against_collateral` | ✅ |
| `repay_against_collateral` | ✅ |
| `liquidate` | ✅ |
| `flash_loan` | ✅ |
| `repay_flash_loan` | ✅ |
| `set_oracle_pubkey` | ✅ |
| `set_price` | ✅ |
| `set_max_move_bps` | ✅ |
| `set_max_flash_bps` | ✅ |
| `set_price_bounds` | ✅ |
| `propose_admin` | ✅ |
| `accept_admin` | ✅ |
| `set_guardian` | ✅ |
| `set_emergency_state` | ✅ |
| `set_pause` | ✅ |
| `set_min_borrow` | ✅ |
| `set_asset_isolation` | ✅ |
| `set_collateral_asset` | ✅ |
| `set_close_factor_bps` | ✅ |
| `set_liquidation_incentive_bps` | ✅ |
| `set_debt_ceiling` | ✅ |
| `set_flash_fee` | ✅ |
| `fund_insurance` | ✅ |
| `set_insurance_share` | ✅ |
| `credit_insurance_fund` | ✅ |
| `write_off_bad_debt` | ✅ |
| `set_asset_params` | ✅ |
| `deposit_collateral_asset` | ✅ |
| `borrow_asset` | ✅ |
| `repay_asset` | ✅ |
| `withdraw_asset` | ✅ |

### `initialize` itself

`initialize` is the **only** entry point exempt from the guard.  A second call
returns `LendingError::AlreadyInitialized` instead.

---

## Running the Tests

```bash
# Run guard tests only (fast)
cargo test -p stellarlend-lending initialization_guard

# Run the full lending test suite
cargo test -p stellarlend-lending
```

---

## Security Assumptions Verified

1. ✅ **No state mutation before init** — every write path returns `NotInitialized`.
2. ✅ **Double-init prevented** — `AlreadyInitialized` on any repeat call.
3. ✅ **Admin unchanged on failed re-init** — original admin address preserved.
4. ✅ **View functions safe before init** — return `0` / `None` without panicking.
5. ✅ **Post-init operations unaffected** — the guard is a one-instruction `.has()`
   check and adds no observable behaviour after `initialize` succeeds.

---

## Notes about recent contract changes

- `initialize` now returns `Result<(), LendingError>` instead of panicking; clients
  should use `try_initialize` or handle `LendingError::AlreadyInitialized`.
- `require_initialized` is implemented as a `pub(crate)` free function in
  `src/lib.rs` so it can be reused across all entry points without duplication.
- Two new `LendingError` variants were added: `InvalidIsolationCeiling = 7003`
  and `SelfLiquidation = 7004`.  Both had existing references but lacked enum
  declarations; those are now resolved.

---

## References

- Guard implementation: `stellar-lend/contracts/lending/src/lib.rs` — `require_initialized`
- Guard test suite: `stellar-lend/contracts/lending/src/initialization_guard_test.rs`
- Security notes: `stellar-lend/contracts/hello-world/INITIALIZATION_SECURITY_NOTES.md`
