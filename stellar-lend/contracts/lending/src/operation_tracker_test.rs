//! Comprehensive tests for operation tracking and idempotency enforcement.

#![cfg(test)]

use crate::operation_tracker::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, BytesN, Env, Vec};

// ═══════════════════════════════════════════════════════════════════════════
// Sequence Number Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_sequence_starts_at_zero_for_new_user() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    assert_eq!(get_user_sequence(&env, &user), 0);
}

#[test]
fn test_sequence_increments_correctly() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    assert_eq!(get_user_sequence(&env, &user), 0);
    
    let seq1 = increment_user_sequence(&env, &user);
    assert_eq!(seq1, 1);
    assert_eq!(get_user_sequence(&env, &user), 1);
    
    let seq2 = increment_user_sequence(&env, &user);
    assert_eq!(seq2, 2);
    assert_eq!(get_user_sequence(&env, &user), 2);
}

#[test]
fn test_sequence_validation_success() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    // Validate sequence 0 (initial state)
    assert!(validate_sequence(&env, &user, 0).is_ok());
    
    increment_user_sequence(&env, &user);
    
    // Validate sequence 1 (after increment)
    assert!(validate_sequence(&env, &user, 1).is_ok());
}

#[test]
fn test_sequence_validation_mismatch() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    // Try to validate future sequence
    let result = validate_sequence(&env, &user, 5);
    assert!(result.is_err());
    
    match result {
        Err(OperationTrackerError::SequenceMismatch { expected, provided }) => {
            assert_eq!(expected, 0);
            assert_eq!(provided, 5);
        }
        _ => panic!("Expected SequenceMismatch error"),
    }
}

#[test]
fn test_sequence_prevents_double_execution() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    // First operation with sequence 0
    assert!(validate_sequence(&env, &user, 0).is_ok());
    increment_user_sequence(&env, &user);
    
    // Attempt to repeat operation with sequence 0 (should fail)
    let result = validate_sequence(&env, &user, 0);
    assert!(result.is_err());
    
    match result {
        Err(OperationTrackerError::SequenceMismatch { expected, provided }) => {
            assert_eq!(expected, 1);
            assert_eq!(provided, 0);
        }
        _ => panic!("Expected SequenceMismatch error"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Operation Registration Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_register_new_operation() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[1u8; 32]);
    
    let result = register_operation(&env, &op_id, &user, 3600);
    assert!(result.is_ok());
    
    // Verify record exists and has correct state
    let record = get_operation_record(&env, &op_id).unwrap();
    assert_eq!(record.status, OperationStatus::Pending);
    assert_eq!(record.initiator, user);
    assert!(record.result.is_none());
    assert!(record.executed_at.is_none());
}

#[test]
fn test_register_duplicate_pending_operation_fails() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[1u8; 32]);
    
    // Register first time
    register_operation(&env, &op_id, &user, 3600).unwrap();
    
    // Try to register again (should fail)
    let result = register_operation(&env, &op_id, &user, 3600);
    assert!(matches!(
        result,
        Err(OperationTrackerError::OperationInProgress)
    ));
}

#[test]
fn test_register_after_failed_operation_succeeds() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[2u8; 32]);
    
    // Register, mark executing, then fail
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    fail_operation(&env, &op_id, &user).unwrap();
    
    // Should allow retry (register again with same ID)
    let result = register_operation(&env, &op_id, &user, 3600);
    assert!(result.is_ok());
    
    // Verify record is now Pending again
    let record = get_operation_record(&env, &op_id).unwrap();
    assert_eq!(record.status, OperationStatus::Pending);
}

// ═══════════════════════════════════════════════════════════════════════════
// Operation Status Transition Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_mark_executing_from_pending() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[3u8; 32]);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    
    let result = mark_executing(&env, &op_id, &user);
    assert!(result.is_ok());
    
    let record = get_operation_record(&env, &op_id).unwrap();
    assert_eq!(record.status, OperationStatus::Executing);
}

#[test]
fn test_mark_executing_twice_fails() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[4u8; 32]);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    
    // Try to mark executing again
    let result = mark_executing(&env, &op_id, &user);
    assert!(matches!(
        result,
        Err(OperationTrackerError::OperationInProgress)
    ));
}

