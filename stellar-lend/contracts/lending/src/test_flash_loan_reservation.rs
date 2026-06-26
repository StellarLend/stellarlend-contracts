//! Tests for flash loan reservation accounting
//! 
//! Verifies that:
//! 1. Flash loan reservations debit the counter
//! 2. Repayment credits the counter back
//! 3. Deposit cap check uses current + reserved
//! 4. Same-ledger interleaving of flash loan + deposit respects cap

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    vec, Address, Env, IntoVal, Symbol, Val,
};
use crate::{
    LendingContract, LendingContractClient,
    DataKey, AssetParams,
};

/// Mock flash loan receiver contract for testing
mod flash_loan_receiver {
    use soroban_sdk::{contract, contractimpl, Address, Env, Vec, Val};
    
    pub struct FlashLoanReceiver;
    
    #[contractimpl]
    impl FlashLoanReceiver {
        pub fn on_flash_loan(
            env: Env,
            initiator: Address,
            asset: Address,
            amount: i128,
            fee: i128,
            data: Vec<Val>,
        ) {
            // In a real receiver, this would do arbitrage and repay
            // For testing, we just verify the parameters
            assert_eq!(amount, 1000i128);
            assert!(fee > 0);
        }
    }
}

/// Test that flash loan reservation is debited and credited.
#[test]
fn test_flash_loan_reservation_debit_credit() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    let receiver = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Set asset params with deposit cap
    client.set_asset_params(
        &asset,
        &1_000_000_000i128, // total supply
        &500_000_000i128,   // deposit cap
        &8000u32,           // collateral factor
        &9000u32,           // liquidation threshold
    );
    
    // Seed contract with asset balance
    // (In real test, mint tokens to contract)
    
    // Verify initial reservation is zero
    let initial_reserved: i128 = env
        .storage()
        .temporary()
        .get(&DataKey::ReservedForFlashLoan(asset.clone()))
        .unwrap_or(0i128);
    assert_eq!(initial_reserved, 0i128);
    
    // Execute flash loan
    let flash_amount = 1000i128;
    let callback_data = vec![&env];
    
    // Note: In actual implementation, this would call flash_loan entrypoint
    // For test structure, we verify the reservation functions directly
    crate::reserve_flash_loan(&env, &asset, flash_amount);
    
    let reserved_after_debit: i128 = env
        .storage()
        .temporary()
        .get(&DataKey::ReservedForFlashLoan(asset.clone()))
        .unwrap_or(0i128);
    assert_eq!(reserved_after_debit, flash_amount);
    
    // Simulate repayment and release
    crate::release_flash_loan_reservation(&env, &asset, flash_amount);
    
    let reserved_after_release: i128 = env
        .storage()
        .temporary()
        .get(&DataKey::ReservedForFlashLoan(asset.clone()))
        .unwrap_or(0i128);
    assert_eq!(reserved_after_release, 0i128);
}

/// Test deposit cap check includes flash loan reservations.
#[test]
fn test_deposit_cap_includes_reservation() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    let depositor = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Set deposit cap at 10,000
    let deposit_cap = 10_000i128;
    client.set_asset_params(
        &asset,
        &1_000_000_000i128,
        &deposit_cap,
        &8000u32,
        &9000u32,
    );
    
    // Set total deposits at 8,000 (under cap)
    env.storage().persistent().set(
        &DataKey::TotalDeposits(asset.clone()),
        &8000i128,
    );
    
    // Reserve 1,500 for flash loan
    // Effective deposits = 8,000 + 1,500 = 9,500
    crate::reserve_flash_loan(&env, &asset, 1500i128);
    
    // Deposit of 1,000 should fail: 9,500 + 1,000 = 10,500 > 10,000 cap
    let result = std::panic::catch_unwind(|| {
        crate::check_deposit_cap(&env, &asset, 1000i128);
    });
    assert!(result.is_err(), "deposit should fail when cap would be exceeded by reservation");
    
    // Deposit of 500 should succeed: 9,500 + 500 = 10,000 == cap
    crate::check_deposit_cap(&env, &asset, 500i128); // Should not panic
    
    // Release reservation
    crate::release_flash_loan_reservation(&env, &asset, 1500i128);
    
    // Now deposit of 2,000 should succeed: 8,000 + 2,000 = 10,000 == cap
    crate::check_deposit_cap(&env, &asset, 2000i128); // Should not panic
}

