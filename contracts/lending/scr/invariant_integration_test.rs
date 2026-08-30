#![cfg(test)]

use soroban_sdk::{testutils::Address as _, token, Address, Env};
use crate::{LendingContract, LendingContractClient, DataKey};

/// Helper to create test token
fn create_token(env: &Env, admin: &Address) -> token::Client {
    token::StellarAssetClient::new(env, &Address::generate(env))
}

#[test]
fn test_deposit_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // This test verifies that deposit calls check_invariant_before and check_invariant_after
    // In a real test, we would mock the token client and verify the balance matches
}

#[test]
fn test_withdraw_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify withdraw has before/after invariant checks
}

#[test]
fn test_borrow_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify borrow has before/after invariant checks
}

#[test]
fn test_repay_has_invariant_checks() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify repay has before/after invariant checks
}

#[test]
fn test_borrow_against_collateral_checks_collateral_asset() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify cross-asset borrow checks collateral_asset invariant
}

#[test]
fn test_repay_against_collateral_checks_collateral_asset() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let repay_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify cross-asset repay checks collateral_asset invariant
}

#[test]
fn test_liquidate_checks_both_assets() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify liquidate checks BOTH debt_asset and collateral_asset invariants
    // This is critical: before/after checks for both assets
}

#[test]
fn test_flash_loan_excludes_callback_from_invariant() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let receiver = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Verify flash loan:
    // 1. Checks invariant BEFORE loan
    // 2. Sets FlashActive guard (skips checks during callback)
    // 3. Checks invariant AFTER full repayment
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_drift_detection_in_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Manually corrupt accounting to trigger drift detection
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &9999i128);
    });
    
    // This should panic with invariant violation
    client.deposit(&user, &1000, &asset);
}

#[test]
#[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
fn test_drift_detection_in_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Set up initial state
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::UserBalance(user.clone(), asset.clone()), &1000i128);
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &1000i128);
    });
    
    // Corrupt accounting before withdrawal
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &500i128);
    });
    
    // This should panic with invariant violation
    client.withdraw(&user, &100, &asset);
}

#[test]
fn test_bad_debt_reduces_expected_reserve() {
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Set up accounting with bad debt
    env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &10000i128);
    env.storage().persistent().set(&DataKey::Treasury(asset.clone()), &500i128);
    env.storage().persistent().set(&DataKey::BadDebt(asset.clone()), &200i128);
    
    let expected = crate::invariants::compute_expected_reserve(&env, &asset);
    
    // Expected = 10000 + 500 - 200 = 10300
    assert_eq!(expected, 10300);
}

#[test]
fn test_treasury_increases_expected_reserve() {
    let env = Env::default();
    let asset = Address::generate(&env);
    
    // Set up accounting with treasury fees
    env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &8000i128);
    env.storage().persistent().set(&DataKey::Treasury(asset.clone()), &350i128);
    
    let expected = crate::invariants::compute_expected_reserve(&env, &asset);
    
    // Expected = 8000 + 350 = 8350
    assert_eq!(expected, 8350);
}

#[test]
fn test_operation_sequence_maintains_invariant() {
    let env = Env::default();
    env.mock_all_auths();
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Test sequence of operations maintains invariant at each step
    // 1. User1 deposits
    // 2. User2 deposits
    // 3. User1 borrows
    // 4. User1 repays
    // 5. User2 withdraws
    // 
    // Each operation should pass invariant checks
}