#[test]
fn test_complete_operation_increments_sequence() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[5u8; 32]);
    
    let initial_seq = get_user_sequence(&env, &user);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    complete_operation(&env, &op_id, OperationResult::Deposit(1000), &user).unwrap();
    
    // Verify sequence incremented
    assert_eq!(get_user_sequence(&env, &user), initial_seq + 1);
    
    // Verify record has completed status
    let record = get_operation_record(&env, &op_id).unwrap();
    assert_eq!(record.status, OperationStatus::Completed);
    assert_eq!(record.result, Some(OperationResult::Deposit(1000)));
    assert!(record.executed_at.is_some());
}

#[test]
fn test_fail_operation_does_not_increment_sequence() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[6u8; 32]);
    
    let initial_seq = get_user_sequence(&env, &user);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    fail_operation(&env, &op_id, &user).unwrap();
    
    // Verify sequence NOT incremented
    assert_eq!(get_user_sequence(&env, &user), initial_seq);
    
    // Verify record has failed status
    let record = get_operation_record(&env, &op_id).unwrap();
    assert_eq!(record.status, OperationStatus::Failed);
}

#[test]
fn test_cancel_pending_operation() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[7u8; 32]);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    
    let result = cancel_operation(&env, &op_id, &user);
    assert!(result.is_ok());
    
    let record = get_operation_record(&env, &op_id).unwrap();
    assert_eq!(record.status, OperationStatus::Cancelled);
}

#[test]
fn test_cannot_cancel_executing_operation() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[8u8; 32]);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    
    let result = cancel_operation(&env, &op_id, &user);
    assert!(matches!(
        result,
        Err(OperationTrackerError::InvalidOperationStatus)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Idempotency Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_check_idempotent_returns_cached_result() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[9u8; 32]);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    complete_operation(&env, &op_id, OperationResult::Borrow(5000), &user).unwrap();
    
    // Check idempotency
    let cached = check_idempotent(&env, &op_id);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap(), OperationResult::Borrow(5000));
}

#[test]
fn test_check_idempotent_returns_none_for_pending() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[10u8; 32]);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    
    let cached = check_idempotent(&env, &op_id);
    assert!(cached.is_none());
}

#[test]
fn test_validate_operation_preconditions_rejects_completed() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[11u8; 32]);
    
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    complete_operation(&env, &op_id, OperationResult::Success, &user).unwrap();
    
    // Try to start another operation with same ID
    let result = validate_operation_preconditions(&env, &user, Some(op_id), None);
    assert!(matches!(
        result,
        Err(OperationTrackerError::OperationAlreadyCompleted)
    ));
}

#[test]
fn test_validate_operation_preconditions_checks_sequence() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    // Try to submit operation with wrong sequence
    let result = validate_operation_preconditions(&env, &user, None, Some(5));
    assert!(matches!(
        result,
        Err(OperationTrackerError::SequenceMismatch { .. })
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Authorization Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_unauthorized_user_cannot_mark_executing() {
    let env = Env::default();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[12u8; 32]);
    
    // User1 registers operation
    register_operation(&env, &op_id, &user1, 3600).unwrap();
    
    // User2 tries to mark executing
    let result = mark_executing(&env, &op_id, &user2);
    assert!(matches!(
        result,
        Err(OperationTrackerError::UnauthorizedOperationAccess)
    ));
}

#[test]
fn test_unauthorized_user_cannot_complete_operation() {
    let env = Env::default();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[13u8; 32]);
    
    register_operation(&env, &op_id, &user1, 3600).unwrap();
    mark_executing(&env, &op_id, &user1).unwrap();
    
    // User2 tries to complete
    let result = complete_operation(&env, &op_id, OperationResult::Success, &user2);
    assert!(matches!(
        result,
        Err(OperationTrackerError::UnauthorizedOperationAccess)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Operation ID Generation Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_generate_operation_id_is_deterministic() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_type = symbol_short!("deposit");
    
    let params1 = Vec::from_array(&env, [1000.into_val(&env), user.clone().into_val(&env)]);
    let params2 = Vec::from_array(&env, [1000.into_val(&env), user.clone().into_val(&env)]);
    
    let id1 = generate_operation_id(&env, &user, &op_type, &params1);
    let id2 = generate_operation_id(&env, &user, &op_type, &params2);
    
    // Same inputs should produce same ID
    assert_eq!(id1, id2);
}

#[test]
fn test_generate_operation_id_different_for_different_inputs() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_type = symbol_short!("deposit");
    
    let params1 = Vec::from_array(&env, [1000.into_val(&env)]);
    let params2 = Vec::from_array(&env, [2000.into_val(&env)]);
    
    let id1 = generate_operation_id(&env, &user, &op_type, &params1);
    let id2 = generate_operation_id(&env, &user, &op_type, &params2);
    
    // Different amounts should produce different IDs
    assert_ne!(id1, id2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Retry Scenario Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_retry_after_network_timeout_with_operation_id() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[14u8; 32]);
    
    // Simulate: operation submitted, completed, but response lost
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    complete_operation(&env, &op_id, OperationResult::Deposit(1000), &user).unwrap();
    
    // Client retries with same operation_id
    // Should get cached result instead of executing again
    let cached = check_idempotent(&env, &op_id);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap(), OperationResult::Deposit(1000));
    
    // Verify sequence incremented only once
    assert_eq!(get_user_sequence(&env, &user), 1);
}

