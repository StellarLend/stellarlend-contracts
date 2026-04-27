# Common Crate Shared Types

This crate currently exposes StellarLend's shared upgrade-management types and logic through `stellarlend_common::upgrade`.

## Shared Invariants

- `required_approvals` must be greater than zero.
- `required_approvals` and the approver set are bounded by `MAX_UPGRADE_APPROVERS`.
- proposal versions must increase monotonically.
- `Approved`, `Executed`, and `RolledBack` proposals must have at least `required_approvals` recorded approvals.
- `Executed` and `RolledBack` proposals must retain prior WASM hash and version metadata for rollback safety.

## Safe Conversions

The common crate now treats `UpgradeProposal -> UpgradeStatus` as a validated conversion instead of an unchecked field copy. Consumers should use `UpgradeProposal::try_into_status` or `UpgradeManager::upgrade_status` so corrupted approval counts or inconsistent stages fail closed.

## Trust Boundaries

- Admin powers: initialize upgrade storage, manage the approver set, create proposals, and roll back executed upgrades.
- Approver powers: approve proposals and execute upgrades once threshold is met.
- No guardian or token-transfer logic lives in this crate.
- Reentrancy surface is minimal because the module does not invoke user-controlled contracts; the only external effect is host-managed WASM replacement during upgrade execution or rollback.

## Notes For Downstream Contracts

- Do not reconstruct `UpgradeStatus` manually in downstream crates.
- Do not widen approval counts or thresholds beyond the common crate's bound.
- Treat `StorageCorrupted` as a hard stop: it indicates persisted upgrade state violates shared invariants.
