# Event Schema Versioning and Migration Compatibility

## Overview

All protocol events include a `schema_version` field to enable safe decoding across contract upgrades and ensure indexer compatibility. This document defines the versioning policy, migration paths, and indexer integration guide.

## Current Schema Version

**Version: 1**

Defined in `stellar-lend/contracts/lending/src/events.rs`:
```rust
pub const EVENT_SCHEMA_VERSION: u32 = 1;
```

## Event Schema Design Principles

### 1. **All Events Carry Version**

Every event struct includes `schema_version: u32` as the first or prominent field:

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositEvent {
    pub schema_version: u32,  // ← Version field
    pub user: Address,
    pub amount: i128,
    pub new_balance: i128,
    pub timestamp: u64,
}
```

###2. **Version Emitted at Initialization**

Contract emits `SchemaVersionEvent` once during initialization:

```rust
pub fn emit_schema_version(env: &Env) {
    let event = SchemaVersionEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        timestamp: env.ledger().timestamp(),
    };
    env.events().publish((Symbol::new(env, "SchemaVersionEvent"),), event);
}
```

### 3. **Immutability of Published Schemas**

Once a schema version is deployed to mainnet:
- Field names, types, and order are **IMMUTABLE**
- Cannot remove fields
- Cannot change field semantics
- Can only ADD new events or EXTEND existing events in new versions

### 4. **Backward Compatibility Requirement**

Indexers MUST:
- Parse events based on `schema_version` field
- Support all historical schema versions
- Gracefully handle unknown future versions

## Event Inventory (Version 1)

| Event Name | Purpose | Key Fields |
|------------|---------|------------|
| `SchemaVersionEvent` | Anchor schema version at initialization | `schema_version`, `timestamp` |
| `DepositEvent` | User deposits collateral | `user`, `amount`, `new_balance` |
| `WithdrawEvent` | User withdraws collateral | `user`, `amount`, `new_balance` |
| `BorrowEvent` | User borrows against collateral | `user`, `amount`, `new_debt` |
| `RepayEvent` | User repays debt | `user`, `amount`, `new_debt` |
| `FlashLoanEvent` | Flash loan initiated | `initiator`, `receiver`, `amount`, `fee` |
| `FlashLoanRepaidEvent` | Flash loan repaid | `payer`, `asset`, `amount` |
| `LiquidationEventV1` | Liquidation executed | `liquidator`, `borrower`, `repaid`, `seized`, `health_factor_before`, `shortfall` |
| `DebtCeilingUpdatedEvent` | Admin updates debt ceiling | `ceiling` |
| `FlashFeeUpdatedEvent` | Admin updates flash fee | `fee_bps` |
| `CloseFactorBpsSetEvent` | Admin sets close factor | `close_factor_bps` |
| `LiquidationIncentiveBpsSetEvent` | Admin sets liquidation incentive | `incentive_bps` |
| `BadDebtWrittenOffEvent` | Governance writes off bad debt | `amount`, `insurance_used`, `reserve_used`, `socialized` |

## Schema Evolution Strategy

### When to Increment Version

Increment `EVENT_SCHEMA_VERSION` when:
1. **Adding fields** to existing event (breaking change for strict parsers)
2. **Changing field semantics** (e.g., changing units from wei to gwei)
3. **Removing fields** (not recommended - deprecated fields should remain with zero/null values)
4. **Reordering fields** (breaks position-based parsers)

### How to Add a New Event (Non-Breaking)

Adding a completely new event type does NOT require version increment:

```rust
// Version 1 - add new event
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewFeatureEvent {
    pub schema_version: u32,  // Still version 1
    pub feature_data: i128,
    pub timestamp: u64,
}
```

Rationale: Indexers can safely ignore unknown event types. Only changes to EXISTING event schemas require versioning.

### How to Extend an Event (Breaking Change)

To add a field to an existing event:

**Step 1:** Create new event struct with incremented version suffix:

```rust
// OLD (version 1)
#[contracttype]
pub struct DepositEvent {
    pub schema_version: u32,
    pub user: Address,
    pub amount: i128,
    pub new_balance: i128,
    pub timestamp: u64,
}

