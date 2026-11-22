# Upgrade Playbook

## Implementation status

The canonical lending contract (`stellar-lend/contracts/lending/src/upgrade.rs`) implements `upgrade_init`, `upgrade_propose`, `upgrade_approve`, and `upgrade_execute` with the same timelock / multisig approval model described below. Governance tests live in `src/upgrade_governance_test.rs`.

## Overview

This playbook provides a practical guide for safely upgrading StellarLend contracts, including preflight checks, execution procedures, post-upgrade monitoring, and rollback criteria. It aligns with the upgrade authorization model documented in `docs/UPGRADE_AUTHORIZATION.md`.

## API Contract and Invariants

### Upgrade Functions

The upgrade path is governed by the following contract, which MUST be preserved to avoid breaking governance and upgrade execution:

- `upgrade_init(admin, approvers, required_approvals, timelock_seconds)`: Initializes the multisig governor. Reverts if `admin` is invalid, `required_approvals` is zero, or `approvers` contains duplicates or invalid signers. Enforces a monotonic nonce for all subsequent proposals.
- `upgrade_propose(admin, wasm_hash, new_version)`: Creates a proposal with a unique nonce. Reverts if called by non-admin, if `new_version` is not greater than the current version, or if there is already a pending proposal with the same nonce. A timelock starts when the proposal is created.
- `upgrade_approve(approver, proposal_id)`: Records approval from a validated signer. Reverts if the approver is not in the initial signer set, if the proposal is not pending, or if the approval would exceed the required threshold. Each signer can approve only once.
- `upgrade_execute(approver, proposal_id)`: Enables execution only after the timelock has elapsed and the required approval threshold has been reached. Reverts if called too early, with insufficient approvals, or after the proposal has been executed/rolled back. The current WASM hash and version are atomically updated, and a `up_exec` event is emitted.
- `upgrade_rollback(admin, proposal_id)`: Reverts state to the pre-proposal version and WASM hash. Only callable by admin and only for proposals that have not been executed. Emits `up_rollback`.

### Invariants

- **Nonce-bound approvals**: Each proposal is tied to a monotonically increasing nonce; approvals and executions reference the exact proposal ID. No replay of approvals across proposals is possible.
- **Signer-set validation**: Approvals are accepted only from addresses present in the initial `approvers` set. The set cannot be mutated while proposals are pending.
- **Timelock enforcement**: No execution can occur before `timelock_seconds` has elapsed from proposal creation.
- **Rollback safety**: Rollback restores the exact previous version/hash and leaves all data intact. Repeated rollback is idempotent and guarded by proposal state.
- **Error atomicity**: Any failed operation leaves all storage unchanged; no partial writes occur.
- **State and data invariants**: Proposal state is stored as a single record keyed by nonce/ID and can transition only from `Pending` to `Executed` or `Pending` to `RolledBack`. Current version and WASM hash are committed in the same atomic operation as the proposal transition.
- **Failure invariants**: Invalid callers, unknown or inactive proposals, duplicate approvals, insufficient thresholds, and premature execution revert before any state mutation, leaving all data and version/hash values unchanged.

### Compatibility Guarantee

The public function signatures and event names listed above are the supported API contract. Consumers must not rely on internal storage keys or non-public fields. The contract keeps all existing export names and adds new functionality without removing prior exports. The contract is non-interactive; therefore, keyboard, focus, screen-reader, responsive, and reduced-motion accessibility considerations do not apply.

### Regression Coverage

The following behavior MUST be covered by focused tests in `src/upgrade_governance_test.rs` and `scripts/tests/test_preflight_upgrade.sh`:

- **Success paths**: propose, approve, timelock wait, execute, and rollback. Verify version/hash updates and event emission.
- **Failure paths**: non-admin propose, invalid approver, duplicate approval, premature execution, insufficient threshold, invalid version, and unknown proposal ID.
- **Boundary paths**: zero timelock, zero required approvals, duplicate approvers, empty approver set, and version equality.
- **Retry paths**: failed calls leave state unchanged and a subsequent corrected call succeeds; rollback is guarded for already executed proposals.
- **Permission paths**: admin-only operations, approver-only operations, and threshold boundary (`required_approvals` vs approval count).
- **Loading/empty paths**: the contract has no async loading or UI empty states; equivalents are absent proposal rows and empty signer-set rejection.
- **Accessibility paths**: non-interactive contract has no keyboard/focus/screen-reader/responsive/reduced-motion surface; accessibility compatibility is preserved by the absence of interactive UI.

