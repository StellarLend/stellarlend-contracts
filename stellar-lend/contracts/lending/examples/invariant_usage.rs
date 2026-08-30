//! Example: Using Reserve Invariant Checking
//!
//! This example demonstrates how to use the reserve invariant checking system
//! in the lending contract.

#![cfg(test)]

use soroban_sdk::{Address, Env};

/// Example 1: Basic invariant check pattern
///
/// Every state-changing operation follows this pattern:
/// 1. Check invariant before
/// 2. Perform operation
/// 3. Check invariant after
#[test]
fn example_basic_invariant_pattern() {
    use crate::invariants;
    
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // BEFORE the operation
    invariants::check_invariant_before(&env, &asset);
    
    // Perform state-changing operation
    // ... business logic that modifies storage and transfers tokens ...
    
    // AFTER the operation
    invariants::check_invariant_after(&env, &asset);
}

/// Example 2: Using the macro wrapper
///
/// For cleaner code, use the with_invariant_check! macro
#[test]
fn example_using_macro() {
    use crate::with_invariant_check;
    
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Wrap operation in macro
    let result = with_invariant_check!(env, asset, {
        // ... state-changing operation ...
        42i128
    });
    
    assert_eq!(result, 42);
}

/// Example 3: Multi-asset operation (like liquidation)
///
/// Operations that affect multiple assets should check both
#[test]
fn example_multi_asset_operation() {
    use crate::invariants;
    
    let env = Env::default();
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    
    // Check BOTH assets before
    invariants::check_invariant_before(&env, &debt_asset);
    invariants::check_invariant_before(&env, &collateral_asset);
    
    // Perform liquidation (transfers both debt and collateral tokens)
    // ... liquidation logic ...
    
    // Check BOTH assets after
    invariants::check_invariant_after(&env, &debt_asset);
    invariants::check_invariant_after(&env, &collateral_asset);
}

/// Example 4: Understanding the invariant formula
///
/// The invariant checks that:
/// actual_balance == expected_balance
#[test]
fn example_invariant_formula() {
    use crate::{invariants, DataKey};
    use soroban_sdk::token::Client as TokenClient;
    
    let env = Env::default();
    let asset = Address::generate(&env);
    let contract_address = env.current_contract_address();
    
    // Actual balance: what the contract actually holds
    let token_client = TokenClient::new(&env, &asset);
    let actual_balance = token_client.balance(&contract_address);
    
    // Expected balance: computed from internal accounting
    let expected_balance = invariants::compute_expected_reserve(&env, &asset);
    
    // The invariant asserts they match exactly
    assert_eq!(actual_balance, expected_balance);
}

/// Example 5: Expected balance computation
///
/// Shows what components make up the expected balance
#[test]
fn example_expected_balance_components() {
    use crate::DataKey;
    
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Component 1: Total deposits (user collateral)
    let total_deposits: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDeposits)
        .unwrap_or(0);
    
    // Component 2: Bad debt (reduces reserves)
    let bad_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::BadDebt)
        .unwrap_or(0);
    
    // Expected balance formula
    let expected = total_deposits - bad_debt;
    
    println!("Expected balance breakdown:");
    println!("  Total deposits: {}", total_deposits);
    println!("  Bad debt: -{}", bad_debt);
    println!("  Expected: {}", expected);
}

/// Example 6: What triggers a panic
///
/// When drift is detected, the invariant panics with details
#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn example_drift_triggers_panic() {
    use crate::{invariants, DataKey};
    
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Create intentional mismatch
    // Say accounting shows 10,000 but token balance is 9,500
    env.storage().persistent().set(&DataKey::TotalDeposits, &10000i128);
    
    // This will panic because token balance (0 in test) != expected (10000)
    invariants::check_invariant_before(&env, &asset);
}