#[test]
fn test_retry_failed_operation_with_same_id() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[15u8; 32]);
    
    // First attempt fails
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    fail_operation(&env, &op_id, &user).unwrap();
    
    // Retry with same operation_id should be allowed
    let result = register_operation(&env, &op_id, &user, 3600);
    assert!(result.is_ok());
    
    // Second attempt succeeds
    mark_executing(&env, &op_id, &user).unwrap();
    complete_operation(&env, &op_id, OperationResult::Deposit(1000), &user).unwrap();
    
    // Verify sequence incremented only for successful completion
    assert_eq!(get_user_sequence(&env, &user), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Concurrent Operation Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_sequence_prevents_concurrent_operations() {
    let env = Env::default();
    let user = Address::generate(&env);
    
    // User submits two operations concurrently, both with sequence 0
    
    // First operation validates and executes
    assert!(validate_sequence(&env, &user, 0).is_ok());
    increment_user_sequence(&env, &user);
    
    // Second operation validates with stale sequence (should fail)
    let result = validate_sequence(&env, &user, 0);
    assert!(result.is_err());
    
    match result {
        Err(OperationTrackerError::SequenceMismatch { expected, provided }) => {
            assert_eq!(expected, 1);
            assert_eq!(provided, 0);
        }
        _ => panic!("Expected SequenceMismatch"),
    }
}

#[test]
fn test_operation_id_prevents_duplicate_submissions() {
    let env = Env::default();
    let user = Address::generate(&env);
    let op_id = BytesN::from_array(&env, &[16u8; 32]);
    
    // First submission registers successfully
    register_operation(&env, &op_id, &user, 3600).unwrap();
    mark_executing(&env, &op_id, &user).unwrap();
    
    // Second submission with same ID detects in-progress operation
    let result = register_operation(&env, &op_id, &user, 3600);
    assert!(matches!(
        result,
        Err(OperationTrackerError::OperationInProgress)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Multi-User Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_different_users_have_independent_sequences() {
    let env = Env::default();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    
    // User1 increments sequence
    increment_user_sequence(&env, &user1);
    increment_user_sequence(&env, &user1);
    
    // User2's sequence should still be 0
    assert_eq!(get_user_sequence(&env, &user1), 2);
    assert_eq!(get_user_sequence(&env, &user2), 0);
}

#[test]
fn test_different_users_can_use_same_operation_id() {
    let env = Env::default();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    
    // Use same operation ID bytes for different users
    let op_id = BytesN::from_array(&env, &[17u8; 32]);
    
    // Both users can register operations with same ID
    // (operation IDs are scoped to initiator)
    register_operation(&env, &op_id, &user1, 3600).unwrap();
    // Note: In current implementation, operation IDs are global.
    // This test documents that behavior. For per-user scoping,
    // DataKey would need to be OperationRecord(user, op_id).
    
    let result = register_operation(&env, &op_id, &user2, 3600);
    // Currently fails because operation ID is already registered
    assert!(result.is_err());
}