## Pre-Upgrade Checklist

### 1. Authorization Verification
- [ ] Confirm admin address is controlled and secure
- [ ] Verify approver set is properly configured (minimum `required_approvals`)
- [ ] Test approver keys can authenticate to the network
- [ ] Document all participants and their roles

### 2. Contract State Assessment
- [ ] Backup critical state using `data_backup(&admin, &backup_name)`
- [ ] Record current contract version and WASM hash
- [ ] Document storage schema version
- [ ] Verify data store entry counts and sample critical data
- [ ] Check for any ongoing operations that might conflict

### 3. New WASM Validation
- [ ] Deploy new WASM to testnet/futurenet
- [ ] Run full test suite against new version
- [ ] Verify upgrade migration safety tests pass: `cargo test -p stellarlend-lending upgrade_migration_safety --lib`
- [ ] Validate all 45 tests pass with 0 failures
- [ ] Test key functions with sample data
- [ ] **Run preflight upgrade check**: `./scripts/preflight_upgrade.sh <new_wasm_path> --network testnet`
  - This validates that no exports are removed (backward compatibility)
  - Ensures binary size hasn't grown beyond 10% (configurable with `--max-size-growth`)
  - Compares against the previously deployed artifact from `scripts/deployed/<network>/checksums.txt`
  - Fails if safety checks are not met
  - Use `--force` only with explicit governance approval

### 4. Schema Change Analysis
- [ ] Identify any storage schema changes
- [ ] Document migration requirements
- [ ] Prepare migration memos and version numbers
- [ ] Test migration with backup/restore procedures

### 5. Risk Assessment
- [ ] Review change impact on active operations
- [ ] Identify potential failure modes
- [ ] Prepare rollback triggers and criteria
- [ ] Document monitoring requirements

## Preflight Upgrade Gate

Before executing any upgrade, run the preflight upgrade script to validate the new WASM artifact is safe to deploy.

### Running the Preflight Check

```bash
# Basic usage (compares against last deployed artifact on testnet)
./scripts/preflight_upgrade.sh stellar-lend/target/wasm32-unknown-unknown/release/hello_world.optimized.wasm --network testnet

# With custom size growth threshold (default is 10%)
./scripts/preflight_upgrade.sh <new_wasm_path> --network mainnet --max-size-growth 15

# Force bypass (only with explicit governance approval)
./scripts/preflight_upgrade.sh <new_wasm_path> --network mainnet --force
```

### What the Preflight Check Validates

1. **Export Compatibility**: Ensures no exported functions have been removed from the WASM
   - Removing exports breaks backward compatibility
   - Adding new exports is allowed and reported

2. **Binary Size Growth**: Verifies the new WASM hasn't grown beyond the configured threshold
   - Default threshold: 10% growth
   - Configurable via `--max-size-growth` flag
   - Size reductions are always allowed
   - Large size increases may impact deployment costs and performance

3. **Baseline Comparison**: Uses checksums from `scripts/deployed/<network>/checksums.txt` as the reference
   - The baseline is established during initial deployment
   - Updated via `scripts/deploy.sh --update-checksum` after approved upgrades

### Override Safety

The `--force` flag bypasses all safety checks. This should only be used:
- With explicit governance approval
- After manual review of the changes
- When the size growth is justified and documented
- When export removals are intentional and migration is planned

### Test Coverage

The preflight script has comprehensive test coverage in `scripts/tests/test_preflight_upgrade.sh`:
- 18 test cases covering all scenarios
- Edge cases: missing files, hash mismatches, threshold boundaries
- Override flag testing
- Multi-network support
The governance regression tests in `src/upgrade_governance_test.rs` cover the success, failure, boundary, retry, permission, and loading/empty states defined in "Regression Coverage" above.