/// Example 7: Panic message format
///
/// Shows what information is included when a violation occurs
#[test]
fn example_panic_message_format() {
    // When an invariant violation occurs, you'll see:
    //
    // thread 'test_name' panicked at 'RESERVE INVARIANT VIOLATION [AFTER]:
    //   asset=GBXYZ...,
    //   actual_balance=9950,
    //   expected_balance=10000,
    //   drift=-50'
    //
    // Interpretation:
    // - [AFTER] - Violation detected after operation
    // - drift=-50 - Actual is 50 stroops less than expected
    // - This means either:
    //   1. Forgot to update accounting
    //   2. Transferred too many tokens
    //   3. Missing bad debt tracking
}

/// Example 8: Adding invariant checks to a new operation
///
/// Template for protecting new state-changing functions
#[test]
fn example_new_operation_template() {
    use crate::invariants;
    
    fn my_new_operation(
        env: &Env,
        user: &Address,
        amount: i128,
        asset: &Address,
    ) -> Result<i128, String> {
        // STEP 1: Check invariant BEFORE
        invariants::check_invariant_before(env, asset);
        
        // STEP 2: Validate inputs
        if amount <= 0 {
            return Err("Invalid amount".to_string());
        }
        
        // STEP 3: Perform state changes
        // - Update internal accounting (storage)
        // - Transfer tokens
        // ... operation logic ...
        
        // STEP 4: Check invariant AFTER
        invariants::check_invariant_after(env, asset);
        
        // STEP 5: Return result
        Ok(amount)
    }
}

/// Example 9: Disabling invariants in production (optional)
///
/// If gas costs are too high, you can use feature flags
#[test]
fn example_conditional_compilation() {
    // In your code:
    /*
    #[cfg(feature = "invariant-checks")]
    invariants::check_invariant_before(&env, &asset);
    
    // ... operation ...
    
    #[cfg(feature = "invariant-checks")]
    invariants::check_invariant_after(&env, &asset);
    */
    
    // Then build without checks:
    // $ cargo build --release --no-default-features
}

/// Example 10: Debugging a violation
///
/// Steps to debug when invariant fails
#[test]
fn example_debugging_violation() {
    // When you see a violation:
    //
    // 1. Note the drift amount
    //    - Positive drift: more tokens than expected
    //    - Negative drift: fewer tokens than expected
    //
    // 2. Check the operation that failed
    //    - Did it update internal accounting?
    //    - Did it transfer the correct amount?
    //
    // 3. Verify accounting components
    //    - TotalDeposits updated?
    //    - BadDebt tracked correctly?
    //    - All storage writes successful?
    //
    // 4. Look for common issues
    //    - Missing storage.set() call
    //    - Rounding error accumulation
    //    - Forgot to account for fees
    //    - External token transfer bypassing accounting
    //
    // 5. Add logging or debugging
    //    - Print expected_balance components
    //    - Verify token transfers occurred
    //    - Check storage state before/after
}

/// Example 11: Testing your own operations
///
/// How to write tests for invariant-protected operations
#[test]
fn example_testing_pattern() {
    use crate::{invariants, DataKey, LendingContract, LendingContractClient};
    use soroban_sdk::testutils::Address as _;
    
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    client.initialize(&admin);
    
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    // Set up balanced initial state
    env.storage().persistent().set(&DataKey::TotalDeposits, &1000i128);
    // (In real test, also mock token balance)
    
    // Execute operation - should not panic
    // client.withdraw(&user, &100, &asset);
    
    // Verify final state is still balanced
    invariants::check_invariant_after(&env, &asset);
}

/// Example 12: Performance considerations
///
/// Understanding the gas cost impact
#[test]
fn example_performance_impact() {
    // Each invariant check performs:
    // 1. Token balance query (~2,000 gas)
    // 2. Storage reads for accounting (~1,000 gas each)
    // 3. Arithmetic operations (~100 gas)
    //
    // Total per check: ~5,000 gas
    // Total per operation (before + after): ~10,000 gas
    //
    // For a deposit costing 50,000 gas:
    // - Without invariants: 50,000 gas
    // - With invariants: 60,000 gas
    // - Overhead: 20%
    //
    // If this is too high, consider:
    // - Feature flag to disable in production
    // - Sampling (check 1 in 10 operations)
    // - Check only high-risk operations
}
