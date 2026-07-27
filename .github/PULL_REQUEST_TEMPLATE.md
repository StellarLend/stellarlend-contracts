## Summary

<!-- 1-3 bullet points describing what this PR changes and why -->

## Type of change

- [ ] Bug fix
- [ ] New feature / functionality
- [ ] Refactor (no functional change)
- [ ] Documentation only
- [ ] Contract storage / upgrade change

---

## Contracts Release Checklist

> Full checklist: [docs/release_checklist.md](../docs/release_checklist.md)
> Skip sections that do not apply and note why.

### Functional tests
- [ ] New public functions have success-path and failure-path tests
- [ ] All new error codes are reachable through a test
- [ ] Zero/negative/boundary values tested for numeric inputs
- [ ] `cargo test` passes — no new failures beyond the [known baseline](../docs/release_checklist.md#quick-reference-known-pre-existing-test-failures)

### Invariants
- [ ] Cross-asset view guarantees G-1..G-10 hold (if `cross_asset` touched)
- [ ] Repay semantics preserved: borrow system errors on overpay; cross-asset clamps
- [ ] Interest ceiling division unchanged (`interest ≥ 1` for any `principal > 0 && elapsed > 0`)

### Upgrade safety _(skip if no storage / signature changes)_
- [ ] No storage key renamed or removed without a migration path
- [ ] New persistent entries documented in `docs/storage.md`
- [ ] `initialize` still idempotent (second call → `AlreadyInitialized`)

### Monitoring / events _(skip if no event changes)_
- [ ] New operations emit an event with topic + data
- [ ] No existing topic string renamed silently

### Security notes

**Auth / access control**
- [ ] `require_auth()` called for every entry point that modifies user state
- [ ] No new function bypasses the pause guard without justification

**Arithmetic**
- [ ] No unchecked arithmetic on untrusted values
- [ ] No new division site can produce divide-by-zero

**Reentrancy** _(skip if no cross-contract calls)_
- [ ] State updated before external call (Checks-Effects-Interactions)

**Rounding**
- [ ] Floor for collateral capacity; ceiling for interest owed

### Docs
- [ ] Public functions have a doc comment (error codes, params, return)
- [ ] Relevant docs sections updated (`CROSS_ASSET_RULES.md`, `REPAY_SEMANTICS.md`, `storage.md`)

### CI
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] No secrets or key material committed
