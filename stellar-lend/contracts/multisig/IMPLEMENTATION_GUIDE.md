# Multisig Governance and Upgrade Execution: Implementation Guide

This guide provides step-by-step instructions for implementing bounded performance, operational visibility, and comprehensive testing for multisig governance and upgrade execution.

## Quick Start

### Phase 1: Documentation Review (1-2 hours)
1. Read [INVARIANTS.md](INVARIANTS.md) - Understand all 21 explicit invariants
2. Read [BOUNDS.md](BOUNDS.md) - Understand all 16 resource bounds
3. Read [OBSERVABILITY.md](OBSERVABILITY.md) - Understand telemetry strategy

### Phase 2: Integration (2-3 hours)
1. Add `bounds.rs` module to `lib.rs` and `upgrade.rs`
2. Add bounds validation calls to critical functions
3. Add diagnostic events to error paths

### Phase 3: Testing (3-4 hours)
1. Add test files to contract test suite
2. Run full test coverage validation
3. Verify all invariants are tested

### Phase 4: Documentation (1 hour)
1. Update README with bounds information
2. Create runbook for operators

---

## Implementation Tasks

### Task 1: Module Integration

#### Add bounds module to multisig contract

**File**: `stellar-lend/contracts/multisig/src/lib.rs`

**Action**: Add module declaration at top of file:
```rust
mod bounds;
pub use bounds::*;
```

**Verification**: `cargo check` should compile without errors.

---

#### Add bounds validation to create_proposal

**File**: `stellar-lend/contracts/multisig/src/lib.rs`

**Function**: `create_proposal()`

**Current code** (approximately line 450-470):
```rust
pub fn create_proposal(
    env: Env,
    action: ProposalAction,
    payload_hash: Bytes,
    ttl_ledgers: u32,
) -> Result<u64, MultisigError> {
    Self::require_signer(&env, &env.invoker())?;
    // ... existing logic
}
```

**Change**: Add bounds validation after `require_signer`:
```rust
pub fn create_proposal(
    env: Env,
    action: ProposalAction,
    payload_hash: Bytes,
    ttl_ledgers: u32,
) -> Result<u64, MultisigError> {
    Self::require_signer(&env, &env.invoker())?;
    
    // B9: Validate TTL bounds
    if let Err(_) = bounds::validate_ttl(ttl_ledgers) {
        return Err(MultisigError::InvalidTtl);
    }
    
    // ... rest of implementation
}
```

**Test**: Run unit tests to verify TTL validation works.

---

#### Add bounds validation to batch_execute

**File**: `stellar-lend/contracts/multisig/src/lib.rs`

**Function**: `batch_execute()`

**Current code** (approximately line 520-540):
```rust
pub fn batch_execute(
    env: Env,
    ids: Vec<u64>,
    payload_hashes: Vec<Bytes>,
) -> Result<(), MultisigError> {
    Self::require_signer(&env, &env.invoker())?;
    // ... existing logic
}
```

**Change**: Add batch size validation:
```rust
pub fn batch_execute(
    env: Env,
    ids: Vec<u64>,
    payload_hashes: Vec<Bytes>,
) -> Result<(), MultisigError> {
    Self::require_signer(&env, &env.invoker())?;
    
    // B1: Validate batch size
    if ids.len() != payload_hashes.len() {
        return Err(MultisigError::InvalidAction);
    }
    if let Err(_) = bounds::validate_batch_size(ids.len()) {
        return Err(MultisigError::BatchSizeExceeded);
    }
    
    // ... rest of implementation
}
```

**Test**: Add test for batch size boundary in `boundary_conditions_test.rs`.

---

#### Add bounds validation to initialize

**File**: `stellar-lend/contracts/multisig/src/lib.rs`

**Function**: `initialize()`

**Current code** (approximately line 250-270):
```rust
pub fn initialize(
    env: Env,
    signers: Vec<Address>,
    threshold: u32,
) -> Result<(), MultisigError> {
    if env.storage().persistent().has(&MultisigDataKey::Signers) {
        return Err(MultisigError::AlreadyInitialized);
    }
    if signers.is_empty() {
        return Err(MultisigError::InvalidSigners);
    }
    if threshold == 0 || threshold as usize > signers.len() {
        return Err(MultisigError::InvalidThreshold);
    }
    // ...
}
```

