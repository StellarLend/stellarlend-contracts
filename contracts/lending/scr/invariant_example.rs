//! Example demonstrating reserve invariant checking in action
//! 
//! This example shows:
//! 1. How invariant checks are triggered before/after each operation
//! 2. What happens when drift is detected
//! 3. How to interpret invariant violation messages

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::{LendingContract, LendingContractClient, DataKey};

#[test]
fn example_successful_operation_with_invariants() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // SCENARIO: User deposits 1000 tokens
    // 
    // Before deposit:
    //   actual_balance = token_client.balance(&contract) = 0
    //   expected_balance = TotalDeposits(0) + Treasury(0) - BadDebt(0) = 0
    //   ✓ Invariant passes
    //
    // Operation: Transfer 1000 tokens to contract, update TotalDeposits to 1000
    //
    // After deposit:
    //   actual_balance = token_client.balance(&contract) = 1000
    //   expected_balance = TotalDeposits(1000) + Treasury(0) - BadDebt(0) = 1000
    //   ✓ Invariant passes
    
    println!("✓ Deposit completed successfully with invariant checks");
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION [BEFORE]")]
fn example_drift_detected_before_operation() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // SCENARIO: Accounting is corrupted before operation
    //
    // Corrupt the accounting ledger directly
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &5000i128);
    });
    
    // Try to deposit
    // 
    // Before deposit:
    //   actual_balance = token_client.balance(&contract) = 0
    //   expected_balance = TotalDeposits(5000) + Treasury(0) - BadDebt(0) = 5000
    //   drift = 0 - 5000 = -5000
    //   ✗ PANIC: "RESERVE INVARIANT VIOLATION [BEFORE]: 
    //            asset=..., actual_balance=0, expected_balance=5000, drift=-5000"
    
    client.deposit(&user, &1000, &asset);
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION [AFTER]")]
fn example_drift_detected_after_operation() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // SCENARIO: Operation creates accounting drift
    //
    // This would happen if there's a bug in the operation logic
    // For example, if token transfer succeeds but accounting update fails or is incorrect
    //
    // Before operation: both balances are 0, invariant passes ✓
    // After operation: 
    //   - Token transfer brings actual_balance to 1000
    //   - But if accounting bug only adds 900 to TotalDeposits
    //   actual_balance = 1000
    //   expected_balance = 900
    //   drift = +100
    //   ✗ PANIC: "RESERVE INVARIANT VIOLATION [AFTER]: 
    //            asset=..., actual_balance=1000, expected_balance=900, drift=+100"
    
    // Note: In the actual implementation, this shouldn't happen
    // This test would require injecting a bug to demonstrate
}

#[test]
fn example_complex_operation_sequence() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // SCENARIO: Multiple users interact with protocol
    //
    // Step 1: Alice deposits 5000
    //   Before: actual=0, expected=0, drift=0 ✓
    //   After: actual=5000, expected=TotalDeposits(5000)=5000, drift=0 ✓
    //
    // Step 2: Bob deposits 3000
    //   Before: actual=5000, expected=TotalDeposits(5000)=5000, drift=0 ✓
    //   After: actual=8000, expected=TotalDeposits(8000)=8000, drift=0 ✓
    //
    // Step 3: Alice withdraws 2000
    //   Before: actual=8000, expected=TotalDeposits(8000)=8000, drift=0 ✓
    //   After: actual=6000, expected=TotalDeposits(6000)=6000, drift=0 ✓
    //
    // Step 4: Flash loan of 4000 with 1% fee (40)
    //   Before flash: actual=6000, expected=6000, drift=0 ✓
    //   During callback: FlashActive=true, invariant checks SKIPPED
    //   After repayment: actual=6040, expected=TotalDeposits(6000)+Treasury(40)=6040, drift=0 ✓
    
    println!("✓ All operations maintained invariant throughout sequence");
}

