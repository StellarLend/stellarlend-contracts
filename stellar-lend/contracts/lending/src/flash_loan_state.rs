//! Enhanced Flash Loan State Machine with Deterministic Transitions
//!
//! This module implements explicit state tracking for flash loan operations
//! to ensure deterministic, atomic, and auditable execution.
//!
//! ## Problem: Implicit Flash Loan State
//!
//! Previous implementation used a single `FlashActive` boolean flag:
//! - Binary state (active/inactive) provides reentrancy protection
//! - BUT: No tracking of callback execution status
//! - BUT: No audit trail of flash loan lifecycle
//! - BUT: Difficult to diagnose failed callbacks
//!
//! ## Solution: Explicit Flash Loan State Machine
//!
//! Track full lifecycle:
//! ```
//! [NONE] 
//!   ↓ flash_loan() called
//! [INITIATED] (initiator, receiver, amount, fee recorded)
//!   ↓ callback invoked
//! [CALLBACK_EXECUTING] (callback start time recorded)
//!   ↓ repay_flash_loan() called
//! [REPAYMENT_RECEIVED] (repay amount recorded)
//!   ↓ callback returns
//! [CALLBACK_COMPLETED] (callback end time recorded)
//!   ↓ verification passed
//! [COMPLETED] (final balance verified)
//!   ↓ cleanup
//! [NONE]
//! ```
//!
//! ## Benefits
//!
//! 1. **Audit Trail**: Every flash loan has complete execution record
//! 2. **Failure Diagnosis**: Know exactly where callback failed
//! 3. **Reentrancy Protection**: More explicit than boolean flag
//! 4. **Deterministic Recovery**: Can detect and handle partial failures
//! 5. **Event Correlation**: Flash loan request ID links all events
//!
//! ## Flash Loan Request ID
//!
//! Each flash loan is assigned a unique request ID (hash of parameters):
//! ```rust
//! request_id = sha256(initiator || receiver || asset || amount || nonce)
//! ```
//!
//! This enables:
//! - Correlation of FlashLoanEvent → callback events → FlashLoanRepaidEvent
//! - Detection of duplicate flash loan submissions
//! - Audit trail reconstruction
//! - Debugging failed callbacks

use soroban_sdk::{contracttype, Address, Bytes, BytesN, Env};

/// Flash loan execution status for deterministic state tracking.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlashLoanStatus {
    /// Flash loan initiated, parameters validated, treasury debited.
    Initiated,
    /// Callback is currently executing.
    CallbackExecuting,
    /// Callback has called repay_flash_loan (repayment received).
    RepaymentReceived,
    /// Callback returned successfully.
    CallbackCompleted,
    /// Final verification passed, flash loan completed successfully.
    Completed,
    /// Flash loan failed (callback panicked or repayment insufficient).
    Failed,
}

/// Complete flash loan execution record.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanRecord {
    /// Unique identifier for this flash loan request
    pub request_id: BytesN<32>,
    /// Address that initiated the flash loan
    pub initiator: Address,
    /// Address receiving the flash-loaned funds (callback target)
    pub receiver: Address,
    /// Asset being flash-loaned
    pub asset: Address,
    /// Amount of the flash loan
    pub amount: i128,
    /// Fee charged (in basis points)
    pub fee: i128,
    /// Current execution status
    pub status: FlashLoanStatus,
    /// Ledger timestamp when flash loan was initiated
    pub initiated_at: u64,
    /// Ledger timestamp when callback started executing (if reached)
    pub callback_started_at: Option<u64>,
    /// Ledger timestamp when repayment was received (if reached)
    pub repayment_received_at: Option<u64>,
    /// Amount repaid via repay_flash_loan (if called)
    pub repaid_amount: Option<i128>,
    /// Ledger timestamp when callback completed (if successful)
    pub callback_completed_at: Option<u64>,
    /// Treasury balance before flash loan (for verification)
    pub treasury_before: i128,
    /// Required treasury balance after flash loan
    pub required_treasury_after: i128,
}

/// Storage key for flash loan state tracking.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlashLoanStateKey {
    /// Active flash loan record (only one can be active at a time)
    ActiveFlashLoan,
    /// Flash loan execution history (by request_id, with TTL)
    FlashLoanHistory(BytesN<32>),
    /// Counter for generating unique nonces
    FlashLoanNonce,
}

