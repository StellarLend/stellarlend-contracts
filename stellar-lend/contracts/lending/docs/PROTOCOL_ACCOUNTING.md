# Protocol Accounting

This note records the low-level debt accounting invariants enforced by
`src/debt.rs` and covered by `src/property_invariants_test.rs`.

## Debt Position

Each user debt record is stored as:

```rust
DebtPosition {
    principal: i128,
    last_update: u64,
}
```

`borrow_amount`, `repay_amount`, `settle_accrual`, and `effective_debt` all use
checked arithmetic. Overflow is surfaced as `DebtError::Overflow`.

## Property Invariants

The proptest suite samples bounded principals, elapsed times, rates, and
mutation amounts to avoid intentional overflow noise while still covering broad
accounting behavior:

- `repay_amount` never makes principal negative.
- Full or over-sized repayment clears principal to zero.
- `borrow_amount` increases the settled principal by exactly the borrowed
  amount.
- `effective_debt` is always at least the stored principal for non-negative
  rates.
- Zero principal, zero elapsed time, or zero rate leaves effective debt equal to
  principal.

The suite also includes deterministic unbounded extreme cases proving that
overflowing inputs return `DebtError::Overflow` instead of wrapping.
