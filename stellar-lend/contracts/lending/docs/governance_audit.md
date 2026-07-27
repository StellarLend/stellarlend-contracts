# Governance Audit Log

## Overview

The StellarLend protocol includes a comprehensive governance audit log that tracks all administrative and governance actions. This provides transparency, compliance monitoring, and incident response capabilities for protocol operators and users.

## Features

- **Complete Coverage**: All admin actions are automatically logged
- **Immutable Records**: Audit entries cannot be modified once created
- **Gas-Efficient Storage**: Circular buffer with configurable maximum entries
- **Real-Time Events**: Events emitted for off-chain monitoring
- **Flexible Payload System**: Extensible schema for different action types

## Architecture

### Storage Structure

```
AuditLogKey::Count        -> u64 (total entries)
AuditLogKey::Entry(N)    -> AuditEntry (circular buffer)
```

### Audit Entry Structure

```rust
pub struct AuditEntry {
    pub id: u64,                    // Sequential ID
    pub action: GovernanceAction,     // Type of action
    pub caller: Address,             // Who performed it
    pub timestamp: u64,              // When it occurred
    pub payload: GovernancePayload,    // Action-specific data
}
```

## Governance Actions

The audit log tracks the following action types:

### Protocol Management
- `Initialize` - Protocol initialization with admin and settings
- `SetAdmin` - Admin address changes
- `SetGuardian` - Guardian configuration

### Emergency Controls
- `SetPause` - Pause state changes for specific operations
- `EmergencyShutdown` - Emergency protocol halt
- `StartRecovery` - Begin controlled recovery mode
- `CompleteRecovery` - Return to normal operation

### Oracle Management
- `SetOracle` - Global oracle configuration
- `ConfigureOracle` - Oracle parameter changes
- `SetPrimaryOracle` - Asset-specific primary oracle
- `SetFallbackOracle` - Asset-specific fallback oracle
- `SetOraclePaused` - Oracle pause state
- `UpdatePriceFeed` - Price feed submissions

### Risk Parameters
- `SetLiquidationThreshold` - Liquidation threshold BPS
- `SetCloseFactor` - Close factor BPS
- `SetLiquidationIncentive` - Liquidation incentive BPS

### Protocol Settings
- `InitializeBorrowSettings` - Borrow module initialization
- `InitializeDepositSettings` - Deposit module initialization
- `InitializeWithdrawSettings` - Withdraw module initialization
- `SetFlashLoanFee` - Flash loan fee configuration

### Cross-Asset Operations
- `InitializeCrossAssetAdmin` - Cross-asset admin setup
- `SetAssetParams` - Asset-specific parameters

### Upgrade Management
- `UpgradeInit` - Upgrade system initialization
- `UpgradeAddApprover` - Add upgrade approver
- `UpgradeRemoveApprover` - Remove upgrade approver
- `UpgradePropose` - Submit upgrade proposal
- `UpgradeApprove` - Approve upgrade proposal
- `UpgradeExecute` - Execute approved upgrade
- `UpgradeRollback` - Roll back failed upgrade

### Financial Operations
- `CreditInsuranceFund` - Add funds to insurance
- `OffsetBadDebt` - Clear bad debt with insurance

### Data Management
- `GrantDataWriter` - Grant data write permissions
- `RevokeDataWriter` - Revoke data write permissions
- `DataBackup` - Create data backup
- `DataRestore` - Restore from backup
- `DataMigrate` - Schema migration

## API Reference

### View Functions

#### `get_governance_audit_entries(limit: u32) -> Vec<AuditEntry>`

Returns the most recent audit entries in reverse chronological order.

**Parameters:**
- `limit`: Maximum number of entries to return (1-100)

**Returns:**
- Vector of audit entries, newest first

**Example:**
```rust
// Get last 10 governance actions
let entries = contract.get_governance_audit_entries(&env, 10);
for entry in entries.iter() {
    println!("Action {}: {:?}", entry.id, entry.action);
}
```

#### `get_governance_audit_count() -> u64`

Returns the total number of audit entries since contract deployment.

**Returns:**
- Total count of audit entries

**Example:**
```rust
let total_actions = contract.get_governance_audit_count(&env);
println!("Total governance actions: {}", total_actions);
```

### Events

#### `GovernanceAuditEvent`

Emitted for every governance action with the following structure:

```rust
pub struct GovernanceAuditEvent {
    pub id: u64,                    // Sequential ID
    pub action: GovernanceAction,     // Action type
    pub caller: Address,             // Performer address
    pub timestamp: u64,              // Block timestamp
    pub payload: GovernancePayload,    // Action data
}
```

## Usage Examples

### Monitoring Recent Activity

```rust
// Get last 20 governance actions
let recent = contract.get_governance_audit_entries(&env, 20);

// Filter for emergency actions
for entry in recent.iter() {
    match entry.action {
        GovernanceAction::EmergencyShutdown |
        GovernanceAction::StartRecovery |
        GovernanceAction::CompleteRecovery => {
            println!("Emergency action by {:?}", entry.caller);
        }
        _ => {}
    }
}
```

### Compliance Checking