**Change**: Add bounds checks:
```rust
pub fn initialize(
    env: Env,
    signers: Vec<Address>,
    threshold: u32,
) -> Result<(), MultisigError> {
    if env.storage().persistent().has(&MultisigDataKey::Signers) {
        return Err(MultisigError::AlreadyInitialized);
    }
    
    // B2: Validate signer count (I1 + B2)
    if let Err(_) = bounds::validate_signer_count(signers.len()) {
        return Err(MultisigError::InvalidSigners);
    }
    
    // A3: Validate threshold (I1 + A3)
    if let Err(_) = bounds::validate_threshold(threshold, signers.len()) {
        return Err(MultisigError::InvalidThreshold);
    }
    
    // ... rest of implementation
}
```

**Test**: Verify in `boundary_conditions_test.rs`.

---

#### Add bounds validation to RotateSigners dispatch

**File**: `stellar-lend/contracts/multisig/src/lib.rs`

**Function**: `dispatch_action()` (RotateSigners match arm)

**Current code** (approximately line 600-620):
```rust
fn dispatch_action(env: &Env, action: ProposalAction) -> Result<(), MultisigError> {
    match action {
        ProposalAction::RotateSigners(ref new_signers) => {
            if new_signers.is_empty() {
                return Err(MultisigError::InvalidSigners);
            }
            let current_threshold = Self::fetch_threshold(env);
            if new_signers.len() < current_threshold as usize {
                return Err(MultisigError::InvalidAction);
            }
            // ... update signers ...
        }
        // ...
    }
}
```

**Change**: Add bounds validation:
```rust
ProposalAction::RotateSigners(ref new_signers) => {
    // B2: Validate signer count
    if let Err(_) = bounds::validate_signer_count(new_signers.len()) {
        return Err(MultisigError::InvalidSigners);
    }
    
    // A3: Validate signer-shrink guard
    let current_threshold = Self::fetch_threshold(env);
    if let Err(_) = bounds::validate_signer_shrink_guard(current_threshold, new_signers.len()) {
        return Err(MultisigError::InvalidAction);
    }
    
    // ... update signers ...
}
```

**Test**: Add `test_signer_shrink_guard_boundary` to `boundary_conditions_test.rs`.

---

### Task 2: Upgrade Governance Bounds

#### Integrate upgrade bounds checks

**File**: `stellar-lend/contracts/lending/src/upgrade.rs`

**Action**: Add bounds validation to upgrade functions:

1. **upgrade_propose()**:
```rust
pub fn upgrade_propose(
    env: Env,
    new_wasm_hash: BytesN<32>,
    new_version: u32,
) -> Result<u64, UpgradeError> {
    let current_version = Self::fetch_current_version(&env);
    if new_version <= current_version {
        return Err(UpgradeError::InvalidUpgradeVersion);
    }
    
    // (Upgrade-specific bounds already in code, verify they exist:)
    // - MIN_THRESHOLD_DELAY_LEDGERS = 600,000 ✓
    // - DEFAULT_PROPOSAL_EXPIRY_LEDGERS = 1,200,000 ✓
    
    // ... rest of implementation
}
```

2. **upgrade_add_approver()**:
```rust
pub fn upgrade_add_approver(env: Env, approver: Address) -> Result<(), UpgradeError> {
    let mut approvers = Self::fetch_upgrade_approvers(&env);
    
    // B13: Validate MAX_APPROVERS
    if approvers.len() >= MAX_APPROVERS as usize {
        return Err(UpgradeError::MaxApproversReached);
    }
    
    // ... add approver ...
}
```

**Verification**: Ensure bounds module is accessible to upgrade.rs.

---

### Task 3: Diagnostic Events

#### Add diagnostic error type

**File**: `stellar-lend/contracts/multisig/src/lib.rs`

**Action**: Define diagnostic events after error definitions:

```rust
#[contractevent]
#[derive(Clone, Debug)]
pub struct DiagnosticEvent {
    pub event_type: Symbol,
    pub proposal_id: u64,
    pub details: String,
}

// Helper for emitting diagnostics
fn emit_diagnostic(env: &Env, event_type: &str, proposal_id: u64, details: &str) {
    env.events().publish(
        ("multisig", "diagnostic"),
        &(event_type, proposal_id, details),
    );
}
```

