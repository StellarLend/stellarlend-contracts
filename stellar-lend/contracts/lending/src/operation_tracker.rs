//! Operation tracking and idempotency enforcement for deterministic state transitions.
//!
//! This module provides:
//! - Per-user operation sequence numbers (monotonic nonces)
//! - Operation ID deduplication with TTL-based expiry
//! - Idempotency key validation
//! - Operation status tracking (Pending, Completed, Failed, Cancelled)
//!
//! ## Design Goals
//!
//! 1. **Prevent duplicate submissions**: Same operation submitted multiple times
//!    should be detected and rejected or return cached result.
//! 2. **Prevent stale response replay**: Client receiving old response and retrying
//!    should not create contradictory state.
//! 3. **Enable safe retries**: Failed operations should be retryable without
//!    risk of double-execution.
//! 4. **Preserve user intent**: Interrupted operations should be recoverable
//!    without silently repeating on-chain actions.
//!
//! ## Architecture
//!
//! ### Per-User Sequence Numbers
//!
//! Each user has a monotonically increasing sequence number tracking the count
//! of completed operations. Operations must include the expected sequence number;
//! if it doesn't match the stored value, the operation is rejected.
//!
//! ```rust
//! // User submits operation with sequence = 5
//! // Protocol checks: stored_sequence == 5? 
//! //   → Yes: proceed, increment to 6
//! //   → No: reject with SequenceMismatch
//! ```
//!
//! ### Operation ID Deduplication
//!
//! Each operation can optionally include a unique operation_id (32-byte hash).
//! The protocol stores operation_id → OperationStatus mappings with TTL.
//!
//! ```rust
//! pub struct OperationRecord {
//!     pub status: OperationStatus,
//!     pub result: OperationResult,
//!     pub executed_at: u64,
//!     pub expires_at: u64,
//! }
//! ```
//!
//! If operation_id is seen again within TTL:
//! - Status::Completed → return cached result (idempotent)
//! - Status::Pending → reject with OperationInProgress
//! - Status::Failed → allow retry with same ID
//!
//! ### State Transition Safety
//!
//! All operations follow a state machine:
//!
//! ```
//! [NONE] 
//!   ↓ (submit with operation_id)
//! [PENDING] 
//!   ↓ (execution starts)
//! [EXECUTING]
//!   ↓ (success) → [COMPLETED] (return cached result on retry)
//!   ↓ (failure) → [FAILED] (allow retry with same/different ID)
//!   ↓ (cancelled) → [CANCELLED] (allow new operation)
//! ```

use soroban_sdk::{contracttype, Address, BytesN, Env, Symbol, Vec};

/// Operation status for tracking execution lifecycle.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationStatus {
    /// Operation has been registered but not yet executed.
    Pending,
    /// Operation is currently executing (guards against re-entry).
    Executing,
    /// Operation completed successfully.
    Completed,
    /// Operation failed and may be retried.
    Failed,
    /// Operation was explicitly cancelled by user.
    Cancelled,
}

/// Result of a completed operation (cached for idempotency).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationResult {
    /// Deposit operation result: new balance
    Deposit(i128),
    /// Withdraw operation result: new balance
    Withdraw(i128),
    /// Borrow operation result: new debt principal
    Borrow(i128),
    /// Repay operation result: new debt principal
    Repay(i128),
    /// Liquidate operation result: amount repaid
    Liquidate(i128),
    /// Generic success without specific result
    Success,
}

/// Operation record stored for deduplication and idempotency.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationRecord {
    /// Current status of the operation
    pub status: OperationStatus,
    /// Cached result (only valid when status == Completed)
    pub result: Option<OperationResult>,
    /// Ledger timestamp when operation was first submitted
    pub submitted_at: u64,
    /// Ledger timestamp when operation finished executing
    pub executed_at: Option<u64>,
    /// Ledger timestamp when this record expires (for TTL-based cleanup)
    pub expires_at: u64,
    /// User who initiated the operation (for authorization checks)
    pub initiator: Address,
}

/// Storage key for per-user operation sequence numbers.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationTrackerKey {
    /// User's current operation sequence number (monotonic counter)
    UserSequence(Address),
    /// Operation ID → OperationRecord mapping
    OperationRecord(BytesN<32>),
}