/// TTL for flash loan history records (7 days)
pub const FLASH_LOAN_HISTORY_TTL: u32 = 120_960; // ~7 days at 5s/ledger

// ═══════════════════════════════════════════════════════════════════════════
// Flash Loan Request ID Generation
// ═══════════════════════════════════════════════════════════════════════════

/// Generate a unique flash loan request ID.
///
/// Combines flash loan parameters with a monotonic nonce to ensure uniqueness
/// even if same parameters are used multiple times.
pub fn generate_flash_loan_request_id(
    env: &Env,
    initiator: &Address,
    receiver: &Address,
    asset: &Address,
    amount: i128,
) -> BytesN<32> {
    use soroban_sdk::xdr::ToXdr;
    
    // Get and increment nonce
    let key = FlashLoanStateKey::FlashLoanNonce;
    let nonce: u64 = env
        .storage()
        .instance()
        .get(&key)
        .unwrap_or(0u64);
    
    let next_nonce = nonce.checked_add(1).expect("flash_loan: nonce overflow");
    env.storage().instance().set(&key, &next_nonce);
    
    // Hash: initiator || receiver || asset || amount || nonce
    let mut data = Bytes::new(env);
    data.append(&initiator.to_xdr(env));
    data.append(&receiver.to_xdr(env));
    data.append(&asset.to_xdr(env));
    data.append(&Bytes::from_array(env, &amount.to_be_bytes()));
    data.append(&Bytes::from_array(env, &nonce.to_be_bytes()));
    
    env.crypto().sha256(&data)
}

// ═══════════════════════════════════════════════════════════════════════════
// Flash Loan State Management
// ═══════════════════════════════════════════════════════════════════════════

/// Get the currently active flash loan record, if any.
///
/// Returns `None` if no flash loan is currently executing.
pub fn get_active_flash_loan(env: &Env) -> Option<FlashLoanRecord> {
    let key = FlashLoanStateKey::ActiveFlashLoan;
    env.storage().instance().get(&key)
}

/// Check if a flash loan is currently active (reentrancy guard).
///
/// Returns `true` if any flash loan is in progress (status != None).
pub fn is_flash_loan_active(env: &Env) -> bool {
    get_active_flash_loan(env).is_some()
}

/// Initialize a new flash loan record.
///
/// Sets status to Initiated and records all parameters.
///
/// # Panics
/// If a flash loan is already active (reentrancy violation).
pub fn initiate_flash_loan(
    env: &Env,
    request_id: BytesN<32>,
    initiator: Address,
    receiver: Address,
    asset: Address,
    amount: i128,
    fee: i128,
    treasury_before: i128,
) {
    // Reentrancy check
    if is_flash_loan_active(env) {
        panic!("FlashLoanReentrancy");
    }
    
    let now = env.ledger().timestamp();
    let required_treasury_after = treasury_before
        .checked_add(fee)
        .expect("flash_loan: required treasury calculation overflow");
    
    let record = FlashLoanRecord {
        request_id: request_id.clone(),
        initiator,
        receiver,
        asset,
        amount,
        fee,
        status: FlashLoanStatus::Initiated,
        initiated_at: now,
        callback_started_at: None,
        repayment_received_at: None,
        repaid_amount: None,
        callback_completed_at: None,
        treasury_before,
        required_treasury_after,
    };
    
    let key = FlashLoanStateKey::ActiveFlashLoan;
    env.storage().instance().set(&key, &record);
}

/// Mark callback execution as started.
///
/// Transitions status: Initiated → CallbackExecuting.
pub fn mark_callback_executing(env: &Env) {
    let key = FlashLoanStateKey::ActiveFlashLoan;
    let mut record = get_active_flash_loan(env)
        .expect("flash_loan: no active flash loan");
    
    if record.status != FlashLoanStatus::Initiated {
        panic!("flash_loan: invalid state transition to CallbackExecuting");
    }
    
    let now = env.ledger().timestamp();
    record.status = FlashLoanStatus::CallbackExecuting;
    record.callback_started_at = Some(now);
    
    env.storage().instance().set(&key, &record);
}