// NEW (version 2)
#[contracttype]
pub struct DepositEventV2 {
    pub schema_version: u32,
    pub user: Address,
    pub amount: i128,
    pub new_balance: i128,
    pub timestamp: u64,
    pub asset: Address,  // ← New field
    pub operation_id: Option<BytesN<32>>,  // ← New field
}
```

**Step 2:** Update `EVENT_SCHEMA_VERSION` constant:

```rust
pub const EVENT_SCHEMA_VERSION: u32 = 2;
```

**Step 3:** Keep old event struct for backward compatibility:

```rust
// Retain old struct definition (do not remove!)
// Indexers may still reference this for historical events
```

**Step 4:** Emit new event version:

```rust
pub fn emit_deposit(env: &Env, user: &Address, amount: i128, new_balance: i128, asset: &Address, operation_id: Option<BytesN<32>>) {
    let event = DepositEventV2 {
        schema_version: EVENT_SCHEMA_VERSION,  // Now 2
        user: user.clone(),
        amount,
        new_balance,
        timestamp: env.ledger().timestamp(),
        asset: asset.clone(),
        operation_id,
    };
    env.events().publish((Symbol::new(env, "DepositEventV2"),), event);
}
```

## Indexer Integration Guide

### Parsing Events by Version

**Recommended Pattern:**

```rust
match event.schema_version {
    1 => {
        // Parse as DepositEvent (v1)
        let deposit_event: DepositEvent = event.data.try_into()?;
        handle_deposit_v1(deposit_event);
    }
    2 => {
        // Parse as DepositEventV2
        let deposit_event: DepositEventV2 = event.data.try_into()?;
        handle_deposit_v2(deposit_event);
    }
    _ => {
        // Unknown future version - log warning and skip
        warn!("Unknown event schema version: {}", event.schema_version);
    }
}
```

### Querying Schema Version

Indexers should:
1. On contract discovery, fetch `SchemaVersionEvent` from initialization
2. Store active schema version in indexer database
3. Use version to determine parsing logic for all subsequent events

**Example Query:**
```sql
-- Indexer DB schema
CREATE TABLE contract_metadata (
    contract_address TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    initialized_at TIMESTAMP NOT NULL
);

-- On SchemaVersionEvent:
INSERT INTO contract_metadata (contract_address, schema_version, initialized_at)
VALUES ('GCXXX...', 1, '2024-01-01 00:00:00');
```

### Handling Missing Fields (Forward Compatibility)

If indexer encounters newer schema version with additional fields:

**Option 1: Parse Common Fields**
```rust
// Extract only fields that exist in your version
let user = event.get("user")?;
let amount = event.get("amount")?;
// Ignore unknown fields
```

**Option 2: Skip Event**
```rust
if event.schema_version > SUPPORTED_VERSION {
    warn!("Skipping event with unsupported version {}", event.schema_version);
    return Ok(());
}
```

### Reconstructing Transaction History

For complete audit trail, indexers should correlate events by:

1. **Ledger Sequence + Transaction Hash** (Stellar-level)
2. **Operation ID** (protocol-level, if included in event)
3. **Flash Loan Request ID** (for flash loan event correlation)

**Example:**
```
Transaction 0x1234...
  ├─ DepositEventV2 (operation_id: 0xABCD...)
  ├─ OperationRegisteredEvent (operation_id: 0xABCD...)
  └─ OperationCompletedEvent (operation_id: 0xABCD...)
```

## Migration Timeline

### Phase 1: Pre-Deployment (Current)

- ✅ All events include `schema_version: u32` field
- ✅ `SchemaVersionEvent` emitted at initialization
- ✅ Indexer documentation published

### Phase 2: Testnet Deployment

1. Deploy contract with version 1 events
2. Monitor indexer compatibility
3. Collect feedback on missing fields/events
4. Iterate on schema design

### Phase 3: Mainnet Deployment

1. Deploy frozen version 1 schema to mainnet
2. Version 1 becomes **immutable**
3. All future changes require new version

### Phase 4: Post-Deployment Evolution

When new features require event schema changes:
1. Design version 2 schema
2. Deploy to testnet for indexer testing
3. After 30-day compatibility verification, deploy to mainnet
4. Support both v1 and v2 events during transition period
5. Eventually deprecate v1 (but keep parsing support)

## New Event: OperationExecutedEvent (Proposed for V2)

To support full operation tracking, propose adding:

```rust
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationExecutedEvent {
    pub schema_version: u32,
    pub operation_id: BytesN<32>,
    pub operation_type: Symbol,  // "deposit", "borrow", "repay", "withdraw"
    pub user: Address,
    pub status: OperationStatus,  // Completed, Failed
    pub result: Option<OperationResult>,
    pub sequence_number: u64,
    pub executed_at: u64,
}
```

**Emit After Every Operation:**
```rust
pub fn emit_operation_executed(env: &Env, operation_id: &BytesN<32>, operation_type: Symbol, user: &Address, status: OperationStatus, result: Option<OperationResult>, sequence: u64) {
    let event = OperationExecutedEvent {
        schema_version: 2,  // New version
        operation_id: operation_id.clone(),
        operation_type,
        user: user.clone(),
        status,
        result,
        sequence_number: sequence,
        executed_at: env.ledger().timestamp(),
    };
    env.events().publish((Symbol::new(env, "OperationExecutedEvent"),), event);
}
```

**Benefits:**
- Indexers can reconstruct complete operation history
- Detect duplicate/retry attempts
- Correlate operations across multiple transactions
- Audit sequence number progression

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_event_includes_schema_version() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    emit_deposit(&env, &user, 1000, 1000);
    
    let events = env.events().all();
    let deposit_event: DepositEvent = events.get(0).unwrap().try_into().unwrap();
    
    assert_eq!(deposit_event.schema_version, 1);
}

#[test]
fn test_schema_version_event_emitted_at_init() {
    let env = Env::default();
    
    emit_schema_version(&env);
    
    let events = env.events().all();
    let schema_event: SchemaVersionEvent = events.get(0).unwrap().try_into().unwrap();
    
    assert_eq!(schema_event.schema_version, EVENT_SCHEMA_VERSION);
}
```