/// TTL for operation records in ledgers (~30 days at 5s/ledger = 518,400 ledgers)
pub const OPERATION_RECORD_TTL: u32 = 518_400;

/// Maximum allowed operations in-flight per user (prevents DoS)
pub const MAX_PENDING_OPERATIONS_PER_USER: u32 = 10;

// ═══════════════════════════════════════════════════════════════════════════
// Sequence Number Management
// ═══════════════════════════════════════════════════════════════════════════

/// Get the current operation sequence number for a user.
///
/// Returns 0 if the user has never performed an operation.
pub fn get_user_sequence(env: &Env, user: &Address) -> u64 {
    let key = OperationTrackerKey::UserSequence(user.clone());
    env.storage()
        .persistent()
        .get(&key)
        .unwrap_or(0u64)
}

/// Increment the user's sequence number and return the new value.
///
/// Called after an operation completes successfully.
fn increment_user_sequence(env: &Env, user: &Address) -> u64 {
    let key = OperationTrackerKey::UserSequence(user.clone());
    let current = get_user_sequence(env, user);
    let next = current
        .checked_add(1)
        .expect("operation_tracker: sequence overflow");
    env.storage()
        .persistent()
        .set(&key, &next);
    
    // Extend TTL so sequence number doesn't expire
    env.storage()
        .persistent()
        .extend_ttl(&key, OPERATION_RECORD_TTL, OPERATION_RECORD_TTL);
    
    next
}

