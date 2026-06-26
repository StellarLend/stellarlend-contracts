# Upgrade Playbook

## Overview

This playbook provides a practical guide for safely upgrading StellarLend contracts, including preflight checks, execution procedures, post-upgrade monitoring, and rollback criteria. It aligns with the upgrade authorization model documented in `docs/UPGRADE_AUTHORIZATION.md`.

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

### Contact and Escalation
- Technical issues: Contact development team
- Security concerns: Follow security protocol
- User complaints: Route through support
- Emergency rollback: Admin can execute immediately

---

**Version:** 1.0  
**Last Updated:** 2025-04-30  
**Review Required:** Every 6 months or after major protocol changes
