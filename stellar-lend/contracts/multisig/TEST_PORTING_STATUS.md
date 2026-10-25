# Multisig Test Module Porting Status

This document tracks the status of wiring orphaned test modules into the multisig crate.

## ✅ Already Wired (Working)

These test modules are declared in `lib.rs` and passing:

- `execution_router_test.rs` - InvokeContract routing tests
- `batch_execute_test.rs` - Batch proposal execution tests
- `cancel_proposal_test.rs` - Proposal cancellation tests
- `approval_binding_test.rs` - Approval binding and nonce tests
- `signer_shrink_guard_test.rs` - Quorum bricking prevention
- `replay_protection_test.rs` - Replay attack prevention

## ✅ Newly Wired (Needs Minor Updates)

These modules are now declared in `lib.rs` but may need minor API updates:

### `quorum_edge_test.rs` - Status: **Needs API Update**
- Tests: Duplicate approval counting, threshold enforcement
- Issues:
  - References `MIN_THRESHOLD_DELAY_LEDGERS` constant (not in current API)
  - Uses `InsufficientApprovals` error (should be `QuorumNotReached`)
  - Uses `initialize(&admin, &threshold)` (current: `initialize(&signers, &threshold)`)
  - Uses `set_signers()` method (signers now set in initialize)
- Fix: Update to current API patterns from `execution_router_test.rs`

### `upgrade_e2e_test.rs` - Status: **Needs API Update**
- Tests: Full proposal lifecycle (create → approve → execute)
- Issues: Same as `quorum_edge_test.rs`
- Fix: Update initialization pattern and error variants

### `invoke_contract_with_args_test.rs` - Status: **Nearly Complete**
- Tests: InvokeContract with typed arguments
- Defines mock `TargetContract` inline
- Issues: Minor - may need `create_proposal` API updates
- Fix: Verify argument encoding matches current `ProposalAction::InvokeContract`

### `cross_contract_error_test.rs` - Status: **Major Rewrite Needed**
- Tests: Error propagation from InvokeContract targets
- Issues:
  - Uses placeholder helper functions (`test_create_cross_contract_proposal`, etc.)
  - References `is_nonce_consumed()` method (need to check if this exists)
  - Mock contract behavior setup not implemented
- Fix: Implement real test helpers using patterns from `execution_router_test.rs`

## ❌ Not Wired (Out of Scope or Deprecated)

### `boundary_conditions_test.rs` - **Placeholder Code**
- Contains comment: "In real tests, you would deploy the contract here"
- Uses `Address::random()` instead of actual contract deployment
- Recommendation: Delete or completely rewrite with real deployments

### `action_allowlist_test.rs` - **Tests Removed Functionality**
- Tests `add_allowed_action()` and `remove_allowed_action()` methods
- These methods don't exist in current contract API
- Recommendation: Delete (functionality was removed)

### `revoke_approval_test.rs` - **Old API**
- Tests `revoke_approval()` functionality
- Uses old error variants and method signatures
- Recommendation: Rewrite when revocation feature is implemented, or delete

### `bounds.rs` - **Not a Test File**
- Contains constant definitions and boundary enforcement logic
- Not a test module - should not be wired as `#[cfg(test)]`
- Already correctly excluded from test modules

## Next Steps

1. **Quick Wins** (Priority: High)
   - Update `quorum_edge_test.rs` API calls
   - Update `upgrade_e2e_test.rs` API calls
   - Verify `invoke_contract_with_args_test.rs` compiles

2. **Medium Effort** (Priority: Medium)
   - Rewrite `cross_contract_error_test.rs` with real helper functions
   - Create mock target contracts for error testing

3. **Cleanup** (Priority: Low)
   - Delete `boundary_conditions_test.rs` (placeholder)
   - Delete `action_allowlist_test.rs` (tests removed functionality)
   - Delete `revoke_approval_test.rs` (old API, feature not implemented)

## API Migration Guide

### Common Changes Needed

```rust
// OLD API
let (env, admin, contract_id) = setup_initialized(threshold);
let client = MultisigContractClient::new(&env, &contract_id);
client.initialize(&admin, &threshold);
client.set_signers(&signers);

// NEW API
let env = Env::default();
env.mock_all_auths();
let contract_id = env.register(MultisigContract, ());
let client = MultisigContractClient::new(&env, &contract_id);
let mut signers = Vec::new(&env);
signers.push_back(Address::generate(&env));
client.initialize(&signers, &threshold);
```

### Error Name Changes

- `InsufficientApprovals` → `QuorumNotReached`
- `ProposalAlreadyExecuted` → `AlreadyExecuted`
- `ApprovalNotFound` → (no direct equivalent, check proposal status)

### Removed Constants

- `MIN_THRESHOLD_DELAY_LEDGERS` - replaced with proposal-specific `eta_ledger`
- Access via: `client.get_proposal(&proposal_id).unwrap().eta_ledger`