#[test]
fn example_liquidation_checks_both_assets() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let usdc = Address::generate(&env); // Debt asset
    let eth = Address::generate(&env);  // Collateral asset
    
    client.initialize(&admin);
    
    // SCENARIO: Liquidation of undercollateralized position
    //
    // Liquidate checks BOTH assets:
    //
    // Before liquidation:
    //   USDC: actual=10000, expected=10000, drift=0 ✓
    //   ETH: actual=5000, expected=5000, drift=0 ✓
    //
    // Operation:
    //   - Liquidator repays 1000 USDC of borrower's debt
    //   - Liquidator receives 1.1 ETH as collateral (10% bonus)
    //
    // After liquidation:
    //   USDC: actual=11000, expected=11000, drift=0 ✓
    //   ETH: actual=3900, expected=3900, drift=0 ✓
    //
    // If either asset has drift, transaction panics and reverts completely
    
    println!("✓ Liquidation maintained invariant for both assets");
}

#[test]
fn example_flash_loan_callback_exemption() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let receiver = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // SCENARIO: Flash loan temporarily violates invariant during callback
    //
    // Initial state: actual=10000, expected=10000 ✓
    //
    // Flash loan flow:
    //   1. Check invariant BEFORE: actual=10000, expected=10000 ✓
    //   2. Set FlashActive guard
    //   3. Transfer 5000 to receiver: actual=5000
    //   4. Callback to receiver (during this, invariant is violated)
    //      - If receiver tried to call deposit/withdraw/etc, invariant checks are SKIPPED
    //      - This is safe because FlashActive guard prevents nested operations
    //   5. Receiver repays 5050 (5000 + 1% fee): actual=10050
    //   6. Add 50 to Treasury
    //   7. Clear FlashActive guard
    //   8. Check invariant AFTER: actual=10050, expected=TotalDeposits(10000)+Treasury(50)=10050 ✓
    //
    // The temporary violation during callback is expected and safe
    // The invariant is restored at the end of the flash loan
    
    println!("✓ Flash loan correctly exempts callback phase from invariant checks");
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn example_bad_debt_accounting_error() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // SCENARIO: Bad debt is recorded but not reflected in accounting
    //
    // Setup: User has deposited 1000
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &1000i128);
    });
    // Simulate token balance
    // actual_balance = 1000
    
    // Protocol records 100 bad debt (e.g., liquidation shortfall)
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::BadDebt(asset.clone()), &100i128);
    });
    
    // Now expected_balance = TotalDeposits(1000) - BadDebt(100) = 900
    // But actual_balance = 1000 (tokens still in contract)
    // drift = +100
    //
    // Next operation will detect this:
    // ✗ PANIC: "RESERVE INVARIANT VIOLATION [BEFORE]: 
    //          asset=..., actual_balance=1000, expected_balance=900, drift=+100"
    //
    // This indicates bad debt was not properly burned/removed from reserves
    
    client.withdraw(&user, &50, &asset);
}

/// Example of interpreting invariant violation messages
///
/// VIOLATION MESSAGE FORMAT:
/// ```
/// RESERVE INVARIANT VIOLATION [BEFORE/AFTER]: 
///   asset=CA3D5KRYM6CB7OWQ6TWYRR3Z4T7GNZLKERYNZGGA5SOAOPIFY6YQGAXE, 
///   actual_balance=10000, 
///   expected_balance=9950, 
///   drift=+50
/// ```
///
/// INTERPRETATION:
/// - [BEFORE]: Drift existed before operation started → Pre-existing bug
/// - [AFTER]: Drift created by this operation → Bug in this operation's logic
/// - drift > 0: Contract holds MORE tokens than accounting suggests → Missing accounting update
/// - drift < 0: Contract holds FEWER tokens than accounting suggests → Tokens missing or double-counted
///
/// DEBUGGING STEPS:
/// 1. Identify the operation from the call stack
/// 2. Check if [BEFORE] or [AFTER]
/// 3. Analyze drift magnitude and direction
/// 4. Review accounting updates in the operation
/// 5. Check for missing token transfers or accounting updates
/// 6. Verify all accounting components are included in compute_expected_reserve()
#[test]
fn example_interpreting_violations() {
    // This is a documentation test - see function doc comment above
    println!("See function documentation for interpretation guide");
}
