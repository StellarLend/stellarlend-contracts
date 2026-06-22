# Upgrade Authorization and Key Rotation

## Scope

This document describes how upgrade authorization works for the lending contract's
native timelocked WASM upgrade flow.

## Authorization model

- `upgrade_init(approvers, threshold)` is `admin` only and stores the approver set.
- `upgrade_propose(new_wasm_hash)` is `admin` only. It records the WASM hash, ETA
  ledger, expiry ledger, and the admin's approval when the admin is in the approver set.
- `upgrade_approve(approver, proposal_id)` is restricted to the configured approver set.
- `upgrade_execute(executor, proposal_id)` is restricted to the configured approver set.
  It calls `env.deployer().update_current_contract_wasm(new_wasm_hash)` only after
  the timelock has elapsed and the approval threshold is met.

All mutating authorization paths call `require_auth()` on the provided caller.

## Role separation

- The stored `admin` is the governance authority for upgrade configuration: it initializes
  approvers and proposes upgrades.
- The approver set is the execution authority for upgrades: only current approvers can approve or
  execute a proposal once it exists.
- Guardian or emergency operators are not part of the upgrade trust boundary and gain no upgrade
  rights from pause or recovery permissions.

If you need to rotate the humans or devices behind the admin role, prefer making `admin` a stable
governance or multisig address and rotating its signers through governance. The lending upgrade
flow does not expose a separate `set_upgrade_admin(...)` entrypoint.

## Key rotation procedure

Safe rotation for an upgrade approver key:

1. Prepare the replacement approver set off-chain.
2. Use the contract admin to call `upgrade_init(replacement_approvers, replacement_threshold)`.
3. Verify the new key can approve and execute a dry-run/testnet proposal.
4. Confirm old keys are rejected for `upgrade_approve` and `upgrade_execute` on new proposals.

Safe rotation for admin/governance signers:

1. Keep the stored upgrade `admin` address stable where possible.
2. If `admin` is a multisig or governance address, rotate its underlying signers atomically in the
   governance layer first.
3. After governance signer rotation is complete, rotate any dedicated upgrade approver keys using
   the add -> verify -> remove flow above.
4. Do not revoke old governance or approver signers until the replacement set has successfully
   exercised the exact upgrade path it is expected to control.

`upgrade_init` enforces threshold safety:

- It rejects an empty approver set.
- It rejects a zero threshold.
- It rejects a threshold greater than the approver count.

This prevents accidental permanent lockout during rotation.

## Invalid upgrade attempts covered by tests

- Unauthorized address attempts to approve or execute upgrades.
- Non-approvers attempting to approve or execute upgrades.
- Duplicate approvals from the same key.
- Execute attempts before threshold approval is reached.
- Execute attempts before the timelock has elapsed.
- Expired proposals rejecting approval and execution.

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
| Old signer keeps upgrade power after rotation | Reinitialize the approver set and verify old signers are no longer included before proposing production upgrades |
| Rotation bricks upgrade execution | `upgrade_init` rejects thresholds greater than the approver count |
| Partial rotation silently weakens authorization | Threshold does not auto-drop; tests verify `n - 1` approvals remain insufficient |
| Governance signer swap changes upgrade authority unexpectedly | Keep stored `admin` stable and rotate underlying multisig/governance signers atomically |
| Wrong replacement address is staged | Operationally verify the new signer can authenticate the real path before revoking the old one |

## Trust boundaries and operator powers

- Upgrade authority boundary: only `admin` can initialize approvers and propose upgrades.
- Execution boundary: only currently configured approvers can execute approved proposals.
- Guardian boundary: guardian operations (pause or emergency flows) are separate from upgrade
  authority and do not grant upgrade proposal, execution, or rollback rights.
- Rotation boundary: replacing the approver set takes effect immediately for future
  `upgrade_approve` and `upgrade_execute` calls.

## External call and token transfer safety

- Upgrade entrypoints (`upgrade_propose`, `upgrade_approve`, `upgrade_execute`) do not perform
  token transfers.
- Token transfer paths remain confined to lending operations such as deposit, withdraw, repay,
  and liquidation modules.
- Authorization checks (`require_auth()`) are enforced on every mutating upgrade path.
- Upgrade tests should verify both authorization and invalid-status rejection on each external
  entrypoint.

## Rollback and failure-path coverage checklist

- Execute rejects unknown proposal ids (`UpgradeProposalNotFound`).
- Execute rejects already executed proposals (`UpgradeAlreadyExecuted`).
- Approval rejects duplicate signers (`UpgradeDuplicateApproval`).
- Approval and execute reject expired proposals (`UpgradeProposalExpired`).
- Upgrade propose/approve/execute emit audit events for off-chain monitoring.
