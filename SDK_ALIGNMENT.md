# SDK Alignment Security Notes

## Summary

Upgraded `soroban-sdk` and `soroban-token-sdk` from `23.4.1` to `25.3.0` across all
contract crates in the `stellar-lend` workspace. All crates already used
`{ workspace = true }` so only `stellar-lend/Cargo.toml` required updating.

## Trust Boundaries

- **Admin**: sole address authorized to call `init`, `add_approver`, `remove_approver`,
  `upgrade_propose`, and `upgrade_rollback`. Admin is set at initialization and stored
  in persistent storage under `UpgradeKey::UpAdmin`.
- **Approvers**: a bounded set (max 32) of addresses authorized to call `upgrade_approve`
  and `upgrade_execute`. The set cannot shrink below `required_approvals`.
- **Guardian/threshold**: `required_approvals` must be ≥ 1 and ≤ `MAX_UPGRADE_APPROVERS`.
  Removal of approvers is blocked if it would make the threshold unsatisfiable.

## Authorization

Every mutating function calls `caller.require_auth()` before any state change.
All admin-only paths call `assert_admin()` which reads the stored admin and panics
with `NotAuthorized` on mismatch.

## Reentrancy

Soroban's single-threaded, message-passing execution model prevents reentrancy at
the host level. No cross-contract calls are made inside mutating paths.

## Arithmetic

All counters use checked Rust arithmetic (`id + 1` on `u64` is bounds-checked via
`ProposalIdOverflow`). Approval counts are bounded by `MAX_UPGRADE_APPROVERS` (u32).

## SDK v25 Breaking Change Fixed

`soroban-env-host` v25 rejects duplicate auth frames (`Error(Auth, ExistingValue)`)
when the same address calls `require_auth()` more than once inside a single
`as_contract` context. Affected tests in `contracts/common/src/upgrade.rs` were
refactored to use one `as_contract` block per function call.

## Pre-existing Failures (not introduced by this PR)

The following crates had pre-existing test/compile failures unrelated to SDK version:
- `stellarlend-amm`: missing `MockAmm` symbols in test scope
- `stellarlend-lending`: unresolved import `crate::constants`
- `hello-world`: 239 compile errors
- `bridge`: 1 pre-existing test failure in `test_bridge_upgrade_coverage_booster`

## Test Results (this PR)

- `stellarlend-common`: **7/7 passed**