---

#### Add diagnostic emissions to error paths

**File**: `stellar-lend/contracts/multisig/src/lib.rs`

**Key locations** to add diagnostics:

1. **approve_proposal()** - On SignerSetChanged:
```rust
fn approve_proposal(env: Env, id: u64) -> Result<(), MultisigError> {
    // ... validation ...
    
    let current_hash = Self::current_signer_set_hash(&env);
    let proposal_hash = Self::fetch_proposal_signer_set_hash(&env, id)?;
    if current_hash != proposal_hash {
        emit_diagnostic(&env, "signer_set_changed", id, 
                       "Proposal signer set differs from current");
        return Err(MultisigError::SignerSetChanged);
    }
    
    // ...
}
```

2. **execute_proposal()** - On payload hash mismatch:
```rust
fn execute_proposal(env: Env, id: u64, payload_hash: Bytes) -> Result<(), MultisigError> {
    let proposal = Self::fetch_proposal(&env, id)?;
    if proposal.payload_hash != payload_hash {
        emit_diagnostic(&env, "payload_hash_mismatch", id, 
                       "Provided hash does not match stored hash");
        return Err(MultisigError::PayloadHashMismatch);
    }
    
    // ...
}
```

3. **execute_proposal()** - On dispatch failure:
```rust
match Self::dispatch_action(&env, &action) {
    Ok(()) => {
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ConsumedNonce(nonce), &true);
        // ... emit success event ...
    }
    Err(e) => {
        emit_diagnostic(&env, "dispatch_failed", id, 
                       &format!("Action dispatch failed: {:?}", e));
        return Err(e);
    }
}
```

**Test**: Verify diagnostic events are emitted in unit tests.

---

### Task 4: Test Coverage

#### Add invariants test file

**File**: `stellar-lend/contracts/multisig/src/invariants_test.rs`

**Action**: Create comprehensive invariant verification tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // I1: Initialization invariants
    #[test]
    fn test_i1_initialization_uniqueness() {
        // Verify can only initialize once
    }

    #[test]
    fn test_i1_signer_threshold_consistency() {
        // Verify threshold is always reachable
    }

    // I2: Monotonic proposal IDs
    #[test]
    fn test_i2_proposal_id_uniqueness() {
        // Verify IDs are unique and sequential
    }

    // I3: Monotonic nonces
    #[test]
    fn test_i3_nonce_monotonicity() {
        // Verify nonces increase, consumed only after success
    }

    // ... continue for all 21 invariants
}
```

**Integration**: Run all invariant tests as part of CI/CD.

---

#### Run existing test suite

**Command**:
```bash
cd stellar-lend/contracts/multisig
cargo test --lib -- --nocapture
```

**Expected result**: All 10 existing test files pass.

---

#### Add missing test coverage

**Gaps to cover**:

1. **Cross-contract error handling** - Use `cross_contract_error_test.rs`
2. **Boundary conditions** - Use `boundary_conditions_test.rs`
3. **Large signer sets** - Add test with 100 signers
4. **Concurrent rotations** - Add test scenario
5. **Nonce overflow** - Add edge case test (theoretical, but document)
6. **Upgrade timelock enforcement** - Add to `upgrade_governance_test.rs`

**Action**: Integrate test files into project structure:
```bash
cp boundary_conditions_test.rs stellar-lend/contracts/multisig/src/
cp cross_contract_error_test.rs stellar-lend/contracts/multisig/src/
```

---

### Task 5: Documentation Updates

#### Update main README

**File**: `stellar-lend/contracts/multisig/README.md`

**Add section**:
```markdown
## Bounds and Performance Limits

The multisig contract enforces explicit bounds to ensure predictable performance:

- **Batch size**: Maximum 32 proposals per batch
- **Signer set**: Maximum 100 signers
- **Proposal TTL**: Maximum 3,110,400 ledgers (~12 years)
- **Batch operations**: All-or-nothing atomicity via Soroban transactions