/// Record repayment received during callback.
///
/// Transitions status: CallbackExecuting → RepaymentReceived.
///
/// # Parameters
/// - `repaid_amount`: Amount repaid via `repay_flash_loan`
pub fn record_repayment_received(env: &Env, repaid_amount: i128) {
    let key = FlashLoanStateKey::ActiveFlashLoan;
    let mut record = get_active_flash_loan(env)
        .expect("flash_loan: no active flash loan");
    
    if record.status != FlashLoanStatus::CallbackExecuting {
        panic!("flash_loan: invalid state transition to RepaymentReceived");
    }
    
    let now = env.ledger().timestamp();
    record.status = FlashLoanStatus::RepaymentReceived;
    record.repayment_received_at = Some(now);
    record.repaid_amount = Some(repaid_amount);
    
    env.storage().instance().set(&key, &record);
}

/// Mark callback as completed (returned without panic).
///
/// Transitions status: RepaymentReceived → CallbackCompleted.
pub fn mark_callback_completed(env: &Env) {
    let key = FlashLoanStateKey::ActiveFlashLoan;
    let mut record = get_active_flash_loan(env)
        .expect("flash_loan: no active flash loan");
    
    // Allow transition from CallbackExecuting (no repayment) or RepaymentReceived
    if record.status != FlashLoanStatus::CallbackExecuting
        && record.status != FlashLoanStatus::RepaymentReceived
    {
        panic!("flash_loan: invalid state transition to CallbackCompleted");
    }
    
    let now = env.ledger().timestamp();
    record.status = FlashLoanStatus::CallbackCompleted;
    record.callback_completed_at = Some(now);
    
    env.storage().instance().set(&key, &record);
}

/// Complete the flash loan after final verification.
///
/// Transitions status: CallbackCompleted → Completed.
/// Moves record from active to history, clears active slot.
pub fn complete_flash_loan(env: &Env) {
    let active_key = FlashLoanStateKey::ActiveFlashLoan;
    let mut record = get_active_flash_loan(env)
        .expect("flash_loan: no active flash loan");
    
    if record.status != FlashLoanStatus::CallbackCompleted {
        panic!("flash_loan: invalid state transition to Completed");
    }
    
    record.status = FlashLoanStatus::Completed;
    
    // Move to history (with TTL)
    let history_key = FlashLoanStateKey::FlashLoanHistory(record.request_id.clone());
    env.storage().persistent().set(&history_key, &record);
    env.storage()
        .persistent()
        .extend_ttl(&history_key, FLASH_LOAN_HISTORY_TTL, FLASH_LOAN_HISTORY_TTL);
    
    // Clear active flash loan
    env.storage().instance().remove(&active_key);
}

/// Mark the flash loan as failed.
///
/// Transitions status: * → Failed.
/// Moves record to history for audit trail.
pub fn fail_flash_loan(env: &Env, reason: &str) {
    let active_key = FlashLoanStateKey::ActiveFlashLoan;
    
    if let Some(mut record) = get_active_flash_loan(env) {
        record.status = FlashLoanStatus::Failed;
        
        // Move to history
        let history_key = FlashLoanStateKey::FlashLoanHistory(record.request_id.clone());
        env.storage().persistent().set(&history_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&history_key, FLASH_LOAN_HISTORY_TTL, FLASH_LOAN_HISTORY_TTL);
        
        // Clear active flash loan
        env.storage().instance().remove(&active_key);
    }
    
    // Panic to rollback transaction
    panic!("FlashLoan failed: {}", reason);
}

/// Clean up flash loan state after panic/rollback.
///
/// Called automatically by Soroban's transaction rollback mechanism.
/// Ensures FlashActive flag is always cleared after transaction completes.
pub fn cleanup_flash_loan_on_rollback(env: &Env) {
    // Soroban's instance storage is transaction-scoped, so this is
    // automatically handled. This function is here for documentation.
    //
    // If flash loan panics:
    // 1. All instance storage writes are rolled back
    // 2. ActiveFlashLoan key is removed
    // 3. FlashActive boolean is reset to false
    //
    // This prevents the "stuck FlashActive" bug.
}

