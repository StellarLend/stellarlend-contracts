# Timelock Governance Operations Guide

## Overview

This document provides operational guidance for using the timelock governance system in StellarLend. The timelock introduces delayed execution for high-risk parameter changes while maintaining emergency response capabilities.

## Quick Reference

### Operation Classifications

| Risk Level | Delay Required | Examples |
|------------|----------------|----------|
| **Immediate** | 0 seconds | `get_admin`, `get_price`, view functions |
| **High Risk** | 7 days | `set_liquidation_threshold_bps`, `set_pause`, `set_guardian` |
| **Critical** | 14 days | `set_oracle`, `upgrade_execute`, `complete_recovery` |

### Key Addresses

- **Admin**: Can queue/execute all operations, bound by delay rules
- **Guardian**: Can execute emergency operations immediately (`emergency_shutdown`, `set_pause`)
- **Timelock Contract**: Acts as the admin for the lending contract

## Common Operations

### 1. Standard Parameter Changes (7-day delay)

**Example: Updating Liquidation Threshold**

```bash
# Step 1: Queue the change
stellar contract invoke \
  --id $TIMELOCK_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- queue \
  --caller $ADMIN_ADDRESS \
  --target $LENDING_CONTRACT \
  --func "set_liquidation_threshold_bps" \
  --args '["8000"]' \
  --eta $(($(date +%s) + 604800))  # 7 days from now

# Step 2: Wait 7 days and monitor community feedback

# Step 3: Execute the change
stellar contract invoke \
  --id $TIMELOCK_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- execute \
  --caller $ADMIN_ADDRESS \
  --target $LENDING_CONTRACT \
  --func "set_liquidation_threshold_bps" \
  --args '["8000"]' \
  --eta $ETA_FROM_STEP1
```

### 2. Critical Operations (14-day delay)

**Example: Oracle Update**

```bash
# Step 1: Queue the oracle change (requires 14-day delay)
stellar contract invoke \
  --id $TIMELOCK_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- queue \
  --caller $ADMIN_ADDRESS \
  --target $LENDING_CONTRACT \
  --func "set_oracle" \
  --args '["$NEW_ORACLE_ADDRESS"]' \
  --eta $(($(date +%s) + 1209600))  # 14 days from now

# Step 2: Wait 14 days with extended community review

# Step 3: Execute after delay
stellar contract invoke \
  --id $TIMELOCK_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- execute \
  --caller $ADMIN_ADDRESS \
  --target $LENDING_CONTRACT \
  --func "set_oracle" \
  --args '["$NEW_ORACLE_ADDRESS"]' \
  --eta $ETA_FROM_STEP1
```

### 3. Immediate Operations

**Example: Reading Protocol State**

```bash
# Can be executed immediately without queueing
stellar contract invoke \
  --id $TIMELOCK_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- execute_immediate \
  --caller $ADMIN_ADDRESS \
  --target $LENDING_CONTRACT \
  --func "get_admin" \
  --args '[]'
```

### 4. Emergency Operations

**Example: Guardian Emergency Shutdown**

```bash
# Guardian can execute immediately
stellar contract invoke \
  --id $TIMELOCK_CONTRACT \
  --source $GUARDIAN_KEY \
  --network testnet \
  -- execute_immediate \
  --caller $GUARDIAN_ADDRESS \
  --target $LENDING_CONTRACT \
  --func "emergency_shutdown" \
  --args '[]'
```

### 5. Multisig Threshold Changes (7-day timelock)

**Security Rationale**: Threshold changes control the minimum number of signatures required to authorize multisig operations. A compromised quorum could lower the threshold and immediately execute a malicious proposal in the same transaction. The 7-day timelock prevents same-ledger takeover by enforcing a mandatory delay between queuing and applying the threshold change.

**Risk Level**: High Risk (7 days minimum delay = ~600,000 ledgers)

**Example: Lowering Multisig Threshold**