Run tests with:
```bash
bash scripts/tests/test_preflight_upgrade.sh
```

## Upgrade Execution

### Step 1: Propose Upgrade
```bash
# Admin proposes new WASM
let proposal_id = client.upgrade_propose(&admin, &new_wasm_hash, &new_version);
```

**Verification:**
- Proposal ID is generated
- New version > current version
- Proposal status is "Pending"

### Step 2: Approve (if threshold > 1)
```bash
# Each approver approves
for approver in approvers {
    client.upgrade_approve(&approver, &proposal_id);
}
```

**Verification:**
- All required approvers have approved
- Approval count >= required_approvals
- Proposal status remains "Pending"

### Step 3: Execute Upgrade
```bash
# Any approver can execute once threshold met
client.upgrade_execute(&approver, &proposal_id);
```

**Verification:**
- Contract version updated to new_version
- WASM hash updated to new_wasm_hash
- Proposal status changes to "Executed"

### Step 4: Schema Migration (if required)
```bash
# Migrate storage schema if changed
client.data_migrate_bump_version(&admin, &schema_version, &migration_memo);
```

**Verification:**
- Schema version updated
- Migration event emitted
- Data remains accessible

## Post-Upgrade Verification

### Immediate Checks (0-5 minutes)
- [ ] Verify `current_version()` matches expected
- [ ] Confirm `current_wasm_hash()` is correct
- [ ] Test critical data entries are accessible
- [ ] Validate admin and approver permissions intact
- [ ] Check contract responds to basic queries

### Functional Tests (5-30 minutes)
- [ ] Test deposit/withdraw operations
- [ ] Verify lending functions work
- [ ] Check liquidation mechanisms
- [ ] Validate event emissions
- [ ] Test permission boundaries

### Monitoring Setup (30+ minutes)
- [ ] Enable enhanced logging for 24 hours
- [ ] Set up alerts for error rates
- [ ] Monitor gas usage patterns
- [ ] Track transaction success rates
- [ ] Watch for unexpected state changes

## Rollback Criteria and Procedure

### Automatic Rollback Triggers
- Any critical data becomes inaccessible
- Contract version or hash mismatch
- Authorization permissions corrupted
- Gas usage exceeds 200% of baseline
- Error rate exceeds 5% for 10 minutes

### Manual Rollback Decision Points
- User reports of fund access issues
- Unexpected behavior in core functions
- Security concerns discovered post-upgrade
- Performance degradation > 50%

### Rollback Procedure
```bash
# Admin initiates rollback
client.upgrade_rollback(&admin, &proposal_id);
```

**Rollback Verification:**
- Version restored to previous
- WASM hash reverted
- All data remains accessible
- Proposal status changes to "RolledBack"

**Post-Rollback Actions:**
- Investigate root cause
- Document failure analysis
- Prepare improved upgrade
- Communicate with stakeholders

## What Can Go Wrong

### Authorization Failures
**Symptoms:** "NotAuthorized" errors during upgrade
**Causes:** 
- Wrong admin/approver addresses
- Key rotation not completed
- Insufficient approvals

**Mitigation:**
- Verify all addresses before starting
- Test authentication with small operations
- Maintain approver threshold safety

### State Corruption
**Symptoms:** Data inaccessible, counts wrong
**Causes:**
- Schema migration failures
- Storage key conflicts
- Incomplete backup/restore

**Mitigation:**
- Always backup before upgrade
- Test migration on sample data
- Verify backup integrity

### Version Conflicts
**Symptoms:** "InvalidVersion" errors
**Causes:**
- Non-monotonic version numbers
- Duplicate proposals
- Clock synchronization issues

**Mitigation:**
- Use sequential version numbers
- Check current version before proposing
- Document version history

### Network Issues
**Symptoms:** Transaction timeouts, failures
**Causes:**
- Network congestion
- RPC endpoint issues
- Gas limit exceeded

**Mitigation:**
- Monitor network status
- Use appropriate gas limits
- Have backup RPC endpoints

## Commands Reference