```rust
// Get all actions from last 24 hours
let current_time = env.ledger().timestamp();
let one_day_ago = current_time - 86400;

let entries = contract.get_governance_audit_entries(&env, 100);
for entry in entries.iter() {
    if entry.timestamp >= one_day_ago {
        // Process recent governance actions
        match entry.action {
            GovernanceAction::SetAdmin => {
                // Check admin change compliance
            }
            GovernanceAction::UpgradeExecute => {
                // Verify upgrade procedures
            }
            _ => {}
        }
    }
}
```

### Incident Response

```rust
// Find actions before an incident
let entries = contract.get_governance_audit_entries(&env, 50);
for entry in entries.iter() {
    match entry.action {
        GovernanceAction::SetPause(pause_type, paused) => {
            if *paused {
                println!("Protocol paused: {:?}", pause_type);
            }
        }
        GovernanceAction::EmergencyShutdown => {
            println!("Emergency shutdown triggered by {:?}", entry.caller);
        }
        _ => {}
    }
}
```

## Payload Schemas

Different action types use different payload structures:

### Simple Actions
```rust
// No additional data needed
EmergencyShutdown, StartRecovery, CompleteRecovery
```

### Address-Only Actions
```rust
// Single address parameter
SetAdmin(address), SetGuardian(address), SetOracle(address)
```

### Value-Only Actions
```rust
// Single numeric parameter
SetLiquidationThreshold(bps), SetCloseFactor(bps), SetFlashLoanFee(bps)
```

### Address + Value Actions
```rust
// Address with numeric parameter
CreditInsuranceFund(asset, amount), OffsetBadDebt(asset, amount)
```

### Complex Actions
```rust
// Multiple parameters
Initialize(admin, debt_ceiling, min_borrow)
UpgradePropose(wasm_hash, version, proposal_id)
```

## Security Considerations

### Immutable Records
- Audit entries cannot be modified after creation
- Provides tamper-evident governance history

### Authorization Enforcement
- All audit logging occurs after successful authorization
- Failed actions are not logged (no state change)

### Gas Efficiency
- Circular buffer limits storage growth
- Maximum 1000 entries configurable
- Query limits prevent gas exhaustion

### Privacy
- Only stores addresses and public parameters
- No sensitive user data in audit logs
- Compliant with data protection requirements

## Integration Guidelines

### Off-Chain Monitoring

Set up event listeners for `GovernanceAuditEvent`:

```javascript
// Example JavaScript monitoring
const provider = new StellarProvider();
const contract = new Contract(address, abi);

contract.events.GovernanceAuditEvent()
    .on('data', (event) => {
        const { id, action, caller, timestamp, payload } = event.returnValues;
        
        // Process governance action
        console.log(`Governance action ${id}: ${action} by ${caller}`);
        
        // Store in monitoring database
        storeAuditEntry(event.returnValues);
    });
```

### Alerting

Configure alerts for critical actions:

```yaml
alerts:
  - action: EmergencyShutdown
    severity: critical
    notification: slack, email
    
  - action: UpgradeExecute
    severity: high
    notification: slack
    
  - action: SetAdmin
    severity: medium
    notification: email
```

### Compliance Reporting

Generate regular compliance reports:

```rust
// Daily governance summary
fn generate_daily_report(env: &Env) -> ComplianceReport {
    let entries = get_recent_audit_entries(env, 100);
    let mut report = ComplianceReport::new();
    
    for entry in entries.iter() {
        if entry.timestamp >= yesterday_start() {
            report.add_action(entry);
        }
    }
    
    report
}
```

## Best Practices

### For Protocol Operators

1. **Regular Monitoring**: Monitor audit events in real-time
2. **Access Control**: Limit admin keys to essential personnel
3. **Documentation**: Document all governance decisions
4. **Testing**: Test governance actions in staging first
5. **Backup**: Maintain backups of audit data

### For Users

1. **Verification**: Check audit log for recent changes
2. **Alerts**: Set up notifications for critical actions
3. **Transparency**: Review governance history regularly
4. **Security**: Verify admin actions are legitimate

### For Developers

1. **Integration**: Use audit log for governance UI
2. **Filtering**: Implement efficient filtering for large datasets
3. **Caching**: Cache frequently accessed audit data
4. **Validation**: Validate audit data integrity
5. **Testing**: Include audit logging in all tests

## Troubleshooting

### Common Issues

**Missing Audit Entries**
- Check if action was successful (failed actions aren't logged)
- Verify function has audit logging implemented
- Check authorization requirements

**Storage Limits**
- Monitor entry count approaching MAX_AUDIT_ENTRIES
- Implement external archival for long-term storage
- Consider increasing MAX_AUDIT_ENTRIES if needed

**Performance Issues**
- Use appropriate query limits
- Implement pagination for large datasets
- Cache frequently accessed entries

### Debug Information

Enable debug logging to trace audit operations:

```rust
// In development builds
#[cfg(debug_assertions)]
log_governance_action(&env, action, caller, payload);
```

## Version History

### v1.0.0
- Initial implementation
- 35 governance action types
- Circular buffer storage
- Event emission
- View functions

### Future Enhancements
- Action filtering and search
- Batch operations
- External archival integration
- Enhanced reporting tools
- Action categorization