```bash
# Step 1: Queue the threshold change (admin only)
stellar contract invoke \
  --id $MULTISIG_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- queue_threshold_change \
  --new_threshold 2

# Output: ThresholdChangeQueuedEvent
# {
#   admin: <admin_address>,
#   new_threshold: 2,
#   eta_ledger: <current_ledger + 600000>
# }

# Step 2: Wait ~7 days (~600,000 ledgers) for the delay to elapse
#         Community reviews and discusses the threshold change
#         Monitor for any objections or concerns

# Step 3: Apply the threshold change after delay has passed
stellar contract invoke \
  --id $MULTISIG_CONTRACT \
  --source $ADMIN_KEY \
  --network testnet \
  -- apply_threshold_change

# Output: ThresholdChangeAppliedEvent
# {
#   admin: <admin_address>,
#   old_threshold: 3,
#   new_threshold: 2,
#   ledger: <current_ledger>
# }
```

**Key Properties**:

- **Atomic Two-Step Flow**: Queue and apply are separate transactions, never executed together
- **Minimum Delay**: 600,000 ledgers (~7 days at 5-second blocks) between queue and apply
- **One Pending Change**: Only one threshold change can be queued at a time; queuing a new change overwrites the previous pending change
- **Admin Only**: Only the multisig admin can queue and apply threshold changes
- **Event Emission**: Both queue and apply operations emit events for indexing in the api/src/services/stellar.service.ts

**Implementation Details**:

```rust
// Queue a new threshold (replaces any existing pending change)
pub fn queue_threshold_change(env: Env, new_threshold: u32) -> Result<(), MultisigError>
// Apply the queued threshold change (requires delay to have elapsed)
pub fn apply_threshold_change(env: Env) -> Result<(), MultisigError>
// Get the pending threshold change (if any)
pub fn get_pending_threshold_change(env: Env) -> Option<ThresholdChange>
// Get minimum delay in ledgers
pub fn get_min_threshold_delay_ledgers(env: Env) -> u32
```

**Common Scenarios**:

*Scenario 1: Malicious Threshold Reduction Attempt*
```
Time T: Compromised quorum queues threshold reduction (3 → 1)
        - Threshold remains at 3 on same ledger
        - Cannot be applied immediately
        
Time T + 7 days: Window opens to apply reduction
                 - Community discovers malicious intent
                 - No apply call made by admin
                 - Threshold never changes, remains at 3
                 - Original quorum security maintained
```

*Scenario 2: Legitimate Threshold Adjustment*
```
Time T: Admin queues threshold adjustment (3 → 2)
        - Documented reason: "Reduce governance friction"
        - Community reviews proposal during 7-day window
        
Time T + 3 days: Community approves the change
                - Admin informed of approval
                
Time T + 7 days: Delay window closes, admin applies change
                - Threshold updated to 2
                - Event emitted for indexing
                - New governance rules take effect
```

## Emergency Response Procedures

### 1. Immediate Threat Response

When an immediate threat is detected:

1. **Guardian Action**: Trigger emergency shutdown
   ```bash
   stellar contract invoke --id $TIMELOCK_CONTRACT --source $GUARDIAN_KEY \
     -- execute_immediate --caller $GUARDIAN_ADDRESS \
     --target $LENDING_CONTRACT --func "emergency_shutdown" --args '[]'
   ```

2. **Verify State**: Check emergency state
   ```bash
   stellar contract invoke --id $TIMELOCK_CONTRACT \
     -- get_emergency_state
   ```

### 2. Recovery Process

After threat mitigation:

1. **Start Recovery** (Admin only):
   ```bash
   stellar contract invoke --id $TIMELOCK_CONTRACT --source $ADMIN_KEY \
     -- start_recovery --caller $ADMIN_ADDRESS
   ```

2. **Allow User Withdrawals**: During recovery, users can repay debts and withdraw collateral

