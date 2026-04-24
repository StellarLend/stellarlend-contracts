# Zero-Amount Semantics

**Reference:** Issue #646
**Branch:** `docs/zero-amount-semantics-tests`
**Test module:** `stellar-lend/contracts/lending/src/zero_amount_semantics_test.rs`

---

## Overview

StellarLend adopts a **reject** policy for zero and negative `amount` inputs
across every public entrypoint that accepts a monetary value. A zero-amount
call is **never** a silent no-op; it always returns a typed `Err`.

**Why reject rather than no-op?**

- Prevents integration bugs where an uninitialised or miscalculated value is
  silently accepted.
- Keeps event logs unambiguous — no zero-amount events in transaction history.
- Fails loudly at the earliest possible point, before any external token
  transfer or storage mutation.

---

## Entrypoint Behaviour Table

### Core user-facing entrypoints

| Entrypoint              | Parameter | Zero / Negative behaviour | Error returned                 |
|-------------------------|-----------|---------------------------|--------------------------------|
| `deposit`               | `amount`  | **Reject**                | `DepositError::InvalidAmount`  |
| `deposit_collateral`    | `amount`  | **Reject**                | `BorrowError::InvalidAmount`   |
| `withdraw`              | `amount`  | **Reject**                | `WithdrawError::InvalidAmount` |
| `borrow`                | `amount`  | **Reject**                | `BorrowError::InvalidAmount`   |
| `repay`                 | `amount`  | **Reject**                | `BorrowError::InvalidAmount`   |
| `liquidate`             | `amount`  | **Reject**                | `BorrowError::InvalidAmount`   |

### Cross-asset entrypoints

| Entrypoint                   | Parameter | Zero / Negative behaviour | Error returned                   |
|------------------------------|-----------|---------------------------|----------------------------------|
| `deposit_collateral_asset`   | `amount`  | **Reject**                | `CrossAssetError::InvalidAmount` |
| `borrow_asset`               | `amount`  | **Reject**                | `CrossAssetError::InvalidAmount` |
| `repay_asset`                | `amount`  | **Reject**                | `CrossAssetError::InvalidAmount` |
| `withdraw_asset`             | `amount`  | **Reject**                | `CrossAssetError::InvalidAmount` |

### Admin / configuration entrypoints

| Entrypoint                      | Parameter | Zero behaviour | Rationale                                             |
|---------------------------------|-----------|----------------|-------------------------------------------------------|
| `credit_insurance_fund`         | `amount`  | **Reject**     | Zero credit is a no-op; likely a caller bug.          |
| `offset_bad_debt`               | `amount`  | **Reject**     | Zero offset wastes a governance transaction.          |
| `set_liquidation_threshold_bps` | `bps`     | **Reject**     | Zero threshold disables liquidation safety entirely.  |
| `set_close_factor_bps`          | `bps`     | **Reject**     | Must be in `1..=10000`; zero is structurally invalid. |
| `set_flash_loan_fee_bps`        | `fee_bps` | **Allow**      | Zero fee is the conventional way to offer free loans. |

### View / query helpers

| Entrypoint                         | Zero input         | Return | Notes                              |
|------------------------------------|--------------------|--------|------------------------------------|
| `get_liquidation_incentive_amount` | `repay_amount = 0` | `0`    | Correct: zero repay → zero bonus.  |
| `get_max_liquidatable_amount`      | (no position)      | `0`    | Correct: no debt → nothing to liquidate. |

---

## Invariants

1. **No state mutation on rejection** — when an entrypoint returns `Err(InvalidAmount)`,
   storage (balances, positions, totals) must be identical to before the call.

2. **Clean `Result::Err` return** — zero-amount rejections surface as typed Rust
   errors, not panics or contract aborts. Callers can handle them without
   catching a host-level trap.

3. **Composability** — a rejected zero-amount call must not leave the contract
   in a state that corrupts subsequent valid operations.

4. **Guard executes first** — the zero/negative check runs before authorisation
   side-effects (`require_auth`), external token transfers, and storage writes.

---

## Security Notes

- **Token transfer paths** — `deposit`, `repay`, and token-based liquidation use
  token `transfer_from`; `withdraw` uses an outbound transfer. The zero-amount
  guard fires before any external token interaction.
- **Authorisation** — `require_auth()` calls are still present on all
  state-changing paths; the amount check does not bypass them.
- **Reentrancy** — the flash-loan reentrancy guard is set *after* the amount
  check, so a zero-amount flash loan is rejected before the guard is toggled.

---

## How to verify

```bash
cd stellar-lend
cargo test -p lending zero_amount -- --nocapture
```

All tests live in:
`stellar-lend/contracts/lending/src/zero_amount_semantics_test.rs`

---

## References

- Issue: [#646 — Document and test ZERO amount semantics across all public entrypoints](https://github.com/StellarLend/stellarlend-contracts/issues/646)
- Prior art: [#385 — Zero-Amount Operation Handling Tests](https://github.com/StellarLend/stellarlend-contracts/issues/385)