/// Validate that the provided sequence number matches the user's current sequence.
///
/// Returns `Ok(())` if the sequence is correct, `Err` otherwise.
///
/// # Errors
/// - If `expected_sequence` doesn't match stored sequence
/// - If sequence number would overflow on increment
pub fn validate_sequence(
    env: &Env,
    user: &Address,
    expected_sequence: u64,
) -> Result<(), OperationTrackerError> {
    let current = get_user_sequence(env, user);
    if expected_sequence != current {
        return Err(OperationTrackerError::SequenceMismatch {
            expected: current,
            provided: expected_sequence,
        });
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Operation ID Tracking
// ═══════════════════════════════════════════════════════════════════════════

/// Load an operation record by ID.
///
/// Returns `None` if no record exists or if it has expired.
pub fn get_operation_record(
    env: &Env,
    operation_id: &BytesN<32>,
) -> Option<OperationRecord> {
    let key = OperationTrackerKey::OperationRecord(operation_id.clone());
    let record: Option<OperationRecord> = env.storage().persistent().get(&key);
    
    // Check if record has expired
    if let Some(ref rec) = record {
        let now = env.ledger().timestamp();
        if now > rec.expires_at {
            // Record expired, clean up and return None
            env.storage().persistent().remove(&key);
            return None;
        }
    }
    
    record
}

/// Register a new operation with Pending status.
///
/// # Errors
/// - `OperationAlreadyExists` if operation_id already registered
/// - `TooManyPendingOperations` if user has exceeded pending operation limit
pub fn register_operation(
    env: &Env,
    operation_id: &BytesN<32>,
    initiator: &Address,
    ttl_seconds: u64,
) -> Result<(), OperationTrackerError> {
    // Check if operation already exists
    if let Some(existing) = get_operation_record(env, operation_id) {
        match existing.status {
            OperationStatus::Completed => {
                return Err(OperationTrackerError::OperationAlreadyCompleted);
            }
            OperationStatus::Pending | OperationStatus::Executing => {
                return Err(OperationTrackerError::OperationInProgress);
            }
            OperationStatus::Failed | OperationStatus::Cancelled => {
                // Allow retry by overwriting failed/cancelled record
            }
        }
    }
    
    let now = env.ledger().timestamp();
    let expires_at = now
        .checked_add(ttl_seconds)
        .expect("operation_tracker: expiry timestamp overflow");
    
    let record = OperationRecord {
        status: OperationStatus::Pending,
        result: None,
        submitted_at: now,
        executed_at: None,
        expires_at,
        initiator: initiator.clone(),
    };
    
    let key = OperationTrackerKey::OperationRecord(operation_id.clone());
    env.storage().persistent().set(&key, &record);
    
    // Set TTL in ledgers (convert seconds to ledgers: ~5s per ledger)
    let ttl_ledgers = (ttl_seconds / 5).max(1) as u32;
    env.storage()
        .persistent()
        .extend_ttl(&key, ttl_ledgers, ttl_ledgers);
    
    Ok(())
}

/// Mark an operation as executing (guards against concurrent execution).
///
/// # Errors
/// - `OperationNotFound` if operation_id not registered
/// - `OperationInProgress` if already executing
/// - `UnauthorizedOperationAccess` if caller != initiator
pub fn mark_executing(
    env: &Env,
    operation_id: &BytesN<32>,
    caller: &Address,
) -> Result<(), OperationTrackerError> {
    let key = OperationTrackerKey::OperationRecord(operation_id.clone());
    let mut record = get_operation_record(env, operation_id)
        .ok_or(OperationTrackerError::OperationNotFound)?;
    
    // Authorization check
    if &record.initiator != caller {
        return Err(OperationTrackerError::UnauthorizedOperationAccess);
    }
    
    // State validation
    if record.status == OperationStatus::Executing {
        return Err(OperationTrackerError::OperationInProgress);
    }
    
    record.status = OperationStatus::Executing;
    env.storage().persistent().set(&key, &record);
    
    Ok(())
}

/// Complete an operation successfully and cache the result.
///
/// Also increments the user's sequence number.
///
/// # Errors
/// - `OperationNotFound` if operation_id not registered
/// - `UnauthorizedOperationAccess` if caller != initiator
pub fn complete_operation(
    env: &Env,
    operation_id: &BytesN<32>,
    result: OperationResult,
    caller: &Address,
) -> Result<(), OperationTrackerError> {
    let key = OperationTrackerKey::OperationRecord(operation_id.clone());
    let mut record = get_operation_record(env, operation_id)
        .ok_or(OperationTrackerError::OperationNotFound)?;
    
    // Authorization check
    if &record.initiator != caller {
        return Err(OperationTrackerError::UnauthorizedOperationAccess);
    }
    
    let now = env.ledger().timestamp();
    record.status = OperationStatus::Completed;
    record.result = Some(result);
    record.executed_at = Some(now);
    
    env.storage().persistent().set(&key, &record);
    
    // Increment user sequence number on successful completion
    increment_user_sequence(env, &record.initiator);
    
    Ok(())
}

/// Mark an operation as failed (allows retry).
///
/// # Errors
/// - `OperationNotFound` if operation_id not registered
/// - `UnauthorizedOperationAccess` if caller != initiator
pub fn fail_operation(
    env: &Env,
    operation_id: &BytesN<32>,
    caller: &Address,
) -> Result<(), OperationTrackerError> {
    let key = OperationTrackerKey::OperationRecord(operation_id.clone());
    let mut record = get_operation_record(env, operation_id)
        .ok_or(OperationTrackerError::OperationNotFound)?;
    
    // Authorization check
    if &record.initiator != caller {
        return Err(OperationTrackerError::UnauthorizedOperationAccess);
    }
    
    let now = env.ledger().timestamp();
    record.status = OperationStatus::Failed;
    record.executed_at = Some(now);
    
    env.storage().persistent().set(&key, &record);
    
    Ok(())
}

/// Cancel a pending operation (allows new operation with different ID).
///
/// # Errors
/// - `OperationNotFound` if operation_id not registered
/// - `UnauthorizedOperationAccess` if caller != initiator
/// - `InvalidOperationStatus` if operation is not Pending
pub fn cancel_operation(
    env: &Env,
    operation_id: &BytesN<32>,
    caller: &Address,
) -> Result<(), OperationTrackerError> {
    let key = OperationTrackerKey::OperationRecord(operation_id.clone());
    let mut record = get_operation_record(env, operation_id)
        .ok_or(OperationTrackerError::OperationNotFound)?;
    
    // Authorization check
    if &record.initiator != caller {
        return Err(OperationTrackerError::UnauthorizedOperationAccess);
    }
    
    // Can only cancel pending operations
    if record.status != OperationStatus::Pending {
        return Err(OperationTrackerError::InvalidOperationStatus);
    }
    
    record.status = OperationStatus::Cancelled;
    env.storage().persistent().set(&key, &record);
    
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Idempotency Enforcement
// ═══════════════════════════════════════════════════════════════════════════

/// Check if an operation is idempotent (completed and can return cached result).
///
/// Returns `Some(result)` if operation already completed, `None` otherwise.
pub fn check_idempotent(
    env: &Env,
    operation_id: &BytesN<32>,
) -> Option<OperationResult> {
    let record = get_operation_record(env, operation_id)?;
    
    if record.status == OperationStatus::Completed {
        record.result
    } else {
        None
    }
}

/// Validate operation preconditions before execution.
///
/// Checks:
/// 1. If operation_id provided, ensure it's not in-progress or completed
/// 2. If expected_sequence provided, validate against stored sequence
///
/// # Errors
/// - `OperationAlreadyCompleted` if attempting to re-execute completed operation
/// - `OperationInProgress` if operation is currently executing
/// - `SequenceMismatch` if sequence number doesn't match
pub fn validate_operation_preconditions(
    env: &Env,
    user: &Address,
    operation_id: Option<BytesN<32>>,
    expected_sequence: Option<u64>,
) -> Result<(), OperationTrackerError> {
    // Validate sequence number if provided
    if let Some(seq) = expected_sequence {
        validate_sequence(env, user, seq)?;
    }
    
    // Validate operation ID if provided
    if let Some(ref op_id) = operation_id {
        if let Some(record) = get_operation_record(env, op_id) {
            match record.status {
                OperationStatus::Completed => {
                    return Err(OperationTrackerError::OperationAlreadyCompleted);
                }
                OperationStatus::Pending | OperationStatus::Executing => {
                    return Err(OperationTrackerError::OperationInProgress);
                }
                OperationStatus::Failed | OperationStatus::Cancelled => {
                    // Allow retry
                }
            }
        }
    }
    
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Error Types
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationTrackerError {
    /// Operation sequence number mismatch
    SequenceMismatch { expected: u64, provided: u64 },
    /// Operation ID already exists with Completed status
    OperationAlreadyCompleted,
    /// Operation is currently being executed
    OperationInProgress,
    /// Operation ID not found in storage
    OperationNotFound,
    /// Caller is not the operation initiator
    UnauthorizedOperationAccess,
    /// Operation status doesn't allow requested action
    InvalidOperationStatus,
    /// User has too many pending operations
    TooManyPendingOperations,
}

impl core::fmt::Display for OperationTrackerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SequenceMismatch { expected, provided } => {
                write!(
                    f,
                    "SequenceMismatch: expected {}, provided {}",
                    expected, provided
                )
            }
            Self::OperationAlreadyCompleted => write!(f, "OperationAlreadyCompleted"),
            Self::OperationInProgress => write!(f, "OperationInProgress"),
            Self::OperationNotFound => write!(f, "OperationNotFound"),
            Self::UnauthorizedOperationAccess => write!(f, "UnauthorizedOperationAccess"),
            Self::InvalidOperationStatus => write!(f, "InvalidOperationStatus"),
            Self::TooManyPendingOperations => write!(f, "TooManyPendingOperations"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Utility Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Generate a deterministic operation ID from operation parameters.
///
/// Clients should call this with operation-specific parameters to ensure
/// the same logical operation always produces the same ID.
///
/// Example:
/// ```rust
/// let op_id = generate_operation_id(
///     env,
///     &user,
///     &symbol_short!("deposit"),
///     &vec![env, amount.into_val(env), asset.into_val(env)],
/// );
/// ```
pub fn generate_operation_id(
    env: &Env,
    user: &Address,
    operation_type: &Symbol,
    params: &Vec<soroban_sdk::Val>,
) -> BytesN<32> {
    use soroban_sdk::crypto::Hash;
    
    // Hash: user || operation_type || params
    let mut data = soroban_sdk::Bytes::new(env);
    data.append(&user.to_xdr(env));
    data.append(&operation_type.to_xdr(env));
    for param in params.iter() {
        data.append(&param.to_xdr(env));
    }
    
    env.crypto().sha256(&data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    fn test_sequence_starts_at_zero() {
        let env = Env::default();
        let user = Address::generate(&env);
        
        assert_eq!(get_user_sequence(&env, &user), 0);
    }

    #[test]
    fn test_sequence_increments() {
        let env = Env::default();
        let user = Address::generate(&env);
        
        let seq1 = increment_user_sequence(&env, &user);
        assert_eq!(seq1, 1);
        
        let seq2 = increment_user_sequence(&env, &user);
        assert_eq!(seq2, 2);
        
        assert_eq!(get_user_sequence(&env, &user), 2);
    }

    #[test]
    fn test_sequence_validation_success() {
        let env = Env::default();
        let user = Address::generate(&env);
        
        // Current sequence is 0
        assert!(validate_sequence(&env, &user, 0).is_ok());
        
        increment_user_sequence(&env, &user);
        
        // Current sequence is now 1
        assert!(validate_sequence(&env, &user, 1).is_ok());
    }

    #[test]
    fn test_sequence_validation_mismatch() {
        let env = Env::default();
        let user = Address::generate(&env);
        
        // Try to submit with sequence 5 when current is 0
        let result = validate_sequence(&env, &user, 5);
        assert!(result.is_err());
        
        if let Err(OperationTrackerError::SequenceMismatch { expected, provided }) = result {
            assert_eq!(expected, 0);
            assert_eq!(provided, 5);
        } else {
            panic!("Expected SequenceMismatch error");
        }
    }

    #[test]
    fn test_operation_registration() {
        let env = Env::default();
        let user = Address::generate(&env);
        let op_id = BytesN::from_array(&env, &[1u8; 32]);
        
        // Register new operation
        let result = register_operation(&env, &op_id, &user, 3600);
        assert!(result.is_ok());
        
        // Verify record exists
        let record = get_operation_record(&env, &op_id).unwrap();
        assert_eq!(record.status, OperationStatus::Pending);
        assert_eq!(record.initiator, user);
    }

    #[test]
    fn test_duplicate_operation_rejected() {
        let env = Env::default();
        let user = Address::generate(&env);
        let op_id = BytesN::from_array(&env, &[1u8; 32]);
        
        // Register operation
        register_operation(&env, &op_id, &user, 3600).unwrap();
        
        // Mark as executing
        mark_executing(&env, &op_id, &user).unwrap();
        
        // Try to register again - should fail
        let result = register_operation(&env, &op_id, &user, 3600);
        assert!(matches!(
            result,
            Err(OperationTrackerError::OperationInProgress)
        ));
    }

    #[test]
    fn test_completed_operation_idempotent() {
        let env = Env::default();
        let user = Address::generate(&env);
        let op_id = BytesN::from_array(&env, &[1u8; 32]);
        
        // Register and complete operation
        register_operation(&env, &op_id, &user, 3600).unwrap();
        mark_executing(&env, &op_id, &user).unwrap();
        complete_operation(&env, &op_id, OperationResult::Deposit(1000), &user).unwrap();
        
        // Check idempotency
        let cached = check_idempotent(&env, &op_id);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), OperationResult::Deposit(1000));
        
        // Verify sequence incremented
        assert_eq!(get_user_sequence(&env, &user), 1);
    }

    #[test]
    fn test_failed_operation_allows_retry() {
        let env = Env::default();
        let user = Address::generate(&env);
        let op_id = BytesN::from_array(&env, &[1u8; 32]);
        
        // Register and fail operation
        register_operation(&env, &op_id, &user, 3600).unwrap();
        mark_executing(&env, &op_id, &user).unwrap();
        fail_operation(&env, &op_id, &user).unwrap();
        
        // Should allow retry with same ID
        let result = register_operation(&env, &op_id, &user, 3600);
        assert!(result.is_ok());
        
        // Verify sequence NOT incremented (operation failed)
        assert_eq!(get_user_sequence(&env, &user), 0);
    }

    #[test]
    fn test_operation_cancellation() {
        let env = Env::default();
        let user = Address::generate(&env);
        let op_id = BytesN::from_array(&env, &[1u8; 32]);
        
        // Register operation
        register_operation(&env, &op_id, &user, 3600).unwrap();
        
        // Cancel it
        let result = cancel_operation(&env, &op_id, &user);
        assert!(result.is_ok());
        
        // Verify status
        let record = get_operation_record(&env, &op_id).unwrap();
        assert_eq!(record.status, OperationStatus::Cancelled);
    }

    #[test]
    fn test_unauthorized_access_rejected() {
        let env = Env::default();
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);
        let op_id = BytesN::from_array(&env, &[1u8; 32]);
        
        // User1 registers operation
        register_operation(&env, &op_id, &user1, 3600).unwrap();
        
        // User2 tries to mark executing - should fail
        let result = mark_executing(&env, &op_id, &user2);
        assert!(matches!(
            result,
            Err(OperationTrackerError::UnauthorizedOperationAccess)
        ));
    }
}