3. **Complete Recovery** (14-day delay required):
   ```bash
   # Queue recovery completion
   stellar contract invoke --id $TIMELOCK_CONTRACT --source $ADMIN_KEY \
     -- queue --caller $ADMIN_ADDRESS --target $LENDING_CONTRACT \
     --func "complete_recovery" --args '[]' \
     --eta $(($(date +%s) + 1209600))
   
   # Execute after 14 days
   stellar contract invoke --id $TIMELOCK_CONTRACT --source $ADMIN_KEY \
     -- execute --caller $ADMIN_ADDRESS --target $LENDING_CONTRACT \
     --func "complete_recovery" --args '[]' --eta $ETA
   ```

## Monitoring and Alerting

### Event Monitoring

Set up monitoring for these critical events:

```javascript
// Monitor queued actions
contract.events.filter({
  topics: ["timelock", "queue"]
}).on('data', (event) => {
  console.log('Action queued:', event.data);
  // Alert community about pending change
});

// Monitor executions
contract.events.filter({
  topics: ["timelock", "execute"]
}).on('data', (event) => {
  console.log('Action executed:', event.data);
  // Log successful execution
});

// Monitor emergency events
contract.events.filter({
  topics: ["timelock", "emergency_shutdown"]
}).on('data', (event) => {
  console.log('EMERGENCY: Protocol shutdown triggered');
  // Send immediate alerts
});

// Monitor multisig threshold changes
contract.events.filter({
  topics: ["multisig", "ThresholdChangeQueuedEvent"]
}).on('data', (event) => {
  console.log('Threshold change queued:', {
    admin: event.admin,
    new_threshold: event.new_threshold,
    eta_ledger: event.eta_ledger,
    eta_time: new Date(event.eta_ledger * 5 * 1000) // ~5 sec per ledger
  });
  // Alert governance participants immediately
  // Trigger 7-day review period
});

contract.events.filter({
  topics: ["multisig", "ThresholdChangeAppliedEvent"]
}).on('data', (event) => {
  console.log('Threshold change applied:', {
    admin: event.admin,
    old_threshold: event.old_threshold,
    new_threshold: event.new_threshold,
    ledger: event.ledger
  });
  // Log governance state change
  // Update UI to reflect new threshold
  // Notify signers of updated requirements
});
```

### Community Notification

For all queued actions:

1. **Immediate Notification**: Post to governance forum/Discord
2. **Technical Details**: Include function name, parameters, execution time
3. **Impact Assessment**: Explain what the change does and why
4. **Objection Period**: Provide clear process for community feedback

## Security Best Practices

### Admin Key Management

1. **Multi-signature**: Use multi-sig wallet for admin operations
2. **Cold Storage**: Keep admin keys in hardware wallets
3. **Rotation**: Regularly rotate admin keys
4. **Backup**: Maintain secure backup procedures

### Multisig Threshold Governance

1. **Careful Adjustments**: Only lower threshold when governance is fully operational
2. **Community Consensus**: Require explicit community approval before threshold changes
3. **Rationale Documentation**: Always document why a threshold change is needed
4. **Monitor Queued Changes**: Set up alerts for all threshold change events
5. **Delay Verification**: Confirm the full 7-day delay before applying changes
6. **Post-Application Review**: Update all signing protocols after threshold changes

**Protection Against Takeover**:
- The 7-day timelock prevents a compromised quorum from executing a takeover in a single block
- Even if attackers lower the threshold, they cannot pass a proposal in the same ledger
- Community has 7 days to detect and prevent the malicious application
- Admin can queue an increased threshold change if compromise is suspected

### Guardian Key Management

1. **Hot Wallet**: Guardian keys should be readily accessible for emergencies
2. **Monitoring**: 24/7 monitoring for threat detection
3. **Response Time**: Aim for <1 hour emergency response
4. **Limited Scope**: Guardian can only execute emergency operations

### Operational Security

1. **Verification**: Always verify queued actions before execution
2. **Community Review**: Allow full delay period for community input
3. **Cancellation**: Be prepared to cancel malicious or erroneous actions
4. **Documentation**: Document all parameter changes and rationale

## Troubleshooting

### Common Errors

**`DelayTooShort`**: Operation requires longer delay
- Solution: Use correct delay for operation risk level
- High-risk: 7 days minimum
- Critical: 14 days minimum