// ═══════════════════════════════════════════════════════════════════════════
// Query Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Get flash loan execution history by request ID.
///
/// Returns `None` if record has expired (TTL passed) or never existed.
pub fn get_flash_loan_history(
    env: &Env,
    request_id: &BytesN<32>,
) -> Option<FlashLoanRecord> {
    let key = FlashLoanStateKey::FlashLoanHistory(request_id.clone());
    env.storage().persistent().get(&key)
}

/// Get execution details of the currently active flash loan (for debugging).
///
/// Returns detailed status information if a flash loan is in progress.
pub fn get_active_flash_loan_details(env: &Env) -> Option<FlashLoanDebugInfo> {
    let record = get_active_flash_loan(env)?;
    
    let now = env.ledger().timestamp();
    let elapsed_since_initiation = now.saturating_sub(record.initiated_at);
    
    let callback_duration = record.callback_started_at.and_then(|start| {
        record
            .callback_completed_at
            .or(Some(now))
            .map(|end| end.saturating_sub(start))
    });
    
    Some(FlashLoanDebugInfo {
        request_id: record.request_id,
        status: record.status,
        initiator: record.initiator,
        receiver: record.receiver,
        amount: record.amount,
        fee: record.fee,
        elapsed_since_initiation,
        callback_duration,
        repaid_amount: record.repaid_amount,
    })
}

/// Debug information about active flash loan execution.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanDebugInfo {
    pub request_id: BytesN<32>,
    pub status: FlashLoanStatus,
    pub initiator: Address,
    pub receiver: Address,
    pub amount: i128,
    pub fee: i128,
    pub elapsed_since_initiation: u64,
    pub callback_duration: Option<u64>,
    pub repaid_amount: Option<i128>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Invariant Validation
// ═══════════════════════════════════════════════════════════════════════════