### Integration Tests

```rust
#[test]
fn test_indexer_can_parse_all_event_versions() {
    // Simulate indexer parsing events from multiple contract versions
    let v1_event = create_deposit_event_v1();
    let v2_event = create_deposit_event_v2();
    
    assert!(parse_event_by_version(v1_event).is_ok());
    assert!(parse_event_by_version(v2_event).is_ok());
}
```

### Compatibility Tests

```rust
#[test]
fn test_old_indexer_handles_new_schema_gracefully() {
    // Indexer built for v1 encounters v2 event
    let v2_event = DepositEventV2 { /* ... */ };
    
    // Should either:
    // 1. Parse common fields successfully, OR
    // 2. Skip event with warning (not crash)
    
    let result = parse_deposit_event_v1_only(v2_event);
    assert!(result.is_ok() || result.is_err_with_warning());
}
```

## Emergency Schema Hotfix Process

If critical bug found in deployed event schema:

**Option 1: Add Correction Event**
```rust
// Bug: DepositEvent.amount is in wrong units
// Fix: Emit correction event
#[contracttype]
pub struct DepositCorrectionEvent {
    pub schema_version: u32,
    pub original_event_id: BytesN<32>,
    pub corrected_amount: i128,
}
```

**Option 2: Rapid Version Increment**
```rust
// Immediately deploy v1.1 with corrected schema
pub const EVENT_SCHEMA_VERSION: u32 = 11;  // 1.1 encoded as 11
```

**Option 3: Off-Chain Indexer Patching**
```rust
// If event is parseable but semantically wrong,
// apply transformation in indexer:
if schema_version == 1 && event_name == "DepositEvent" {
    amount = event.amount * CORRECTION_FACTOR;
}
```

## References

- Stellar Event Documentation: https://developers.stellar.org/docs/smart-contracts/guides/events
- Soroban SDK Event Types: https://docs.rs/soroban-sdk/latest/soroban_sdk/events/
- Contract Events Best Practices: https://soroban.stellar.org/docs/fundamentals-and-concepts/events

## Changelog

### Version 1 (Initial Release)
- **Date:** 2024-01-15
- **Events:** SchemaVersionEvent, DepositEvent, WithdrawEvent, BorrowEvent, RepayEvent, FlashLoanEvent, FlashLoanRepaidEvent, LiquidationEventV1, DebtCeilingUpdatedEvent, FlashFeeUpdatedEvent, CloseFactorBpsSetEvent, LiquidationIncentiveBpsSetEvent, BadDebtWrittenOffEvent
- **Breaking Changes:** N/A (initial version)
- **Migration:** N/A

### Version 2 (Proposed)
- **Target Date:** TBD (post-mainnet deployment feedback)
- **New Events:** OperationExecutedEvent
- **Extended Events:** DepositEventV2, WithdrawEventV2, BorrowEventV2, RepayEventV2 (add `operation_id` and `asset` fields)
- **Breaking Changes:** Yes (field additions)
- **Migration:** Indexers must add v2 parsing logic alongside v1
- **Deprecation:** V1 events remain supported indefinitely

## Summary

✅ **All events carry `schema_version` for safe decoding**  
✅ **`SchemaVersionEvent` emitted at initialization**  
✅ **Version 1 schema is production-ready and immutable after mainnet deploy**  
✅ **Indexers have clear integration guide**  
✅ **Migration path defined for future versions**  
✅ **Emergency hotfix process documented**  

**Indexer developers:** Reference this document when building event parsers. Always check `schema_version` before parsing event data.

**Protocol developers:** Follow schema evolution process when adding new fields. Never modify existing event structures after mainnet deployment.