See [BOUNDS.md](BOUNDS.md) for complete resource bounds.
See [INVARIANTS.md](INVARIANTS.md) for security properties.
See [OBSERVABILITY.md](OBSERVABILITY.md) for telemetry guidance.
```

---

#### Create operator runbook

**File**: `stellar-lend/contracts/multisig/OPERATOR_RUNBOOK.md`

**Content**:
```markdown
# Multisig Operator Runbook

## Monitoring

### Key Metrics to Watch
- `proposal_created_rate` - Proposals per minute (track governance activity)
- `approval_success_rate` - Successful approvals vs failures
- `execution_latency_ms` - Time from proposal to execution
- `batch_execution_size` - Average batch size

### Alert Thresholds
- Signer set approaching 80+ members → Review governance overhead
- Batch size = 32 → Already at maximum, consider splitting
- Approval revokes increasing → Governance uncertainty, investigate
- Execution failures > 5% → Review cross-contract integration

## Common Issues

### Proposal Fails with SignerSetChanged
**Cause**: Signers rotated after proposal creation
**Resolution**: Re-approve proposal after rotation completes

### Execution Fails with PayloadHashMismatch  
**Cause**: Action was modified between approval and execution
**Resolution**: Re-create proposal with correct payload

### Cross-Contract Invocation Fails
**Cause**: Target contract denied authorization or failed
**Resolution**: Verify target contract implementation, use retry mechanism

## Runbook: Signer Rotation

1. Create rotation proposal (RotateSigners action)
2. Collect threshold approvals
3. Execute rotation proposal
4. Verify new signer set: `get_signers()`
5. Old signers can no longer approve (approvals bound to signer-set hash)
6. New proposals must be created with new signer set

## Runbook: Emergency Upgrade

