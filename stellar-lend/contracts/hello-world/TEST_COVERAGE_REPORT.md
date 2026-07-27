# Test Coverage Report — Guardian Threshold Safety & Authorization Documentation

> **Last updated**: 2026-07-27  
> **Status**: Issue #513 guardrails implemented and tested. Issue #521 doc
> rewritten to match the real codebase. Previously this document described
> tests and error variants that did not exist; those discrepancies are resolved
> below.

---

## Issue #513: Guardian Multisig Threshold Change Safety

### Implementation Summary

Added two new error variants to `GovernanceError` in
`stellar-lend/contracts/hello-world/src/governance.rs`:

| Variant | Code | Meaning |
|---|---|---|
| `RecoveryInProgress` | 11 | A recovery is active; threshold/guardian changes are blocked |
| `InvalidGuardianConfig` | 12 | The resulting config would be invalid (zero threshold, threshold > count, removal bricking count below threshold) |

`set_guardian_threshold` now:
1. Returns `RecoveryInProgress` when `GovernanceDataKey::RecoveryRequest` is
   present in instance storage.
2. Returns `InvalidGuardianConfig` when `threshold == 0` or `threshold >
   gc.guardians.len()`.

`remove_guardian` now:
1. Returns `RecoveryInProgress` when a recovery is active.
2. Returns `InvalidGuardianConfig` when the removal would leave fewer guardians
   than the current threshold (i.e. `(count - 1) < threshold`).

### Test File

`stellar-lend/contracts/hello-world/src/guardian_threshold_safety_test.rs`

| Test Function | Scenario | Expected |
|---|---|---|
| `test_guardian_threshold_change_during_recovery_fails` | Threshold change while recovery active | `RecoveryInProgress` |
| `test_guardian_removal_during_recovery_fails` | Guardian removal while recovery active | `RecoveryInProgress` |
| `test_guardian_removal_would_brick_recovery_fails` | Remove guardian → count < threshold | `InvalidGuardianConfig` |
| `test_guardian_removal_safe_when_enough_remain` | Remove guardian → count >= threshold | Success |
| `test_threshold_change_when_no_recovery_succeeds` | Normal threshold change, no recovery active | Success |
| `test_recovery_threshold_edge_case_one` | Threshold = 1 with exactly one guardian | Success |
| `test_guardian_threshold_zero_fails` | Set threshold to 0 | `InvalidGuardianConfig` |
| `test_guardian_threshold_exceeds_count_fails` | Set threshold > guardian count | `InvalidGuardianConfig` |
| `test_guardian_removal_clears_after_recovery_completes` | Remove after recovery clears | Success |

### Running the Tests

```bash
cd stellar-lend/contracts/hello-world
cargo test guardian_threshold_safety_test
```

### What is NOT yet implemented

- Auto-adjustment of threshold on guardian removal (the threshold is validated
  but not silently reduced). If auto-adjustment is desired, file a follow-up
  issue.
- `recovery_test.rs` — the broader recovery flow tests referenced in the
  original version of this document do not exist yet.

---

## Issue #521: Authorization Primitives Documentation

### Implementation Summary

`stellar-lend/contracts/hello-world/docs/authorization-primitives.md` has been
rewritten to describe the **real** authorization surface:

- Single admin model via `admin::require_admin`.
- Guardian model via `GovernanceDataKey::GuardianConfig`.
- No RBAC (`grant_role` / `revoke_role` / `require_role_or_admin` do not exist).
- `gov_can_vote` documented as a governance query function, not a reusable auth primitive.

See `docs/authorization-primitives.md` for the full reference.

---

## Combined Coverage Analysis

| Area | Status | Notes |
|---|---|---|
| `RecoveryInProgress` guard on `set_guardian_threshold` | ✅ Implemented & tested | |
| `RecoveryInProgress` guard on `remove_guardian` | ✅ Implemented & tested | |
| `InvalidGuardianConfig` on zero threshold | ✅ Implemented & tested | |
| `InvalidGuardianConfig` on threshold > count | ✅ Implemented & tested | |
| `InvalidGuardianConfig` on removal bricking count | ✅ Implemented & tested | |
| Recovery-completes-then-removal | ✅ Tested | |
| Auto-threshold-adjustment on removal | ❌ Not implemented | Out of scope; file separate issue if desired |
| Full `governance_test.rs` suite | ❌ Not yet written | Tracked separately |
| Full `recovery_test.rs` suite | ❌ Not yet written | Tracked separately |

---

## CI

```bash
# Run guardian safety tests
cargo test guardian_threshold_safety_test

# Run all hello-world tests
cd stellar-lend/contracts/hello-world
cargo test
```