/// Test same-ledger interleaving: flash loan + deposit.
#[test]
fn test_same_ledger_flash_loan_and_deposit() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    let depositor = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Set deposit cap at 10,000
    client.set_asset_params(
        &asset,
        &1_000_000_000i128,
        &10_000i128,
        &8000u32,
        &9000u32,
    );
    
    // Set total deposits at 8,000
    env.storage().persistent().set(
        &DataKey::TotalDeposits(asset.clone()),
        &8000i128,
    );
    
    // Ledger sequence for this test
    env.ledger().set_sequence(100);
    
    // Step 1: Flash loan reservation of 1,500
    crate::reserve_flash_loan(&env, &asset, 1500i128);
    
    // Step 2: Attempt deposit of 2,000 (same ledger)
    // Without reservation accounting: 8,000 + 2,000 = 10,000 (would pass)
    // With reservation accounting: 8,000 + 1,500 + 2,000 = 11,500 > 10,000 (must fail)
    let deposit_result = std::panic::catch_unwind(|| {
        crate::check_deposit_cap(&env, &asset, 2000i128);
    });
    assert!(
        deposit_result.is_err(),
        "deposit during active flash loan must respect reservation"
    );
    
    // Step 3: Release flash loan reservation
    crate::release_flash_loan_reservation(&env, &asset, 1500i128);
    
    // Step 4: Now deposit of 2,000 should succeed
    crate::check_deposit_cap(&env, &asset, 2000i128); // Should not panic
}

/// Test that reservation cannot exceed total deposits.
#[test]
#[should_panic(expected = "reserved flash loan amount exceeds total deposits")]
fn test_reservation_cannot_exceed_total_deposits() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Set total deposits at 1,000
    env.storage().persistent().set(
        &DataKey::TotalDeposits(asset.clone()),
        &1000i128,
    );
    
    // Attempt to reserve 1,500 (> total deposits)
    crate::reserve_flash_loan(&env, &asset, 1500i128);
}

/// Test that release cannot exceed current reservation.
#[test]
#[should_panic(expected = "flash loan release exceeds reservation")]
fn test_release_cannot_exceed_reservation() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Reserve 500
    crate::reserve_flash_loan(&env, &asset, 500i128);
    
    // Attempt to release 1,000 (> reserved)
    crate::release_flash_loan_reservation(&env, &asset, 1000i128);
}

/// Test multiple concurrent flash loan reservations on same asset.
#[test]
fn test_multiple_flash_loan_reservations() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    // Set total deposits at 10,000
    env.storage().persistent().set(
        &DataKey::TotalDeposits(asset.clone()),
        &10000i128,
    );
    
    // First reservation: 2,000
    crate::reserve_flash_loan(&env, &asset, 2000i128);
    assert_eq!(
        crate::get_reserved_for_flash_loan(&env, &asset),
        2000i128
    );
    
    // Second reservation: 3,000
    crate::reserve_flash_loan(&env, &asset, 3000i128);
    assert_eq!(
        crate::get_reserved_for_flash_loan(&env, &asset),
        5000i128
    );
    
    // Third reservation: 4,000 (total would be 9,000)
    crate::reserve_flash_loan(&env, &asset, 4000i128);
    assert_eq!(
        crate::get_reserved_for_flash_loan(&env, &asset),
        9000i128
    );
    
    // Fourth reservation: 2,000 would exceed total (9,000 + 2,000 = 11,000 > 10,000)
    let result = std::panic::catch_unwind(|| {
        crate::reserve_flash_loan(&env, &asset, 2000i128);
    });
    assert!(result.is_err());
}

/// Test reservation is temporary (ledger-scoped).
#[test]
fn test_reservation_is_temporary_storage() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    
    client.initialize(&admin);
    
    crate::reserve_flash_loan(&env, &asset, 1000i128);
    
    // Verify it's in temporary storage
    assert!(
        env.storage().temporary().has(&DataKey::ReservedForFlashLoan(asset.clone())),
        "reservation must be in temporary storage"
    );
    assert!(
        !env.storage().persistent().has(&DataKey::ReservedForFlashLoan(asset.clone())),
        "reservation must NOT be in persistent storage"
    );
    assert!(
        !env.storage().instance().has(&DataKey::ReservedForFlashLoan(asset.clone())),
        "reservation must NOT be in instance storage"
    );
}