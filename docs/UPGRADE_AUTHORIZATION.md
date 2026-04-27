# Upgrade Authorization and Key Rotation

## Scope

This document describes how upgrade authorization works for contracts using
`stellarlend_common::upgrade::UpgradeManager` and how to safely rotate upgrade keys.

## Authorization model

- `upgrade_init(admin, current_wasm_hash, required_approvals)` initializes upgrade state.
- `upgrade_propose(caller, new_wasm_hash, new_version)` is `admin` only.
- `upgrade_add_approver(caller, approver)` is `admin` only.
- `upgrade_remove_approver(caller, approver)` is `admin` only.
- `upgrade_approve(caller, proposal_id)` is restricted to the configured approver set.
- `upgrade_execute(caller, proposal_id)` is restricted to the configured approver set.
- `upgrade_rollback(caller, proposal_id)` is `admin` only.

All mutating authorization paths call `require_auth()` on the provided caller.

## Key rotation procedure

Safe rotation for an approver key:

1. Add a replacement key: `upgrade_add_approver(admin, new_key)`.
2. Verify the new key can approve and execute a proposal.
3. Revoke the old key: `upgrade_remove_approver(admin, old_key)`.
4. Confirm old key is rejected for `upgrade_approve` and `upgrade_execute`.

`upgrade_remove_approver` enforces threshold safety:

- It rejects removals that would leave no approvers.
- It rejects removals that would leave fewer approvers than `required_approvals`.

This prevents accidental permanent lockout during rotation.

## Invalid upgrade attempts covered by tests

- Unauthorized address attempts to add/remove approvers.
- Unauthorized address attempts to approve or execute upgrades.
- Duplicate approvals from the same key.
- Execute attempts before threshold approval is reached.
- Invalid version proposals (`new_version <= current_version`).
- Unsafe key removal that violates threshold constraints.

## Security assumptions

- Admin key custody is out of scope of contract logic and must be handled operationally.
- Approver keys should be distinct from the admin key where possible.
- `required_approvals` should reflect operational risk tolerance (single-key vs multi-key).
- In production, route admin operations through governance/multisig processes to avoid
  single-operator risk.

## Trust boundaries and operator powers

- Upgrade authority boundary: only `admin` can propose upgrades, manage approvers, and roll back
  executed upgrades.
- Execution boundary: only currently configured approvers can execute approved proposals.
- Guardian boundary: guardian operations (pause or emergency flows) are separate from upgrade
  authority and do not grant upgrade proposal, execution, or rollback rights.
- Rotation boundary: removing an approver takes effect immediately for future `upgrade_approve`
  and `upgrade_execute` calls.

## External call and token transfer safety

- Upgrade entrypoints (`upgrade_propose`, `upgrade_approve`, `upgrade_execute`,
  `upgrade_rollback`) do not perform token transfers.
- Token transfer paths remain confined to lending operations such as deposit, withdraw, repay,
  and liquidation modules.
- Authorization checks (`require_auth()`) are enforced on every mutating upgrade path.
- Upgrade tests should verify both authorization and invalid-status rejection on each external
  entrypoint.

## Rollback and failure-path coverage checklist

- Rollback rejects proposals that were never executed (`InvalidStatus`).
- Execute and rollback reject unknown proposal ids (`ProposalNotFound`).
- Non-monotonic version proposals are rejected after successful execution
  (`new_version <= current_version`).
- Execution by a removed approver is rejected even if they approved earlier during proposal
  lifecycle.
