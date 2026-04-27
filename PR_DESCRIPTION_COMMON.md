# Common Crate: Shared Types And Conversion Safety

## Summary

Audits `stellar-lend/contracts/common/src/upgrade.rs`, documents the shared upgrade types exposed by `stellarlend-common`, and hardens proposal-to-status conversion and upgrade-state invariants.

## Changes

- added crate-level docs and clean re-exports in `stellar-lend/contracts/common/src/lib.rs`,
- documented shared upgrade types and manager methods in `stellar-lend/contracts/common/src/upgrade.rs`,
- introduced bounded approver-set validation with `MAX_UPGRADE_APPROVERS`,
- added validated `UpgradeProposal::try_into_status` conversion instead of unchecked status reconstruction,
- added explicit handling for proposal id overflow and corrupted rollback metadata,
- standardized `NotInitialized` errors for missing shared upgrade storage,
- added user-facing security and invariant notes in `stellar-lend/contracts/common/SHARED_TYPES.md`,
- added unit tests covering proposal/status invariants, threshold validation, approval flow, rollback flow, and proposal-id overflow.

## Security Notes

- This common module has no token transfer paths and no user-controlled cross-contract callbacks.
- Authorization is split between admin-only actions and approver-gated execution.
- `UpgradeStatus` is now derived through validated conversion so corrupted approval counts or inconsistent stages fail closed.
- Approver storage is explicitly bounded to reduce storage-growth and governance-surface risk.

## Test Output

Attempted:

```bash
cargo test --manifest-path stellar-lend/Cargo.toml -p stellarlend-common --lib
```

Observed:

```text
The command timed out during toolchain startup in this environment without emitting compiler diagnostics.
```
