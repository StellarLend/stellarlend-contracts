# Guardian Threshold Validation - Fix #1695

## Title
**fix: Add validation to set_guardian_threshold to prevent security vulnerabilities**

## Summary
This PR implements critical validation checks to the `set_guardian_threshold` function in the governance module, addressing Issue #1695. The fix prevents two severe security vulnerabilities:

1. **Threshold Zero Bypass**: Prevents setting threshold to 0, which would trivially bypass social recovery authentication
2. **Recovery Bricking**: Prevents setting threshold above the guardian count, which would make recovery permanently unachievable

## The Problem

### Vulnerability 1: Trivial Recovery Bypass
Without validation, an admin could set `threshold = 0`, which would allow recovery to execute with zero guardian approvals, completely bypassing the social recovery mechanism.

**Impact**: Complete loss of social recovery security guarantees.

### Vulnerability 2: Recovery Bricking
Without validation, an admin could set `threshold > guardian_count`. For example, with 3 guardians registered and threshold=5, recovery becomes mathematically impossible to achieve.

**Impact**: Permanent denial of recovery capability, potentially locking users out of their accounts.

## The Solution

### Changes Made

#### 1. Error Enum Enhancement
Added `InvalidGuardianConfig` error variant to handle invalid guardian configuration states:

```rust
/// The requested guardian configuration would be invalid (e.g. threshold
/// of zero, threshold exceeding the guardian count, or a removal that
/// would make the current threshold unreachable).
InvalidGuardianConfig = 12,
```

#### 2. Validation Implementation
Added two critical validation checks in `set_guardian_threshold`:

**Check 1 - Threshold cannot be zero:**
```rust
if threshold == 0 {
    return Err(GovernanceError::InvalidGuardianConfig);
}
```
*Rationale*: A zero threshold would mean zero approvals are needed, making recovery trivial.

**Check 2 - Threshold cannot exceed guardian count:**
```rust
if threshold > gc.guardians.len() as u32 {
    return Err(GovernanceError::InvalidGuardianConfig);
}
```
*Rationale*: Setting threshold higher than available guardians makes it mathematically impossible to reach the quorum.

#### 3. Recovery Safety Guard
Added check to block threshold changes during active recovery:

```rust
if env.storage().instance().has(&GovernanceDataKey::RecoveryRequest) {
    return Err(GovernanceError::RecoveryInProgress);
}
```
*Rationale*: Prevents retroactively invalidating existing approvals or raising the bar to brick the current recovery.

### Test Coverage

Comprehensive test suite covering all validation scenarios:

| Test | Scenario | Expected Outcome |
|------|----------|------------------|
| `test_guardian_threshold_zero_fails` | Set threshold to 0 | Rejects with `InvalidGuardianConfig` |
| `test_guardian_threshold_exceeds_count_fails` | Set threshold > guardian count | Rejects with `InvalidGuardianConfig` |
| `test_guardian_threshold_change_during_recovery_fails` | Change threshold during recovery | Rejects with `RecoveryInProgress` |
| `test_threshold_change_when_no_recovery_succeeds` | Valid threshold change (no recovery) | Accepts and stores |
| `test_recovery_threshold_edge_case_one` | threshold=1 with 1 guardian | Accepts (valid minimum) |
| 4 additional recovery and removal safety tests | Guardian management safety | All pass |

## Implementation Details

**File Modified**: `stellar-lend/contracts/hello-world/src/governance.rs`

**Functions Enhanced**:
- `set_guardian_threshold()` - Added validation (lines 573-575)
- `GovernanceError` enum - Added `InvalidGuardianConfig` variant (line 12)

**Unmodified**:
- ✅ Recovery logic (`execute_recovery`)
- ✅ Guardian registration (`add_guardian`, `remove_guardian`)
- ✅ Admin authentication checks
- ✅ All other governance functions

## Security Impact

### Before (Vulnerable)
```
threshold = 0 ✅ ACCEPTED (BUG) - Bypasses recovery
threshold = 5 (with 3 guardians) ✅ ACCEPTED (BUG) - Bricks recovery
```

### After (Secure)
```
threshold = 0 ❌ REJECTED - InvalidGuardianConfig
threshold = 5 (with 3 guardians) ❌ REJECTED - InvalidGuardianConfig
threshold = 1-3 (with 3 guardians) ✅ ACCEPTED - Valid range
```

## Testing

All 9 acceptance criteria tests pass:
- ✅ Threshold zero validation
- ✅ Threshold exceeds count validation
- ✅ Recovery in progress blocking
- ✅ Valid threshold acceptance
- ✅ Edge case handling (threshold = count, threshold = 1)
- ✅ Guardian addition/removal safety
- ✅ Admin authorization enforcement

**Test Command**:
```bash
cargo test --lib guardian_threshold_safety_test
```

## Breaking Changes

**None**. This is a pure security hardening:
- Invalid threshold values that should have never been accepted are now rejected
- Valid threshold operations continue to work as before
- Admin auth requirements unchanged
- Error handling is backward compatible

## Related Issues

- Closes #1695: set_guardian_threshold never validates threshold against guardian count

## Acceptance Criteria

- [x] Validation rejects threshold = 0
- [x] Validation rejects threshold > guardian count
- [x] Invalid threshold returns `InvalidGuardianConfig` error
- [x] Valid thresholds (1 to count) accepted
- [x] Recovery safety maintained (changes blocked during active recovery)
- [x] Admin-only access enforced
- [x] All tests pass
- [x] No breaking changes to existing functionality

## Deployment Notes

**No migration needed** - This is a validation-only change. Existing valid configurations continue to work. Invalid configurations that were previously allowed are now rejected.

**Monitoring**: After deployment, monitor for any admin calls to `set_guardian_threshold` with invalid values. These will now be rejected with `InvalidGuardianConfig` errors, which is the intended behavior.

## References

- Issue: #1695
- Related PRs: #1744, #1754, #1755, #1756
- Module: Governance - Social Recovery
- Security Impact: High (prevents account lockout and recovery bypass)

---

**Reviewers**: Please verify:
1. Validation logic correctness
2. Test coverage completeness
3. No unintended side effects on recovery flow
4. Error messaging clarity

