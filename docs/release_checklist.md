# Contracts Release Checklist

Use this checklist for every pull request that modifies `stellar-lend/contracts/`.
Copy the relevant sections into your PR description and check off each item before
requesting a review.

> **Scope**: The canonical deployment target is `contracts/lending`.
> See [ARCHITECTURE.md](../stellar-lend/contracts/ARCHITECTURE.md) for crate ownership boundaries.

---

## 1. Functional Tests

All new behaviour must have a test. Tests must be in the same crate as the code
they cover (`#[cfg(test)]` mod in `src/`).

- [ ] Every new public function has at least one success-path test and one
      failure-path test (`try_*` variant or `assert_eq!(result, Err(Ok(...)))`).
- [ ] All error codes introduced by the PR are reachable through a test.
- [ ] Zero-amount, negative-amount, and boundary values are explicitly tested for
      any function that accepts numeric input.
- [ ] Tests do not use `unwrap()` or `expect()` on `Result`/`Option` values that
      could plausibly fail — use explicit assertions instead.
- [ ] `cargo test` passes locally with no new failures beyond the pre-existing
      baseline (currently: `borrow_health_factor_test::borrow_zero_collateral_rejected`,
      `borrow_health_factor_test::borrow_rejects_when_weighted_collateral_multiplication_overflows`,
      `zero_amount_semantics_test::borrow_zero_returns_invalid_amount`,
      and pause-related failures in `granular_pause_ops_test`, `cross_asset_test`,
      and `liquidate_pause_test`).
- [ ] New test modules are registered in `lib.rs` under `#[cfg(test)]`.

**Relevant docs**: [BORROW_TESTS.md](BORROW_TESTS.md),
[REPAY_SEMANTICS.md](../stellar-lend/docs/REPAY_SEMANTICS.md),
[ZERO_AMOUNT_SEMANTICS.md](ZERO_AMOUNT_SEMANTICS.md)

---

## 2. Invariant Coverage

The following protocol invariants must not be broken and should be covered by at
least one test each if the PR touches the relevant code paths.

### Cross-asset position summary (G-1..G-10)

- [ ] **G-1 Read-only**: `get_cross_position_summary` does not mutate ledger state.
- [ ] **G-2 Idempotent**: calling the view N times returns identical values.
- [ ] **G-3 Collateral accuracy**: `total_collateral_usd` equals the sum of deposits × price.
- [ ] **G-4 Debt accuracy**: `total_debt_usd` equals the sum of outstanding debt × price.
- [ ] **G-5 HF formula**: `health_factor = weighted_collateral × 10_000 / total_debt_usd`
      when debt > 0; sentinel `1_000_000` when debt = 0.
- [ ] **G-6 Monotonicity**: adding collateral never decreases HF; adding debt never increases it.
- [ ] **G-7 User isolation**: one user's operations do not affect another user's summary.
- [ ] **G-8 Order invariance**: depositing assets in any order produces the same totals.
- [ ] **G-9 Conservative rounding**: weighted-collateral uses floor division (never over-counts capacity).
- [ ] **G-10 No view exploitation**: view calls cannot mutate state or bypass access controls.

