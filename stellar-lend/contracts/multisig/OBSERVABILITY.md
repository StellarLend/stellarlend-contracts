# Multisig Governance and Upgrade Execution: Observability and Diagnostics

This document defines observability patterns, structured diagnostics, and telemetry for multisig governance and upgrade execution to enable visibility into latency, failure, and recovery paths without leaking secrets.

## Table of Contents
1. [Event-Based Observability](#event-based-observability)
2. [Structured Diagnostics](#structured-diagnostics)
3. [Latency Metrics](#latency-metrics)
4. [Failure Path Diagnostics](#failure-path-diagnostics)
5. [Recovery Observability](#recovery-observability)
6. [Security Considerations](#security-considerations)
7. [Client Telemetry Integration](#client-telemetry-integration)

---

## Event-Based Observability

### O1: Proposal Lifecycle Events
**Event Type**: `ProposalCreatedEvent`

**Emitted**: On successful `create_proposal()` call

**Payload**:
```rust
#[contractevent]
pub struct ProposalCreatedEvent {
    pub id: u64,                      // Proposal identifier (useful for tracing)
    pub proposer: Address,            // Creator address
    pub action_kind: Symbol,          // "SetThreshold" | "RotateSigners" | "InvokeContract"
    pub expires_at: u64,              // Ledger expiry time
}
```

**Observability Value**:
- Tracks proposal creation rate (detect governance surge)
- Action-kind distribution (identify governance focus areas)
- Expiry times (verify TTL consistency)

**Example Usage**:
```typescript
// Client: Detect proposal creation bursts
events.on('ProposalCreatedEvent', (event) => {
  metrics.increment('proposal_created', { action_kind: event.action_kind });
  logger.info({
    msg: 'proposal_created',
    id: event.id,
    proposer: event.proposer,
    expires_at: event.expires_at,
  });
});
```

---

**Event Type**: `ProposalApprovedEvent`

**Emitted**: On successful `approve_proposal()` call (even if approval doesn't reach quorum)

**Payload**:
```rust
#[contractevent]
pub struct ProposalApprovedEvent {
    pub id: u64,                      // Proposal identifier
    pub approver: Address,            // Signer granting approval
    pub approval_count: u32,          // Total approvals after this one
    pub passed: bool,                 // true if this approval reached threshold
}
```

**Observability Value**:
- Tracks approval progression (1/N, 2/N, ..., N/N)
- Identifies which approvers are missing (detect offline governance participants)
- `passed=true` signals quorum reached (alert governance)

**Example Usage**:
```typescript
// Client: Monitor approval progress toward quorum
events.on('ProposalApprovedEvent', (event) => {
  const progressPct = (event.approval_count / threshold) * 100;
  metrics.gauge('approval_progress_pct', progressPct, { proposal_id: event.id });
  
  if (event.passed) {
    alerts.notify({
      level: 'info',
      title: 'Proposal Passed',
      message: `Proposal ${event.id} has reached quorum (${event.approval_count} approvals)`,
    });
  }
});
```

---

**Event Type**: `ProposalExecutedEvent`

**Emitted**: On successful or failed `execute_proposal()` call

**Payload**:
```rust
#[contractevent]
pub struct ProposalExecutedEvent {
    pub id: u64,                      // Proposal identifier
    pub action_kind: Symbol,          // Action type
    pub ok: bool,                     // true if action dispatched successfully
}
```

**Observability Value**:
- `ok=true` indicates successful action execution
- `ok=false` indicates action dispatch failure (e.g., cross-contract call failed)
- Correlates with external state changes (protocol updates, parameter changes)

**Example Usage**:
```typescript
// Client: Monitor execution outcomes
events.on('ProposalExecutedEvent', (event) => {
  if (event.ok) {
    metrics.increment('proposal_executed_success', { action_kind: event.action_kind });
    logger.info({
      msg: 'proposal_executed_success',
      id: event.id,
      action_kind: event.action_kind,
    });
  } else {
    metrics.increment('proposal_executed_failure', { action_kind: event.action_kind });
    logger.warn({
      msg: 'proposal_execution_failed',
      id: event.id,
      action_kind: event.action_kind,
    });
    alerts.notify({
      level: 'warning',
      title: 'Proposal Execution Failed',
      message: `Proposal ${event.id} action dispatch failed; eligible for retry`,
    });
  }
});
```

---

**Event Type**: `BatchExecutedEvent`

**Emitted**: On successful `batch_execute()` call (either all succeeded or all rolled back)

**Payload**:
```rust
#[contractevent]
pub struct BatchExecutedEvent {
    pub ids: Vec<u64>,                // Proposal IDs executed in batch
}
```

**Observability Value**:
- Tracks batch execution patterns (governance parallelization)
- Correlates multiple proposal outcomes in single transaction
- Enables audit trail of coordinated actions

**Example Usage**:
```typescript
// Client: Track batch operations
events.on('BatchExecutedEvent', (event) => {
  metrics.gauge('batch_size', event.ids.length);
  logger.info({
    msg: 'batch_executed',
    count: event.ids.length,
    ids: event.ids,
  });
});
```

---

**Event Type**: `ApprovalRevokedEvent`

**Emitted**: On successful `revoke_approval()` call

**Payload**:
```rust
#[contractevent]
pub struct ApprovalRevokedEvent {
    pub proposal_id: u64,             // Proposal identifier
    pub signer: Address,              // Revoking signer
}
```

**Observability Value**:
- Tracks approval changes (detect governance uncertainty)
- Correlates with subsequent re-approvals (detect decision reversals)

**Example Usage**:
```typescript
// Client: Monitor approval changes
events.on('ApprovalRevokedEvent', (event) => {
  metrics.increment('approval_revoked', { proposal_id: event.proposal_id });
  logger.warn({
    msg: 'approval_revoked',
    proposal_id: event.proposal_id,
    signer: event.signer,
  });
});
```

---

### O2: Upgrade Governance Events
**Event Type**: `UpgradeProposedEvent`

**Emitted**: On successful `upgrade_propose()` call

**Payload**:
```rust
#[contractevent]
pub struct UpgradeProposedEvent {
    pub proposal_id: u64,             // Upgrade proposal identifier
    pub new_version: u32,             // Target version
    pub eta_ledger: u64,              // Earliest execution ledger
    pub expires_at: u64,              // Latest execution ledger
}
```

**Observability Value**:
- Tracks upgrade proposals and their timeline windows
- `eta_ledger - current_ledger` = seconds until executable
- `expires_at - current_ledger` = seconds until expired

---

**Event Type**: `UpgradeExecutedEvent`

**Emitted**: On successful `upgrade_execute()` call

**Payload**:
```rust
#[contractevent]
pub struct UpgradeExecutedEvent {
    pub proposal_id: u64,             // Upgrade proposal identifier
    pub old_version: u32,             // Previous version
    pub new_version: u32,             // Deployed version
}
```

**Observability Value**:
- Correlates WASM updates with governance approvals
- Version history enables rollback coordination
- Timestamp enables latency analysis

---

## Structured Diagnostics

### O3: Diagnostic Error Types
Define structured error information that exposes failure root causes:

```rust
#[derive(Clone, Debug)]
pub enum DiagnosticError {
    /// Proposal exists but not in expected state for operation
    InvalidState {
        proposal_id: u64,
        expected_state: &'static str,
        actual_state: &'static str,
        ledger: u64,
    },
    /// Approval binding validation failed
    ApprovalBindingMismatch {
        proposal_id: u64,
        approver: Address,
        reason: &'static str,  // "signer_set_changed" | "already_approved" | "binding_invalid"
    },
    /// Signer set changed since proposal creation
    SignerSetChanged {
        proposal_id: u64,
        expected_hash: BytesN<32>,
        current_hash: BytesN<32>,
    },
    /// Nonce state inconsistency
    NonceConsistency {
        proposal_id: u64,
        nonce: u64,
        expected_status: &'static str,  // "unconsumed" | "consumed"
        actual_status: &'static str,
    },
    /// Cross-contract invocation failed
    CrossContractDispatchFailed {
        proposal_id: u64,
        target_contract: Address,
        function: Symbol,
        retry_eligible: bool,  // true if nonce not consumed
    },
}
```

**Example Usage in Contract**:
```rust
fn approve_proposal(env: Env, id: u64) -> Result<(), MultisigError> {
    let proposal = fetch_proposal(&env, id)?;
    
    // Validate state
    if proposal.status != ProposalStatus::Active {
        let diagnostic = DiagnosticError::InvalidState {
            proposal_id: id,
            expected_state: "Active",
            actual_state: proposal.status.to_string(),
            ledger: env.ledger().sequence(),
        };
        env.events().publish(("multisig", "diagnostic"), &diagnostic);
        return Err(MultisigError::ProposalNotPassed);
    }
    
    // Check signer set hasn't changed
    let current_hash = Self::current_signer_set_hash(&env);
    let proposal_hash = Self::fetch_proposal_signer_set_hash(&env, id)?;
    if current_hash != proposal_hash {
        let diagnostic = DiagnosticError::SignerSetChanged {
            proposal_id: id,
            expected_hash: proposal_hash,
            current_hash,
        };
        env.events().publish(("multisig", "diagnostic"), &diagnostic);
        return Err(MultisigError::SignerSetChanged);
    }
    
    // ... rest of approval logic
    Ok(())
}
```

---

### O4: Diagnostic Views
Expose read-only diagnostic functions for state inspection:

```rust
impl MultisigContract {
    /// Query current multisig state (read-only)
    pub fn get_diagnostics(env: Env) -> DiagnosticsReport {
        DiagnosticsReport {
            threshold: Self::fetch_threshold(&env),
            signer_count: Self::fetch_signers(&env).len() as u32,
            current_signer_set_hash: Self::current_signer_set_hash(&env),
            proposal_count: env.storage()
                .persistent()
                .get::<MultisigDataKey, u64>(&MultisigDataKey::ProposalCount)
                .unwrap_or(0),
            next_nonce: env.storage()
                .persistent()
                .get::<MultisigDataKey, u64>(&MultisigDataKey::NextNonce)
                .unwrap_or(0),
            current_ledger: env.ledger().sequence(),
        }
    }
    
    /// Query specific proposal diagnostic info
    pub fn get_proposal_diagnostics(env: Env, id: u64) -> Result<ProposalDiagnostics, MultisigError> {
        let proposal = Self::fetch_proposal(&env, id)?;
        let nonce = Self::fetch_proposal_nonce(&env, id)?;
        let consumed = env.storage()
            .persistent()
            .has(&MultisigDataKey::ConsumedNonce(nonce));
        
        Ok(ProposalDiagnostics {
            proposal_id: id,
            status: proposal.status.to_string(),
            approval_count: proposal.approvals.len() as u32,
            threshold: Self::fetch_threshold(&env),
            nonce,
            nonce_consumed: consumed,
            expires_at: proposal.expires_at,
            current_ledger: env.ledger().sequence(),
            time_until_expiry_ledgers: if proposal.expires_at > env.ledger().sequence() {
                proposal.expires_at - env.ledger().sequence()
            } else {
                0
            },
        })
    }
    
    /// Verify approval binding out-of-band (audit tool)
    pub fn verify_approval_binding(
        env: Env,
        proposal_id: u64,
        approver: Address,
    ) -> Result<bool, MultisigError> {
        let stored_binding = env.storage()
            .persistent()
            .get::<MultisigDataKey, BytesN<32>>(&MultisigDataKey::ApprovalBinding(proposal_id, approver.clone()))
            .ok_or(MultisigError::ProposalNotFound)?;
        
        let computed_binding = Self::approval_binding_hash(&env, proposal_id, &approver)?;
        Ok(stored_binding == computed_binding)
    }
}
```

---

## Latency Metrics

### O5: Proposal Operation Latency
Contract should emit structured latency telemetry (note: Soroban has limited timing APIs, so clients measure end-to-end):

**Client-Side Measurement**:
```typescript
// Client: Measure proposal operations
const operationTimer = {
  start: Date.now(),
  end: null,
  duration_ms: null,
};

try {
  operationTimer.start = Date.now();
  await multisig.createProposal(action, payloadHash, ttlLedgers);
  operationTimer.end = Date.now();
  operationTimer.duration_ms = operationTimer.end - operationTimer.start;
  
  metrics.histogram('proposal_create_ms', operationTimer.duration_ms);
  logger.info({
    msg: 'proposal_created',
    duration_ms: operationTimer.duration_ms,
  });
} catch (error) {
  operationTimer.end = Date.now();
  operationTimer.duration_ms = operationTimer.end - operationTimer.start;
  metrics.histogram('proposal_create_error_ms', operationTimer.duration_ms);
  logger.error({
    msg: 'proposal_create_failed',
    duration_ms: operationTimer.duration_ms,
    error: error.message,
  });
  throw error;
}
```

**Expected Latencies** (per acceptance criteria):
- `create_proposal()`: O(1) + O(n) hashing ≈ 10-50ms (depends on signer count)
- `approve_proposal()`: O(n) membership check + O(1) binding ≈ 5-20ms
- `execute_proposal()`: O(dispatch time) ≈ 50-500ms (depends on action)
- `batch_execute()`: O(batch_size × dispatch) ≈ 500-5000ms

**Anomaly Detection**:
- Proposal creation >500ms: Possible signer set bloat or network latency
- Approval >100ms: Possible large approval set
- Batch execution >10s: Possible cross-contract failures or gas limits

---

### O6: Signer Set Operations Latency
Monitor signer rotation performance:

```typescript
// Client: Measure signer rotation latency
const signerRotationTimer = {
  start: Date.now(),
};

try {
  signerRotationTimer.start = Date.now();
  await multisig.createProposal(
    ProposalAction.RotateSigners(newSigners),
    payloadHash,
    ttlLedgers,
  );
  const duration = Date.now() - signerRotationTimer.start;
  
  metrics.histogram('signer_rotation_proposal_ms', duration);
  logger.info({
    msg: 'signer_rotation_proposed',
    new_signer_count: newSigners.length,
    duration_ms: duration,
  });
} catch (error) {
  const duration = Date.now() - signerRotationTimer.start;
  metrics.histogram('signer_rotation_failed_ms', duration);
  logger.error({
    msg: 'signer_rotation_failed',
    error: error.message,
    duration_ms: duration,
  });
  throw error;
}
```

---

## Failure Path Diagnostics

### O7: Detailed Failure Logging
Clients should log failures with structured context:

```typescript
// Client: Structured failure logging
async function safeApproveProposal(proposalId: number, approver: Address): Promise<void> {
  const startTime = Date.now();
  const context = {
    proposal_id: proposalId,
    approver: approver.toString(),
    operation: 'approve_proposal',
  };
  
  try {
    // Fetch proposal state before attempting approval
    const proposal = await multisig.getProposal(proposalId);
    context['proposal_status'] = proposal.status;
    context['approval_count'] = proposal.approvals.length;
    context['threshold'] = await multisig.getThreshold();
    
    // Attempt approval
    await multisig.approveProposal(proposalId);
    
    logger.info({
      msg: 'proposal_approval_success',
      ...context,
      duration_ms: Date.now() - startTime,
    });
  } catch (error) {
    context['error'] = error.message;
    context['error_code'] = error.code;
    context['duration_ms'] = Date.now() - startTime;
    
    // Map contract errors to diagnostics
    if (error.code === MultisigError.SignerSetChanged) {
      context['diagnostic'] = 'signer_set_changed_after_proposal';
      logger.warn({
        msg: 'approval_failed_signer_rotation',
        ...context,
      });
      metrics.increment('approval_failed_signer_rotation');
    } else if (error.code === MultisigError.ProposalExpired) {
      context['diagnostic'] = 'proposal_expired';
      logger.warn({
        msg: 'approval_failed_expired',
        ...context,
      });
      metrics.increment('approval_failed_expired');
    } else if (error.code === MultisigError.AlreadyApproved) {
      context['diagnostic'] = 'duplicate_approval';
      logger.info({
        msg: 'approval_already_granted',
        ...context,
      });
    } else {
      logger.error({
        msg: 'approval_failed_unknown',
        ...context,
      });
    }
    
    throw error;
  }
}
```

---

### O8: Authorization Failure Diagnostics
Log authorization and binding failures:

```typescript
// Client: Authorization failure diagnostics
async function safeExecuteProposal(proposalId: number, payloadHash: Bytes): Promise<void> {
  const context = {
    proposal_id: proposalId,
    operation: 'execute_proposal',
  };
  
  try {
    // Pre-execution diagnostics
    const diagnostics = await multisig.getProposalDiagnostics(proposalId);
    context['proposal_status'] = diagnostics.status;
    context['approval_count'] = diagnostics.approval_count;
    context['threshold'] = diagnostics.threshold;
    context['nonce'] = diagnostics.nonce;
    context['nonce_consumed'] = diagnostics.nonce_consumed;
    context['time_until_expiry'] = diagnostics.time_until_expiry_ledgers;
    
    // Verify approval binding (optional, for audit)
    try {
      const caller = await getCurrentSigner();
      const bindingValid = await multisig.verifyApprovalBinding(proposalId, caller);
      context['approval_binding_valid'] = bindingValid;
    } catch (_) {
      context['approval_binding_valid'] = false;
    }
    
    // Execute proposal
    await multisig.executeProposal(proposalId, payloadHash);
    
    logger.info({
      msg: 'proposal_executed_success',
      ...context,
    });
  } catch (error) {
    context['error'] = error.message;
    context['error_code'] = error.code;
    
    if (error.code === MultisigError.PayloadHashMismatch) {
      context['diagnostic'] = 'action_payload_modified';
      logger.error({
        msg: 'execution_failed_payload_tampered',
        ...context,
      });
    } else if (error.code === MultisigError.SignerSetChanged) {
      context['diagnostic'] = 'signer_set_changed_retry_needed';
      logger.warn({
        msg: 'execution_failed_signer_rotation',
        ...context,
      });
    } else if (error.code === MultisigError::ProposalNotPassed) {
      context['diagnostic'] = 'insufficient_approvals';
      logger.warn({
        msg: 'execution_failed_no_quorum',
        ...context,
      });
    } else {
      logger.error({
        msg: 'execution_failed_unknown',
        ...context,
      });
    }
    
    throw error;
  }
}
```

---

## Recovery Observability

### O9: Retry Eligibility Signaling
Clients must distinguish retryable from non-retryable failures:

```rust
#[derive(Clone, Debug)]
pub enum RetryEligibility {
    Retryable {
        reason: &'static str,
        backoff_millis: u64,
    },
    NonRetryable {
        reason: &'static str,
    },
}

impl From<MultisigError> for RetryEligibility {
    fn from(err: MultisigError) -> Self {
        match err {
            // Retryable: temporary state issues
            MultisigError::ProposalNotPassed => {
                RetryEligibility::Retryable {
                    reason: "insufficient_approvals_wait_for_more",
                    backoff_millis: 60_000,  // Retry in 1 minute
                }
            }
            MultisigError::SignerSetChanged => {
                RetryEligibility::Retryable {
                    reason: "signer_rotation_in_progress_retry_after_stabilization",
                    backoff_millis: 300_000,  // Retry in 5 minutes
                }
            }
            MultisigError::ProposalExpired => {
                RetryEligibility::NonRetryable {
                    reason: "proposal_expired_create_new_proposal",
                }
            }
            MultisigError::AlreadyExecuted => {
                RetryEligibility::NonRetryable {
                    reason: "proposal_already_executed_idempotent_success",
                }
            }
            _ => RetryEligibility::NonRetryable {
                reason: "unknown_error",
            },
        }
    }
}
```

**Client Usage**:
```typescript
// Client: Retry logic based on eligibility
async function executeWithRetry(
  proposalId: number,
  payloadHash: Bytes,
  maxRetries: number = 3,
): Promise<void> {
  let retries = 0;
  
  while (retries < maxRetries) {
    try {
      await multisig.executeProposal(proposalId, payloadHash);
      return;  // Success
    } catch (error) {
      const eligibility = parseRetryEligibility(error);
      
      if (eligibility.isRetryable) {
        retries++;
        logger.info({
          msg: 'execution_failed_retryable',
          proposal_id: proposalId,
          attempt: retries,
          reason: eligibility.reason,
          backoff_ms: eligibility.backoff_millis,
        });
        
        await sleep(eligibility.backoff_millis);
      } else {
        logger.error({
          msg: 'execution_failed_non_retryable',
          proposal_id: proposalId,
          reason: eligibility.reason,
        });
        throw error;
      }
    }
  }
  
  throw new Error(`Execution failed after ${maxRetries} retries`);
}
```

---

### O10: Cross-Contract Dispatch Recovery
Track cross-contract call failures and retry eligibility:

```rust
fn execute_proposal(env: Env, id: u64, payload_hash: Bytes) -> Result<(), MultisigError> {
    // ... validation ...
    
    let nonce = Self::fetch_proposal_nonce(&env, id)?;
    
    // Dispatch action; capture result before state mutations
    let dispatch_result = Self::dispatch_action(&env, &action);
    
    match dispatch_result {
        Ok(()) => {
            // Success: mark nonce consumed
            env.storage()
                .persistent()
                .set(&MultisigDataKey::ConsumedNonce(nonce), &true);
            env.storage()
                .persistent()
                .set(&MultisigDataKey::Proposal(id), &Proposal {
                    status: ProposalStatus::Executed,
                    ..proposal
                });
            
            env.events().publish(
                ("multisig", "execution"),
                &ProposalExecutedEvent {
                    id,
                    action_kind: symbol_short!("ok"),
                    ok: true,
                },
            );
            Ok(())
        }
        Err(dispatch_err) => {
            // Failure: nonce NOT consumed, eligible for retry
            env.events().publish(
                ("multisig", "execution"),
                &ProposalExecutedEvent {
                    id,
                    action_kind: symbol_short!("err"),
                    ok: false,
                },
            );
            
            // Emit diagnostic with retry eligibility
            env.events().publish(
                ("multisig", "dispatch_failed"),
                &(
                    ("proposal_id", id),
                    ("nonce", nonce),
                    ("nonce_consumed", false),  // Key: will not be consumed
                    ("retry_eligible", true),   // Safe to retry
                ),
            );
            
            Err(dispatch_err)
        }
    }
}
```

---

## Security Considerations

### O11: No Secret Leakage
Observability must **never** expose:
- Private keys or signatures
- Approval binding hashes (even indirect reconstruction via error messages)
- Full proposal action payloads in error messages
- Signer-set content beyond counts

**Safe Patterns**:
```rust
// ✗ UNSAFE: Exposes action payload
logger.error!("Failed to dispatch action: {:?}", action);

// ✓ SAFE: Only exposes action type
logger.error!("Failed to dispatch action: {:?}", action.variant());

// ✗ UNSAFE: Exposes signer addresses
logger.debug!("Current signers: {:?}", signers);

// ✓ SAFE: Only exposes count and hash
logger.debug!(
    "Current signers: count={}, set_hash={}",
    signers.len(),
    format!("{:x}", signer_set_hash)
);

// ✗ UNSAFE: Exposes approval binding
logger.debug!("Approval binding: {:?}", binding_hash);

// ✓ SAFE: Only indicates validation success/failure
logger.debug!("Approval binding validated: {}", binding_valid);
```

---

### O12: Audit Trail Immutability
Approval bindings and events must persist for audit purposes but must not be modifiable:

```rust
pub fn audit_get_approval_binding(
    env: Env,
    proposal_id: u64,
    approver: Address,
) -> Result<BytesN<32>, MultisigError> {
    // Read-only view for audit tools
    env.storage()
        .persistent()
        .get::<MultisigDataKey, BytesN<32>>(&MultisigDataKey::ApprovalBinding(proposal_id, approver.clone()))
        .ok_or(MultisigError::ProposalNotFound)
}
```

---

## Client Telemetry Integration

### O13: OpenTelemetry Integration Example
Clients should export metrics following OpenTelemetry standards:

```typescript
// Client: OpenTelemetry integration
import { metrics } from '@opentelemetry/api';
import { PeriodicExportingMetricReader } = require('@opentelemetry/sdk-metrics');

const multisigMeter = metrics.getMeter('multisig-governance');

// Counters
const proposalCreatedCounter = multisigMeter.createCounter('multisig.proposal.created', {
  description: 'Number of proposals created',
});

const approvalCounter = multisigMeter.createCounter('multisig.approval.granted', {
  description: 'Number of approvals granted',
});

const executionCounter = multisigMeter.createCounter('multisig.execution.total', {
  description: 'Number of proposal executions',
});

// Histograms
const executionLatencyHistogram = multisigMeter.createHistogram('multisig.execution.latency_ms', {
  description: 'Proposal execution latency in milliseconds',
});

const approvalProgressGauge = multisigMeter.createObservableGauge(
  'multisig.approval.progress_pct',
  {
    description: 'Approval progress toward quorum as percentage',
  },
);

// Example usage
events.on('ProposalExecutedEvent', (event) => {
  executionCounter.add(1, { status: event.ok ? 'success' : 'failure' });
  if (event.ok) {
    executionLatencyHistogram.record(latency, { action: event.action_kind });
  }
});
```

---

## Summary of Observability

| Observable | Event/Metric | Use Case |
|-----------|---|---|
| **Proposal Lifecycle** | ProposalCreatedEvent, ProposalApprovedEvent, ProposalExecutedEvent | Track proposal progression and governance activity |
| **Approval Progress** | approval_count, threshold comparison | Monitor quorum progress |
| **Failure Diagnostics** | DiagnosticError types, error codes | Debug authorization and state issues |
| **Signer Set Changes** | SignerSetChanged error, ProposalDiagnostics | Track governance rotation events |
| **Nonce State** | nonce_consumed flag, ConsumedNonce checks | Verify idempotency and retry eligibility |
| **Latency Metrics** | proposal_create_ms, execution_latency_ms | Detect performance regressions |
| **Retry Eligibility** | RetryEligibility enum, backoff signals | Enable intelligent retry logic |
| **Cross-Contract Failures** | CrossContractDispatchFailed diagnostic, retry_eligible flag | Distinguish transient from permanent failures |
| **Upgrade Governance** | UpgradeProposedEvent, UpgradeExecutedEvent | Correlate WASM updates with governance |

All diagnostics are designed to be:
- **Actionable**: Enable client implementation of retries, alerts, and recovery
- **Privacy-Preserving**: No secret leakage
- **Audit-Friendly**: Immutable event trails
- **Performance-Aware**: Enable latency analysis and anomaly detection
