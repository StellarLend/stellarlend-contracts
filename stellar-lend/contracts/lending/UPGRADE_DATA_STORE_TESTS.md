# Upgrade and Data Store Test Scenarios

This document summarizes the comprehensive test coverage added for issue #307 in the lending contract.

## Files

- `src/upgrade_test.rs`
- `src/data_store_test.rs`
- `src/lib.rs` (module wiring for upgrade/data store tests)

## Upgrade Scenarios Covered

### Initialization and config

- Initializes with admin, current hash, version `0`, and required approvals.
- Rejects `required_approvals == 0`.
- Rejects double initialization.
- Returns `false` for `is_approver` before initialization.

### Approver management

- Admin can add approvers.
- Non-admin cannot add approvers.
- `add_approver` is idempotent.

### Propose / approve / execute

- Proposal creation returns incremental ID and correct status view.
- Auto-approval when threshold is `1`.
- Non-admin cannot propose.
- Rejects proposal version `<= current_version`.
- Only approvers can approve.
- Rejects duplicate approval by same address.
- Rejects approval of missing proposal.
- Execute requires approver permissions.
- Execute fails if proposal lacks threshold approvals.
- Execute updates current wasm hash and version.
- Execute cannot be repeated for executed/rolled-back proposal.

### Rollback and status

- Rollback requires admin.
- Rollback only allowed for executed proposals.
- Rollback restores previous hash/version and marks proposal rolled back.
- Rollback cannot be repeated.
- `upgrade_status` fails for unknown proposal IDs.

## Data Store Scenarios Covered

### `data_save` / `data_load`

- Admin and granted writer can save values.
- Unauthorized users cannot save.
- Entry count increments only on new keys.
- Overwrite keeps count stable and updates value.
- Empty values accepted.
- Key/value max boundary accepted; above boundary rejected.
- Load returns latest value and errors on missing key.
- `data_load` is public (no auth requirement).

### `data_backup` / `data_restore`

- Admin/writer can backup; strangers cannot.
- Empty store backup works.
- Backup name boundary enforced.
- Reusing backup name overwrites prior snapshot.
- Restore replaces live set with snapshot exactly.
- Restore from empty snapshot clears store.
- Restore fails for missing backup.
- Only admin can restore.
- Backup remains reusable after restore (idempotent restore behavior).

### `data_migrate_bump_version`

- Admin can bump version forward (including skipping versions).
- Rejects same/lower versions.
- Non-admin/writer cannot migrate.
- Memo accepted and oversized memo rejected.
- Schema version persistence validated across restore workflow.

### Writer management and integration

- `grant_writer` and `revoke_writer` access control.
- Revoked writer loses write access.
- Revoke non-existent writer is a no-op.
- Multiple writers operate independently.
- Full lifecycle scenario tested:
  save -> backup -> migrate -> modify -> restore.

## Security Notes Validated by Tests

- Privileged upgrade actions are separated by role:
  admin-only (`init`, `add_approver`, `upgrade_propose`, `upgrade_rollback`) and approver-gated (`upgrade_approve`, `upgrade_execute`).
- Upgrade execution is threshold-gated and non-repeatable.
- Rollback can only target executed proposals and reverts to captured prior state.
- Data mutating operations are role-gated (admin/writer), while restore/migration remain admin-only.
- Data size and naming bounds are enforced to reduce storage abuse risk.
- Restore semantics are explicit and destructive only under admin control.

## Test Output (local)

Command:

```bash
cargo test -p stellarlend-lending
```

Result:

- `147 passed`
- `0 failed`
- `0 ignored`
- `0 measured`

