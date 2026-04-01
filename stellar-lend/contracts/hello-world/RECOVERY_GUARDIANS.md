# Recovery Guardians Hardening

This note documents the guardian-based social recovery flow exposed by `stellar-lend/contracts/hello-world/src/recovery.rs`.

## Trust Boundaries

- Multisig admins control guardian membership and guardian threshold.
- Guardians can only start or approve recovery for admin rotation.
- Recovery replaces one multisig admin with another after the guardian threshold is met.
- No user funds or protocol reserves move through this flow.

## Security Invariants

- Guardian set updates are rejected while a non-expired recovery is active.
- `set_guardians` rejects empty guardian sets, duplicates, zero threshold, and threshold values above guardian count.
- Recovery start, approval, and execution all validate that:
  - `old_admin` is still a current multisig admin.
  - `new_admin` is not already a multisig admin.
  - `old_admin != new_admin`.
- Expired or invalidated recovery requests clear both the request and approval state.
- Execution counts only unique approvals from addresses that are still guardians.
- Removing a guardian cannot leave the system with an empty guardian set.
- Removing a guardian clamps the stored threshold downward when necessary.

## Authorization

- `set_guardians`, `add_guardian`, `remove_guardian`, and `set_guardian_threshold` require a current multisig admin.
- `start_recovery` and `approve_recovery` require guardian authorization.
- `execute_recovery` requires caller authorization, but recovery still cannot complete without guardian quorum.

## Reentrancy

The recovery path does not perform cross-contract calls or token transfers. State mutations are local to contract storage, so there is no direct reentrancy surface in this module.

## Token Transfer Flows

There are no token transfers in guardian setup, approval, or recovery execution. Admin rotation only rewrites the stored multisig admin set.
