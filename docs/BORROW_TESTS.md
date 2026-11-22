# Borrow Function Test Suite — ⚠️ ASPIRATIONAL / NOT YET IMPLEMENTED

> **Status**: This document describes a planned test suite that has **not been
> implemented**. None of the test files, module paths, or coverage figures below
> exist in the repository. Do not treat them as ground truth.
>
> Tracked by: issue #1754

---

## Why this document exists

This file was written speculatively to describe what a complete borrow test suite
should look like. It was committed before the implementation existed, making it a
misleading "documentation debt" artifact.

---

## Current state of `borrow` in `hello-world`

- `stellar-lend/contracts/hello-world/src/borrow.rs` — **does not exist**. The
  module is not declared in `lib.rs`.
- The bare-bones `borrow` entry-point used by `hello-world`'s basic test suite is
  implemented directly in `lib.rs` and is not the full `borrow_asset` function
  described below.
- There is **no** `src/tests/borrow_test.rs` file.
- There is **no** `tests::borrow_test` module.
- Test coverage for borrow logic: **0%** (no dedicated tests exist).

---

## Planned implementation (not yet started)

When `borrow.rs` and its test suite are eventually implemented, the following
specification should be used as the acceptance target.

### Planned test file

```
stellar-lend/contracts/hello-world/src/borrow.rs          ← implementation
stellar-lend/contracts/hello-world/src/borrow_test.rs     ← tests (top-level src file)
```

> Note: the original doc referenced `src/tests/borrow_test.rs`. The correct
> convention for this crate is a `_test.rs` file alongside the implementation,
> not a `tests/` subdirectory.

### Entry-point to implement

`borrow_asset(env, user, asset, amount) -> i128`

The function is expected to:
1. Validate `amount > 0` and that `asset` is not the contract itself.
2. Check that `asset` is enabled for borrowing.
3. Compute the user's maximum borrowable amount from their collateral and the
   configured collateral factor.
4. Enforce `MIN_COLLATERAL_RATIO_BPS` (15 000, i.e. 150 %).
5. Enforce the per-asset or protocol debt ceiling.
6. Accrue interest on existing debt before adding new principal.
7. Transfer the borrowed asset to the user.
8. Emit a `BorrowEvent` and update analytics.
9. Respect `pause_borrow` and the global pause switch.

### Planned test categories (target: 40+ tests, 95 %+ coverage)

| # | Category | Example tests |
|---|----------|---------------|
| 1 | Test helpers | `create_test_env`, `get_user_position`, `advance_ledger_time` |
| 2 | Successful borrows | basic, at-max, sequential, with existing debt, after repay |
| 3 | Validation errors | zero amount, invalid asset, contract-as-asset |
| 4 | Collateral errors | no collateral, below-ratio, max-exceeded |
| 5 | Interest accrual | time-based accrual before new borrow |
| 6 | Pause | paused/unpaused/no-pause-map/pause-removed |
| 7 | Events | BorrowEvent, PositionUpdatedEvent, AnalyticsUpdatedEvent |
| 8 | Edge cases | exact-max, one-below-max, one-above-max, very-small |
| 9 | Security | zero collateral factor, state consistency, overflow protection |

### Key formula reference

```
max_borrow = (collateral × collateral_factor × 10 000) / MIN_COLLATERAL_RATIO_BPS
```

- `collateral_factor` — basis points (10 000 = 100 %)
- `MIN_COLLATERAL_RATIO_BPS` — 15 000 (150 %)

Interest accrual uses the kink-model rate from `interest_rate.rs`:

```
interest = principal × rate_bps × elapsed_seconds / (10 000 × SECONDS_PER_YEAR)
```

---

## Related files (all exist)

- `stellar-lend/contracts/hello-world/src/interest_rate.rs` — borrow rate model
- `stellar-lend/contracts/hello-world/src/risk_management.rs` — collateral factor config
- `stellar-lend/contracts/hello-world/src/withdraw.rs` — withdraw implementation (reference)
- `stellar-lend/contracts/hello-world/src/repay.rs` — repay implementation (reference)