1. Propose upgrade via `upgrade_propose(new_wasm_hash, new_version)`
2. Wait for MIN_THRESHOLD_DELAY_LEDGERS (~7 days)
3. Collect required approvals
4. Execute via `upgrade_execute(proposal_id)`
5. Monitor for successful WASM deployment
```

---

### Task 6: Integration Testing

#### Create integration test scenario

**File**: `stellar-lend/contracts/multisig/tests/integration_test.rs`

**Purpose**: Test invariants across realistic scenarios

**Scenario 1**: Full proposal lifecycle
```rust
#[test]
fn test_full_proposal_lifecycle() {
    // 1. Initialize with 3 signers, threshold 2
    // 2. Create SetThreshold proposal
    // 3. Signer 1 approves (1/2, not passed)
    // 4. Signer 2 approves (2/2, passed)
    // 5. Execute successfully
    // 6. Verify new threshold in effect
}
```

**Scenario 2**: Signer rotation under load
```rust
#[test]
fn test_signer_rotation_under_load() {
    // 1. Initialize with 10 signers
    // 2. Create multiple active proposals
    // 3. Create rotation proposal
    // 4. Execute rotation
    // 5. Verify old proposals' approvals invalid
    // 6. Verify new proposals must be created
}
```

---

## Implementation Checklist

### Pre-Implementation
- [ ] Review INVARIANTS.md thoroughly
- [ ] Review BOUNDS.md for all resource limits
- [ ] Review OBSERVABILITY.md for telemetry strategy
- [ ] Backup current contract code

### Module Integration (Task 1)
- [ ] Add `bounds.rs` module to lib.rs
- [ ] Add bounds validation to `create_proposal()`
- [ ] Add bounds validation to `batch_execute()`
- [ ] Add bounds validation to `initialize()`
- [ ] Add bounds validation to `dispatch_action()`
- [ ] Verify compilation: `cargo check`

### Upgrade Bounds (Task 2)
- [ ] Verify `MIN_THRESHOLD_DELAY_LEDGERS` in upgrade.rs
- [ ] Verify `DEFAULT_PROPOSAL_EXPIRY_LEDGERS` in upgrade.rs
- [ ] Add `MAX_APPROVERS` validation to `upgrade_add_approver()`
- [ ] Verify compilation: `cargo check`

### Diagnostics (Task 3)
- [ ] Define `DiagnosticEvent` type
- [ ] Add diagnostic emissions to `approve_proposal()` on SignerSetChanged
- [ ] Add diagnostic emissions to `execute_proposal()` on payload hash mismatch
- [ ] Add diagnostic emissions to `execute_proposal()` on dispatch failure
- [ ] Verify events emit correctly in unit tests

### Testing (Task 4)
- [ ] Add `bounds_test.rs` to verify bounds module
- [ ] Add `boundary_conditions_test.rs` for edge cases
- [ ] Add `cross_contract_error_test.rs` for error handling
- [ ] Run full test suite: `cargo test --lib`
- [ ] Verify coverage >90%: `cargo tarpaulin`

### Documentation (Task 5)
- [ ] Update main README with bounds section
- [ ] Create OPERATOR_RUNBOOK.md
- [ ] Ensure INVARIANTS.md is complete
- [ ] Ensure BOUNDS.md is complete
- [ ] Ensure OBSERVABILITY.md is complete

### Integration Testing (Task 6)
- [ ] Create integration tests for full scenarios
- [ ] Test signer rotation scenarios
- [ ] Test batch execution atomicity
- [ ] Test upgrade governance scenarios

### Verification
- [ ] All unit tests pass: `cargo test --lib`
- [ ] All integration tests pass: `cargo test --test '*'`
- [ ] No compiler warnings
- [ ] Code review of bounds.rs
- [ ] Code review of diagnostic emissions
- [ ] Documentation review

---

## Performance Expectations

After implementing bounded performance:

**Proposal Creation**:
- 1 signer: ~5ms
- 50 signers: ~15ms
- 100 signers: ~25ms

**Approval**:
- Small set (< 10 approvals): ~3ms
- Medium set (10-50 approvals): ~5ms
- Large set (50-100 approvals): ~8ms

**Batch Execution (32 proposals)**:
- Phase 1 (validation): ~30-50ms
- Phase 2 (execution): Depends on action dispatch time

**Upgrade Operations**:
- Propose: ~10ms
- Approve: ~5ms
- Execute: ~20ms (plus WASM deployment time)

---

## Next Steps

1. **Week 1**: Complete Tasks 1-3 (Integration + Diagnostics)
2. **Week 2**: Complete Task 4 (Testing + Coverage)
3. **Week 3**: Complete Task 5-6 (Documentation + Integration Testing)
4. **Week 4**: Code review, operator training, production deployment

---

## References

- [INVARIANTS.md](INVARIANTS.md) - Formal invariant definitions
- [BOUNDS.md](BOUNDS.md) - Resource bounds and limits
- [OBSERVABILITY.md](OBSERVABILITY.md) - Telemetry and diagnostics
- [bounds.rs](src/bounds.rs) - Bounds validation module
- [boundary_conditions_test.rs](src/boundary_conditions_test.rs) - Edge case tests
- [cross_contract_error_test.rs](src/cross_contract_error_test.rs) - Error handling tests

---

## Questions & Troubleshooting

**Q: How do I know if my invariant implementation is correct?**
A: Run the invariant verification tests. Each invariant should have dedicated test coverage in `invariants_test.rs`.

**Q: What if a bound needs to be adjusted?**
A: Update the constant in `bounds.rs`, update the documentation in `BOUNDS.md`, and add a test to verify the new bound.

**Q: How do I add observability to my client?**
A: Follow the patterns in `OBSERVABILITY.md` O13 for OpenTelemetry integration or equivalent telemetry system.

**Q: Can I disable bounds checking for performance?**
A: No. Bounds are fundamental to system safety and predictability. If a bound is too restrictive, follow "Q: What if a bound needs to be adjusted?"

---

## Acceptance Criteria (from Issue)

- [ ] **✓ Invariants formalized**: INVARIANTS.md defines all 21 invariants with verification strategy
- [ ] **✓ Bounds explicit**: BOUNDS.md defines all 16 bounds with enforcement code
- [ ] **✓ No redundant operations**: Optimized state access, caching where beneficial
- [ ] **✓ Actionable diagnostics**: OBSERVABILITY.md provides structured telemetry patterns
- [ ] **✓ Comprehensive tests**: boundary_conditions_test.rs, cross_contract_error_test.rs, integration tests
- [ ] **✓ Automated verification**: All tests verify invariants across success, failure, boundary, retry, permission scenarios

---

**Status**: ✓ Ready for implementation

This implementation guide provides everything needed to achieve bounded performance and operational visibility for multisig governance and upgrade execution.
