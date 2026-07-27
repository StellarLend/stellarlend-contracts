# Upgrade Authorization and Key Rotation

## Scope

This document describes how upgrade authorization works for the lending contract
(implemented in `stellar-lend/contracts/lending/src/upgrade.rs`) and how to safely rotate upgrade keys.

## Authorization model

- `upgrade_init(admin, current_wasm_hash, required_approvals)` initializes upgrade state.
- `upgrade_propose(caller, new_wasm_hash, new_version)` is `admin` only.
- `upgrade_add_approver(caller, approver)` is `admin` only.
- `upgrade_remove_approver(caller, approver)` is `admin` only.
- `upgrade_approve(caller, proposal_id)` is restricted to the configured approver set.
- `upgrade_execute(caller, proposal_id)` is restricted to the configured approver set.

All mutating authorization paths call `require_auth()` on the provided caller.

## Role separation

- The stored `admin` is the governance authority for upgrade configuration: it can propose upgrades
  and add/remove approvers.
- The approver set is the execution authority for upgrades: only current approvers can approve or
  execute a proposal once it exists.
- Guardian or emergency operators are not part of the upgrade trust boundary and gain no upgrade
  rights from pause or recovery permissions.

If you need to rotate the humans or devices behind the admin role, prefer making `admin` a stable
governance or multisig address and rotating its signers through governance. The lending upgrade
manager does not expose a separate `set_upgrade_admin(...)` entrypoint.

## Key rotation procedure

Safe rotation for an upgrade approver key:

1. Add a replacement key: `upgrade_add_approver(admin, new_key)`.
2. Verify the new key can approve and execute a proposal.
3. Revoke the old key: `upgrade_remove_approver(admin, old_key)`.
4. Confirm old key is rejected for `upgrade_approve` and `upgrade_execute`.

Safe rotation for admin/governance signers:

1. Keep the stored upgrade `admin` address stable where possible.
2. If `admin` is a multisig or governance address, rotate its underlying signers atomically in the
   governance layer first.
3. After governance signer rotation is complete, rotate any dedicated upgrade approver keys using
   the add -> verify -> remove flow above.
4. Do not revoke old governance or approver signers until the replacement set has successfully
   exercised the exact upgrade path it is expected to control.

`upgrade_remove_approver` enforces threshold safety:

- It rejects removals that would leave no approvers.
- It rejects removals that would leave fewer approvers than `required_approvals`.

This prevents accidental permanent lockout during rotation.

## Invalid upgrade attempts covered by tests

- Unauthorized address attempts to add/remove approvers.
- Unauthorized address attempts to approve or execute upgrades.
- Removed approvers attempting to approve or execute after rotation.
- Duplicate approvals from the same key.
- Execute attempts before threshold approval is reached.
- Invalid version proposals (`new_version <= current_version`).
- Unsafe key removal that violates threshold constraints.
- Partial signer updates that have not yet restored full replacement capacity.

Soroban `Address` values are strongly typed, so there is no zero-address sentinel to rotate into.
The practical "invalid address" risk is rotating to an address you do not control operationally,
which must be mitigated by live authorization checks before revoking the old signer.

## Security assumptions

- Admin key custody is out of scope of contract logic and must be handled operationally.
- Approver keys should be distinct from the admin key where possible.
- `required_approvals` should reflect operational risk tolerance (single-key vs multi-key).
- In production, route admin operations through governance/multisig processes to avoid
  single-operator risk.

## Do / Don't

- Do add replacement signers before removing existing ones.
- Do prove the replacement signer can call the real upgrade path before revocation.
- Do keep `required_approvals` aligned with the signer set size at every step.
- Do rotate governance/multisig signers atomically when they control the stored `admin`.
- Don't treat admin transfer and approver rotation as the same operation; they protect different
  trust boundaries.
- Don't do partial signer swaps and assume the new set is live until it has actually approved or
  executed an upgrade.
- Don't remove the last signer that makes the current threshold satisfiable.
- Don't reuse compromised or decommissioned devices as approver keys, even temporarily.

## Threat model and mitigations

| Threat | Mitigation |
|--------|------------|
| Old signer keeps upgrade power after rotation | `upgrade_remove_approver` takes effect immediately for future approve/execute calls; tests verify revoked signers are rejected |
| Rotation bricks upgrade execution | Removal is rejected if it would make `required_approvals` unsatisfiable |
| Partial rotation silently weakens authorization | Threshold does not auto-drop during staged rotations; tests verify `n - 1` approvals remain insufficient |
| Governance signer swap changes upgrade authority unexpectedly | Keep stored `admin` stable and rotate underlying multisig/governance signers atomically |
| Wrong replacement address is staged | Operationally verify the new signer can authenticate the real path before revoking the old one |

## Trust boundaries and operator powers

- Upgrade authority boundary: only `admin` can propose upgrades and manage approvers.
- Execution boundary: only currently configured approvers can execute approved proposals.
- Guardian boundary: guardian operations (pause or emergency flows) are separate from upgrade
  authority and do not grant upgrade proposal, execution, or rollback rights.
- Rotation boundary: removing an approver takes effect immediately for future `upgrade_approve`
  and `upgrade_execute` calls.

## External call and token transfer safety

- Upgrade entrypoints (`upgrade_propose`, `upgrade_approve`, `upgrade_execute`)
  do not perform token transfers.
- Token transfer paths remain confined to lending operations such as deposit, withdraw, repay,
  and liquidation modules.
- Authorization checks (`require_auth()`) are enforced on every mutating upgrade path.
- Upgrade tests should verify both authorization and invalid-status rejection on each external
  entrypoint.

## Failure-path coverage checklist

- Execute rejects unknown proposal ids (`ProposalNotFound`).
- Non-monotonic version proposals are rejected after successful execution
  (`new_version <= current_version`).
- Execution by a removed approver is rejected even if they approved earlier during proposal
  lifecycle.
- Rotation add/remove actions emit audit events (`up_apadd`, `up_aprm`) so signer changes can be
  monitored off-chain.