**Relevant docs**: [CROSS_ASSET_RULES.md — View Guarantees](CROSS_ASSET_RULES.md#view-guarantees)

### Repay semantics

- [ ] Borrow-system `repay` returns `RepayAmountTooHigh` on overpay (no silent clamp).
- [ ] Cross-asset `repay_asset` silently clamps overpay to the outstanding balance.
- [ ] `get_debt_balance()` after a full repay returns 0; `health_factor` returns sentinel.
- [ ] Interest is settled before principal on every borrow-system repay.

**Relevant docs**: [REPAY_SEMANTICS.md](../stellar-lend/docs/REPAY_SEMANTICS.md)

### Numeric safety

- [ ] No unchecked arithmetic on user-supplied values (use checked ops or Rust's
      debug-mode overflow panics confirmed via test).
- [ ] Interest ceiling division is preserved — any change to `calculate_interest`
      must maintain `interest ≥ 1` for `principal > 0 && elapsed > 0`.

**Relevant docs**: [INTEREST_NUMERIC_ASSUMPTIONS.md](../stellar-lend/docs/INTEREST_NUMERIC_ASSUMPTIONS.md)

---

## 3. Upgrade Safety

Required for any PR that changes **storage keys**, **persistent data types**, or
**`#[contractimpl]` function signatures**.

- [ ] No existing storage key has been renamed or removed without a migration path.
- [ ] Any new `Persistent` or `Instance` storage entry is documented in
      [docs/storage.md](storage.md) with its key name, type, and TTL.
- [ ] Existing ledger entries can still be decoded after the upgrade (backward-compatible
      XDR or explicit migration logic added).
- [ ] `initialize` is still idempotent — a second call returns `AlreadyInitialized`
      and leaves state unchanged.
- [ ] If function signatures changed: client-side call patterns in integration tests
      or scripts have been updated.
- [ ] Upgrade proposal/approval/execute flow tested if the WASM hash changes.

**Relevant docs**: [storage.md](storage.md),
[UPGRADE_AUTHORIZATION.md](UPGRADE_AUTHORIZATION.md),
[deployment.md — Mainnet checklist](deployment.md#8-mainnet-checklist)

---

## 4. Monitoring and Event Changes

Required if the PR adds, removes, or modifies **emitted events** or **analytics state**.

- [ ] Every new user-facing operation emits a corresponding event (topic + data).
- [ ] Event topic strings follow the existing naming pattern (e.g., `pause_event`,
      `RepayEvent`); no topic has been silently renamed.
- [ ] Downstream consumers (indexers, monitoring dashboards) are noted in the PR
      description if event schema changed.
- [ ] `get_protocol_report` / `get_user_report` still return complete data after
      the change.
- [ ] If a pause switch was added or removed: `set_pause_switch` tests cover the
      new granularity and the `pause_event` is verified.

---

## 5. Security Notes Template

Include this block in the PR description for any change that touches auth, arithmetic,
oracle reads, admin controls, or pause logic. Delete items that do not apply.

```
### Security notes

**Auth / access control**
- [ ] `require_auth()` is called for every entry point that modifies user state.
- [ ] Admin-only functions check caller against stored admin address.
- [ ] No new function bypasses the pause guard without explicit justification.

**Arithmetic**
- [ ] All i128 arithmetic on untrusted values uses checked ops or is range-bounded
      by prior validation.
- [ ] No new division site can produce a divide-by-zero (guarded by a prior `== 0`
      check or provably non-zero invariant).

**Oracle / price feed**
- [ ] New price reads go through the staleness check in `oracle.rs`.
- [ ] Price manipulation cannot cause a state transition that benefits the caller
      at the protocol's expense.

**Reentrancy**
- [ ] Any new cross-contract call follows the Checks-Effects-Interactions pattern
      (state updated before the external call, not after).
- [ ] If a token transfer is involved, verify it cannot re-enter through a callback.

**Dust / rounding**
- [ ] Rounding direction is conservative (floor for collateral capacity,
      ceiling for interest owed) — protocol never under-collects or over-extends.

**Recovery mode**
- [ ] If adding a new pause type: confirm whether it should be exempt during
      `EmergencyState::Recovery` (repay is; new borrows are not).
```

**Relevant docs**: [SECURITY_ASSUMPTIONS.md](SECURITY_ASSUMPTIONS.md),
[REENTRANCY_GUARANTEES.md](../stellar-lend/docs/REENTRANCY_GUARANTEES.md)

---

## 6. Documentation Updates

- [ ] Public functions added or changed have a doc comment explaining parameters,
      return value, and error codes (one short line per item is enough).
- [ ] If a new error code was introduced: it appears in the relevant docs section
      (e.g., `REPAY_SEMANTICS.md` error table, `CROSS_ASSET_RULES.md` invariants).
- [ ] `docs/storage.md` is up to date if storage layout changed.
- [ ] `CROSS_ASSET_RULES.md` View Guarantees section updated if `get_cross_position_summary`
      semantics changed.
- [ ] This checklist is complete and included in the PR description.

---

## 7. CI and Commit Hygiene

- [ ] `cargo fmt --check` passes (no formatting diffs).
- [ ] `cargo clippy -- -D warnings` passes with no new warnings.
- [ ] `cargo test` output reviewed — no new `FAILED` entries beyond the known baseline.
- [ ] `cargo audit` shows no new critical advisories (`cargo install cargo-audit` if needed).
- [ ] Commit messages are imperative mood, ≤ 72 chars on the subject line.
- [ ] No secrets, key material, or environment-specific paths committed.
- [ ] Branch is rebased on (or merged from) `main` before opening the PR.

---

## Quick-reference: known pre-existing test failures

These failures exist on `main` and are **not** a blocker for new PRs. Do not mask
or skip them; investigate separately.

| Test                                                                                          | Likely cause                             |
| --------------------------------------------------------------------------------------------- | ---------------------------------------- |
| `borrow_health_factor_test::borrow_zero_collateral_rejected`                                  | Borrow guard logic mismatch              |
| `borrow_health_factor_test::borrow_rejects_when_weighted_collateral_multiplication_overflows` | Extreme-value edge case in borrow module |
| `zero_amount_semantics_test::borrow_zero_returns_invalid_amount`                              | Math-safety / zero-amount validation gap |
| `granular_pause_ops_test::unauthorized_caller_rejected`                                       | Pause authorization rejection            |
| `cross_asset_test::test_deposit_collateral_asset_paused`                                      | Cross-asset pause behavior               |
| `cross_asset_test::test_borrow_asset_paused`                                                  | Cross-asset pause behavior               |
| `liquidate_pause_test::liquidate_blocked_when_global_pause`                                   | Global pause interaction                 |