**`DelayNotElapsed`**: Trying to apply threshold change before 7-day window closes
- Solution: Wait for ETA ledger number to pass
- Use get_pending_threshold_change() to check current ETA
- Calculate remaining time: (eta_ledger - current_ledger) * 5 seconds

**`InvalidThreshold`**: Threshold value is 0 or invalid
- Solution: Provide threshold > 0
- Common values: 2, 3, 5, 7 signers

**`NoQueuedChange`**: Trying to apply when no threshold change is pending
- Solution: Queue a threshold change first with queue_threshold_change()
- Check get_pending_threshold_change() to verify a change is queued

**`Unauthorized`**: Caller is not the admin
- Solution: Use the multisig admin account
- Verify admin address with get_admin()

**`ActionNotQueued`**: Trying to execute non-existent action
- Solution: Verify action was queued successfully
- Check action ID matches exactly

**`TimelockNotReady`**: Trying to execute before delay expires
- Solution: Wait until ETA timestamp has passed

**`TimelockExpired`**: Action expired after grace period
- Solution: Re-queue the action with new ETA

**`EmergencyActive`**: Non-emergency operation during emergency state
- Solution: Complete recovery process first, or use emergency-allowed operations only

**`NotGuardian`**: Guardian trying to execute non-emergency operation
- Solution: Use admin account, or limit to emergency operations

### Recovery Scenarios

**Compromised Admin Key**:
1. Guardian triggers emergency shutdown immediately
2. Deploy new timelock with new admin key
3. Update lending contract admin to new timelock
4. Resume operations with new governance structure

**Lost Guardian Key**:
1. Admin can still manage all operations (with delays)
2. Set new guardian address via standard governance process
3. Emergency response capability restored

**Malicious Queued Action**:
1. Admin cancels the queued action immediately
2. Investigate how malicious action was queued
3. Implement additional security measures
4. Consider emergency shutdown if compromise suspected

**Compromised Multisig Threshold (Attempted Takeover)**:
1. **Detection**: Monitor ThresholdChangeQueuedEvent immediately
2. **Assessment**: Analyze the proposed threshold change
   - If malicious (threshold lowered to <quorum), proceed to step 3
   - If legitimate, allow 7-day review period
3. **Prevention** (if malicious):
   - Do NOT call apply_threshold_change() after the 7-day delay
   - Queue a counter-proposal: raise threshold even higher
   - Announce findings to community immediately
   - Document the attempted exploit
4. **Recovery**:
   - Revoke compromised admin key
   - Deploy new multisig contract with secure signers
   - Gradually migrate governance to new multisig

**Threshold Change Applied by Mistake**:
1. Immediately queue a reverting threshold change (back to original)
2. Monitor next ETA to apply the revert
3. If new threshold creates governance crisis:
   - Declare emergency state
   - Use guardian to stabilize protocol
   - Follow full recovery procedures in Emergency Response Procedures

## Timelock Test Coverage

The multisig crate (`stellarlend-multisig`) includes a `#[cfg(test)]` module that exercises every timing-sensitive transition in the threshold-change and proposal-lifecycle flows. All tests advance the ledger sequence to cross timelock boundaries rather than relying on wall-clock time.

### Threshold-Change Timelock

| Test | Ledger position | Expected outcome |
|------|----------------|-----------------|
| `test_queue_threshold_change_success` | at queue ledger | change queued, eta = queue + 600 000 |
| `test_apply_threshold_change_before_delay` | queue + (MIN − 1) | `DelayNotElapsed` |
| `test_apply_at_exact_min_delay_boundary` | queue + MIN − 1 then queue + MIN | first `DelayNotElapsed`; second succeeds |
| `test_apply_threshold_change_after_delay` | queue + MIN | threshold updated, pending cleared |
| `test_apply_at_exact_eta` | exactly `eta_ledger` | succeeds |
| `test_apply_after_eta` | queue + MIN × 2 | succeeds (no upper bound on application) |
| `test_same_ledger_protection` | same ledger as queue | `DelayNotElapsed` |