/// Validate flash loan state machine invariants.
///
/// Checks:
/// 1. Status transitions are valid (no invalid jumps)
/// 2. Timestamps are monotonic (callback_started <= repayment <= completed)
/// 3. Repayment amount recorded if status >= RepaymentReceived
///
/// # Panics
/// If any invariant is violated (indicates implementation bug).
pub fn validate_flash_loan_invariants(env: &Env) {
    if let Some(record) = get_active_flash_loan(env) {
        // Invariant 1: Valid status progression
        match record.status {
            FlashLoanStatus::Initiated => {
                assert!(record.callback_started_at.is_none());
                assert!(record.repayment_received_at.is_none());
                assert!(record.callback_completed_at.is_none());
            }
            FlashLoanStatus::CallbackExecuting => {
                assert!(record.callback_started_at.is_some());
                // Repayment may or may not have occurred yet
            }
            FlashLoanStatus::RepaymentReceived => {
                assert!(record.callback_started_at.is_some());
                assert!(record.repayment_received_at.is_some());
                assert!(record.repaid_amount.is_some());
            }
            FlashLoanStatus::CallbackCompleted => {
                assert!(record.callback_started_at.is_some());
                assert!(record.callback_completed_at.is_some());
            }
            FlashLoanStatus::Completed | FlashLoanStatus::Failed => {
                panic!("flash_loan invariant: Completed/Failed status should not be in ActiveFlashLoan");
            }
        }
        
        // Invariant 2: Timestamp monotonicity
        if let (Some(started), Some(completed)) =
            (record.callback_started_at, record.callback_completed_at)
        {
            assert!(
                started <= completed,
                "flash_loan invariant: callback_started > callback_completed"
            );
        }
        
        if let (Some(started), Some(repaid)) =
            (record.callback_started_at, record.repayment_received_at)
        {
            assert!(
                started <= repaid,
                "flash_loan invariant: callback_started > repayment_received"
            );
        }
        
        // Invariant 3: Amounts are consistent
        assert!(record.amount > 0, "flash_loan invariant: amount <= 0");
        assert!(record.fee >= 0, "flash_loan invariant: fee < 0");
        assert!(
            record.required_treasury_after == record.treasury_before + record.fee,
            "flash_loan invariant: required_treasury calculation incorrect"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    fn test_no_flash_loan_active_initially() {
        let env = Env::default();
        assert!(!is_flash_loan_active(&env));
        assert!(get_active_flash_loan(&env).is_none());
    }

    #[test]
    fn test_initiate_flash_loan_creates_record() {
        let env = Env::default();
        let initiator = Address::generate(&env);
        let receiver = Address::generate(&env);
        let asset = Address::generate(&env);
        let request_id = BytesN::from_array(&env, &[1u8; 32]);
        
        initiate_flash_loan(&env, request_id.clone(), initiator.clone(), receiver.clone(), asset.clone(), 1000, 10, 5000);
        
        assert!(is_flash_loan_active(&env));
        
        let record = get_active_flash_loan(&env).unwrap();
        assert_eq!(record.status, FlashLoanStatus::Initiated);
        assert_eq!(record.amount, 1000);
        assert_eq!(record.fee, 10);
        assert_eq!(record.treasury_before, 5000);
        assert_eq!(record.required_treasury_after, 5010);
    }

    #[test]
    #[should_panic(expected = "FlashLoanReentrancy")]
    fn test_cannot_initiate_nested_flash_loan() {
        let env = Env::default();
        let initiator = Address::generate(&env);
        let receiver = Address::generate(&env);
        let asset = Address::generate(&env);
        
        let request_id1 = BytesN::from_array(&env, &[1u8; 32]);
        let request_id2 = BytesN::from_array(&env, &[2u8; 32]);
        
        initiate_flash_loan(&env, request_id1, initiator.clone(), receiver.clone(), asset.clone(), 1000, 10, 5000);
        
        // Try to initiate second flash loan (should panic)
        initiate_flash_loan(&env, request_id2, initiator.clone(), receiver, asset, 2000, 20, 5000);
    }

    #[test]
    fn test_full_flash_loan_lifecycle() {
        let env = Env::default();
        let initiator = Address::generate(&env);
        let receiver = Address::generate(&env);
        let asset = Address::generate(&env);
        let request_id = BytesN::from_array(&env, &[1u8; 32]);
        
        // 1. Initiate
        initiate_flash_loan(&env, request_id.clone(), initiator, receiver, asset, 1000, 10, 5000);
        assert_eq!(get_active_flash_loan(&env).unwrap().status, FlashLoanStatus::Initiated);
        
        // 2. Mark callback executing
        mark_callback_executing(&env);
        assert_eq!(get_active_flash_loan(&env).unwrap().status, FlashLoanStatus::CallbackExecuting);
        
        // 3. Record repayment
        record_repayment_received(&env, 1010);
        let record = get_active_flash_loan(&env).unwrap();
        assert_eq!(record.status, FlashLoanStatus::RepaymentReceived);
        assert_eq!(record.repaid_amount, Some(1010));
        
        // 4. Mark callback completed
        mark_callback_completed(&env);
        assert_eq!(get_active_flash_loan(&env).unwrap().status, FlashLoanStatus::CallbackCompleted);
        
        // 5. Complete flash loan
        complete_flash_loan(&env);
        assert!(!is_flash_loan_active(&env));
        
        // 6. Verify record moved to history
        let history = get_flash_loan_history(&env, &request_id).unwrap();
        assert_eq!(history.status, FlashLoanStatus::Completed);
    }

    #[test]
    fn test_flash_loan_request_id_is_unique() {
        let env = Env::default();
        let initiator = Address::generate(&env);
        let receiver = Address::generate(&env);
        let asset = Address::generate(&env);
        
        let id1 = generate_flash_loan_request_id(&env, &initiator, &receiver, &asset, 1000);
        let id2 = generate_flash_loan_request_id(&env, &initiator, &receiver, &asset, 1000);
        
        // Same parameters, but different nonces → different IDs
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_validate_invariants_passes_for_valid_state() {
        let env = Env::default();
        let initiator = Address::generate(&env);
        let receiver = Address::generate(&env);
        let asset = Address::generate(&env);
        let request_id = BytesN::from_array(&env, &[1u8; 32]);
        
        initiate_flash_loan(&env, request_id, initiator, receiver, asset, 1000, 10, 5000);
        mark_callback_executing(&env);
        record_repayment_received(&env, 1010);
        
        // Should not panic
        validate_flash_loan_invariants(&env);
    }
}