### Essential Commands
```bash
# Check current state
client.current_version()
client.current_wasm_hash()
client.data_schema_version()
client.data_entry_count()

# Backup/Restore
client.data_backup(&admin, &backup_name)
client.data_restore(&admin, &backup_name)

# Upgrade operations
client.upgrade_propose(&admin, &hash, &version)
client.upgrade_approve(&approver, &proposal_id)
client.upgrade_execute(&approver, &proposal_id)
client.upgrade_rollback(&admin, &proposal_id)

# Schema migration
client.data_migrate_bump_version(&admin, &version, &memo)
```

### Testing Commands
```bash
# Run upgrade safety tests
cargo test -p stellarlend-lending upgrade_migration_safety --lib

# Run specific test categories
cargo test -p stellarlend-lending test_upgrade_preserves --lib
cargo test -p stellarlend-lending test_rollback_scenarios --lib

# Run with detailed output
cargo test -p stellarlend-lending upgrade_migration_safety --lib -- --nocapture
```

## Security Considerations

### Key Management
- Store admin and approver keys securely
- Use hardware security modules where possible
- Rotate keys following authorization procedures
- Never share private keys in communication

### Audit Trail
- All upgrade operations emit events
- Monitor `up_propose`, `up_approve`, `up_exec`, `up_rollback` events
- Keep detailed logs of all upgrade activities
- Document reasons for each upgrade

### Access Control
- Maintain separation of admin and approver roles
- Use multisig for admin operations in production
- Regularly review approver set composition
- Test authorization boundaries regularly

## Communication Protocol

### Pre-Upgrade Communication
- Announce upgrade window 24 hours in advance
- Share upgrade rationale and changes
- Provide rollback timeline
- Set user expectations

### During Upgrade
- Provide real-time status updates
- Communicate any delays immediately
- Share verification results as they complete
- Be transparent about any issues

### Post-Upgrade
- Confirm successful completion
- Share performance metrics
- Document any issues and resolutions
- Schedule follow-up review

## Design Tradeoffs and Limitations

- **Timelock vs responsiveness**: Mandatory timelock reduces upgrade speed to protect users; it cannot be bypassed by a single approver.
- **Immutable signer set**: The approver set is fixed at initialization while proposals are pending; this simplifies nonce-bound approvals but requires a new initialization for key rotation.
- **Rollback scope**: Rollback restores version and WASM hash only; it does not revert data migrations performed after execution. Migration actions remain separate data operations.
- **Preflight baseline**: The preflight script compares against `scripts/deployed/<network>/checksums.txt`; if the baseline is missing or stale, `--force` must be used only with explicit governance approval and manual verification.
- **Non-interactive contract**: Because the contract has no UI, accessibility is limited to preserving the non-UI contract surface; keyboard, focus, screen-reader, responsive, and reduced-motion behavior are not applicable.
- **Validation commands**: Run `cargo test -p stellarlend-lending upgrade_migration_safety --lib` and `bash scripts/tests/test_preflight_upgrade.sh` before any upgrade to validate governance and preflight behavior.

## Appendix

### Related Documentation
- [Upgrade Authorization](UPGRADE_AUTHORIZATION.md) - Authorization model and key rotation
- [Upgrade Safety Tests](../stellar-lend/contracts/lending/UPGRADE_MIGRATION_SAFETY_TESTS.md) - Comprehensive test suite
- [Quick Reference](../stellar-lend/contracts/lending/UPGRADE_QUICK_REFERENCE.md) - Command reference

### Test Coverage Reference
The upgrade safety suite provides 45 tests covering:
- Basic upgrade with state preservation (3 tests)
- Multi-step upgrade paths (3 tests)
- Rollback scenarios (4 tests)
- Failed upgrade handling (4 tests)
- Concurrent operations (2 tests)
- Storage schema migration (3 tests)
- Authorization and security (3 tests)
- Edge cases (5 tests)
- Governance regression tests cover success, failure, boundary, retry, permission, and non-interactive accessibility-equivalent states.

### Contact and Escalation
- Technical issues: Contact development team
- Security concerns: Follow security protocol
- User complaints: Route through support
- Emergency rollback: Admin can execute immediately

---

**Version:** 1.0  
**Last Updated:** 2025-04-30  
**Review Required:** Every 6 months or after major protocol changes