**Timing boundary**: `MIN_THRESHOLD_DELAY_LEDGERS = 600 000` (≈ 7 days at 5 s/ledger).  
Apply is valid for `current_ledger >= eta_ledger`.

### Proposal Lifecycle

| Test | Ledger position | Expected outcome |
|------|----------------|-----------------|
| `test_execute_proposal_before_eta_rejected` | before eta | `ProposalNotReady` |
| `test_execute_at_exact_eta_boundary` | eta − 1 then eta | first `ProposalNotReady`; second succeeds |
| `test_execute_fresh_proposal_ok` | at eta | threshold updated, `executed = true` |
| `test_execute_proposal_double_execution` | at eta, twice | second call → `ProposalAlreadyExecuted` |
| `test_execute_proposal_not_found` | any | `ProposalNotFound` |
| `test_execute_expired_proposal_rejected` | expires_at + 1 | `ProposalExpired` |
| `test_execute_proposal_at_exact_expiry_ok` | exactly `expires_at` | succeeds (boundary is inclusive) |
| `test_execute_at_expiry_boundary` | exactly `expires_at` then +1 | first succeeds; second `ProposalExpired` |

**Expiry boundary**: `current_ledger > expires_at_ledger` rejects; `current_ledger == expires_at_ledger` succeeds.

### Authorization and Validation

- `test_queue_threshold_change_unauthorized` / `test_apply_threshold_change_unauthorized` — `require_auth()` causes a host-level abort when auth is not mocked; tests use `#[should_panic]`.
- `test_queue_threshold_change_zero_threshold` / `test_create_proposal_invalid_threshold` — zero threshold returns `InvalidThreshold`.
- `test_initialize_already_initialized` — second init returns `AlreadyInitialized`.

## Integration Checklist

### Pre-deployment

- [ ] Deploy timelock contract with correct parameters
- [ ] Deploy multisig contract with admin and initial threshold
- [ ] Set guardian address
- [ ] Configure governance delays (7 days default, 14 days critical)
- [ ] Test all operation classifications
- [ ] Verify emergency procedures
- [ ] Test multisig threshold timelock: queue_threshold_change → apply_threshold_change
- [ ] Verify same-ledger protection (cannot apply before 7 days)
- [ ] Set up event monitoring for ThresholdChangeQueuedEvent and ThresholdChangeAppliedEvent

### Post-deployment

- [ ] Update lending contract admin to timelock address
- [ ] Update timelock admin to multisig contract address
- [ ] Test parameter change flow end-to-end
- [ ] Verify emergency shutdown works
- [ ] Set up event monitoring for multisig threshold changes
- [ ] Train operations team on multisig threshold procedures
- [ ] Document all addresses and keys
- [ ] Create runbooks for threshold adjustment procedures

### Ongoing Operations

- [ ] Monitor queued actions daily
- [ ] Monitor pending threshold changes (check get_pending_threshold_change())
- [ ] Review community feedback on proposals
- [ ] Verify threshold changes are applied only after full delay
- [ ] Maintain guardian key accessibility
- [ ] Regular security audits of procedures
- [ ] Update documentation as needed
- [ ] Log all threshold changes to governance audit trail

## Contact Information

For operational questions or emergency situations:

- **Technical Issues**: [Technical Support Channel]
- **Security Incidents**: [Security Team Contact]
- **Governance Questions**: [Governance Forum]
- **Emergency Contact**: [24/7 Emergency Line]

## Vesting Treasury Sink

When a vesting grant is revoked by the configured admin, any unvested tokens are clawed back and deposited to the protocol treasury address. Operators should note:

- **Revocation Authority**: Only the configured `admin` may call `revoke(grantee)`.
- **Cliff Behavior**: No tokens become claimable until `now >= start + cliff_seconds`.
- **Treasury Sink**: Unvested balance at the time of revoke is transferred to the protocol treasury address configured in the vesting contract.
- **Monitoring**: Watch vesting revoke events and treasury inflows to detect unexpected revocations.
