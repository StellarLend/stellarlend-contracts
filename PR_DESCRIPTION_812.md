## Summary

- Integrates banker's-rounded interest accrual into the live lending debt path in `stellar-lend/contracts/lending`, so debt now accrues over time instead of staying flat.
- Adds modular debt accrual/state logic (`DebtPosition`, accrual settlement helpers) and wires accrual settlement into `borrow` and `repay` using `last_update`.
- Ports and extends drift/rounding regression coverage and updates docs to mark `stellar-lend/contracts/lending` as canonical (with `contracts/lending/scr` documented as legacy reference).

Closes #812

## Type of change

- [x] Bug fix
- [x] New feature / functionality
- [ ] Refactor (no functional change)
- [ ] Documentation only
- [x] Contract storage / upgrade change

---

## Contracts Release Checklist

> Full checklist: [docs/release_checklist.md](../docs/release_checklist.md)  
> Skip sections that do not apply and note why.

### Functional tests
- [x] New public functions have success-path and failure-path tests
- [x] All new error codes are reachable through a test
- [x] Zero/negative/boundary values tested for numeric inputs
- [x] `cargo test` passes — no new failures beyond the [known baseline](../docs/release_checklist.md#quick-reference-known-pre-existing-test-failures)

### Invariants
- [ ] Cross-asset view guarantees G-1..G-10 hold (if `cross_asset` touched)  
      Not applicable: `cross_asset` was not touched.
- [ ] Repay semantics preserved: borrow system errors on overpay; cross-asset clamps  
      Partially applicable: live lending repay ordering is covered; cross-asset not touched.
- [x] Interest ceiling division unchanged (`interest ≥ 1` for any `principal > 0 && elapsed > 0`)

### Upgrade safety _(skip if no storage / signature changes)_
- [x] No storage key renamed or removed without a migration path
- [ ] New persistent entries documented in `docs/storage.md`
- [ ] `initialize` still idempotent (second call → `AlreadyInitialized`)

### Monitoring / events _(skip if no event changes)_
- [ ] New operations emit an event with topic + data  
      Not applicable: no event changes in this PR.
- [x] No existing topic string renamed silently

### Security notes

**Auth / access control**
- [x] `require_auth()` called for every entry point that modifies user state
- [x] No new function bypasses the pause guard without justification

**Arithmetic**
- [x] No unchecked arithmetic on untrusted values
- [x] No new division site can produce divide-by-zero

**Reentrancy** _(skip if no cross-contract calls)_
- [x] State updated before external call (Checks-Effects-Interactions)  
      No new cross-contract calls were introduced.

**Rounding**
- [x] Floor for collateral capacity; ceiling for interest owed  
      Interest accrual uses banker's rounding by design to reduce long-horizon bias; collateral-capacity logic was not modified.

### Docs
- [ ] Public functions have a doc comment (error codes, params, return)
- [x] Relevant docs sections updated (`CROSS_ASSET_RULES.md`, `REPAY_SEMANTICS.md`, `storage.md`)  
      `borrow.md` was updated; the listed docs were not changed in this PR.

### CI
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [x] No secrets or key material committed
