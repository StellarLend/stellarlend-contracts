//! # Repay Dust Handling Tests
//!
//! Comprehensive tests for dust handling in repay operations, ensuring:
//! - Full repay leaves zero debt
//! - Events match final state
//! - Dust cleanup works correctly
//! - Edge cases are handled properly

#![cfg(test)]

use crate::deposit::{DepositDataKey, Position, UserAnalytics};
use crate::events::RepayEvent;
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, IntoVal,
};

/// Helper function to create a test environment
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Helper function to get user position
fn get_user_position(env: &Env, contract_id: &Address, user: &Address) -> Option<Position> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
    })
}

/// Helper function to setup a basic lending scenario
fn setup_lending_scenario(
    env: &Env,
    client: &HelloContractClient,
    user: &Address,
    native_asset: &Address,
    deposit_amount: i128,
    borrow_amount: i128,
) {
    // Deposit collateral
    client.deposit_collateral(user, &None, &deposit_amount);
    
    // Borrow assets
    client.borrow_asset(user, &None, &borrow_amount);
    
    // Mint tokens to user for repayment
    let native_token_client = soroban_sdk::token::StellarAssetClient::new(env, native_asset);
    native_token_client.mint(user, &(borrow_amount * 2)); // Mint extra for full repay
    native_token_client.approve(
        user,
        &env.current_contract_address(),
        &(borrow_amount * 2),
        &(env.ledger().sequence() + 100),
    );
}

#[test]
fn test_full_repay_zero_debt_state() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup lending scenario
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 10000, 5000);

    // Verify initial debt
    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.debt, 5000);
    assert_eq!(position.borrow_interest, 0);

    // Perform full repay (overpay to ensure full repayment)
    let (remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &6000);

    // Verify zero debt state
    assert_eq!(remaining_debt, 0, "Remaining debt should be zero after full repay");
    assert_eq!(principal_paid, 5000, "Principal paid should equal original debt");
    assert_eq!(interest_paid, 0, "No interest should be paid without time passage");

    // Verify position state
    let final_position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(final_position.debt, 0, "Position debt should be zero");
    assert_eq!(final_position.borrow_interest, 0, "Position interest should be zero");
}

#[test]
fn test_dust_cleanup_on_small_remaining_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup lending scenario with small debt
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 10000, 150);

    // Repay most of the debt, leaving dust amount
    let (remaining_debt, _, _) = client.repay_debt(&user, &None, &100);

    // Verify dust was cleaned up (remaining should be 0, not 50)
    assert_eq!(remaining_debt, 0, "Dust amount should be cleaned up to zero");

    // Verify position state
    let final_position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(final_position.debt, 0, "Position debt should be zero after dust cleanup");
    assert_eq!(final_position.borrow_interest, 0, "Position interest should be zero");
}

#[test]
fn test_event_amounts_match_final_state() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup lending scenario
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 10000, 5000);

    // Clear any existing events
    env.events().all();

    // Perform full repay
    let (remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &6000);

    // Verify final state
    assert_eq!(remaining_debt, 0);
    let total_paid = interest_paid + principal_paid;

    // Check events
    let events = env.events().all();
    let repay_events: Vec<_> = events
        .iter()
        .filter_map(|(_, _, data)| {
            RepayEvent::try_from_val(&env, data).ok()
        })
        .collect();

    assert_eq!(repay_events.len(), 1, "Should have exactly one repay event");
    let repay_event = &repay_events[0];
    
    assert_eq!(repay_event.user, user, "Event user should match");
    assert_eq!(repay_event.asset, None, "Event asset should match");
    assert_eq!(repay_event.amount, total_paid, "Event amount should match actual amount paid");
}

#[test]
fn test_dust_cleanup_with_interest_accrual() {
    let env = create_test_env();
    env.ledger().with_mut(|li| li.timestamp = 1000);

    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup lending scenario
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 100000, 1000);

    // Jump forward in time to accrue some interest
    env.ledger().with_mut(|li| li.timestamp = 1000 + 31536000); // 1 year

    // Repay most debt, leaving small amount that should trigger dust cleanup
    let (remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &1050); // Should cover principal + most interest

    // Verify dust cleanup occurred
    assert_eq!(remaining_debt, 0, "Remaining debt should be zero after dust cleanup");
    assert!(interest_paid > 0, "Some interest should have been paid");
    assert!(principal_paid > 0, "Some principal should have been paid");

    // Verify position state
    let final_position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(final_position.debt, 0, "Position debt should be zero");
    assert_eq!(final_position.borrow_interest, 0, "Position interest should be zero");
}

#[test]
fn test_partial_repay_no_dust_cleanup() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup lending scenario
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 10000, 5000);

    // Perform partial repay that leaves significant debt
    let (remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &2000);

    // Verify no dust cleanup (debt is above threshold)
    assert_eq!(remaining_debt, 3000, "Should have 3000 remaining debt");
    assert_eq!(principal_paid, 2000, "Should have paid 2000 principal");
    assert_eq!(interest_paid, 0, "No interest without time passage");

    // Verify position state
    let final_position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(final_position.debt, 3000, "Position debt should be 3000");
    assert_eq!(final_position.borrow_interest, 0, "Position interest should be zero");
}

#[test]
fn test_dust_threshold_boundary() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Test exactly at dust threshold (100)
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 10000, 200);
    
    // Repay leaving exactly 100 (at threshold)
    let (remaining_debt, _, _) = client.repay_debt(&user, &None, &100);
    
    // Should trigger dust cleanup since remaining == DUST_THRESHOLD
    assert_eq!(remaining_debt, 0, "Debt at threshold should be cleaned up");

    // Test just above dust threshold
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 10000, 201);
    
    // Repay leaving 101 (above threshold)
    let (remaining_debt, _, _) = client.repay_debt(&user, &None, &100);
    
    // Should NOT trigger dust cleanup
    assert_eq!(remaining_debt, 101, "Debt above threshold should remain");
}

#[test]
fn test_multiple_repay_operations_dust_handling() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup lending scenario
    setup_lending_scenario(&env, &client, &user, &native_asset_addr, 10000, 1000);

    // First partial repay
    let (remaining_debt, _, _) = client.repay_debt(&user, &None, &500);
    assert_eq!(remaining_debt, 500, "Should have 500 remaining after first repay");

    // Second partial repay
    let (remaining_debt, _, _) = client.repay_debt(&user, &None, &300);
    assert_eq!(remaining_debt, 200, "Should have 200 remaining after second repay");

    // Third repay that should trigger dust cleanup
    let (remaining_debt, _, _) = client.repay_debt(&user, &None, &150);
    assert_eq!(remaining_debt, 0, "Should have 0 remaining after dust cleanup");

    // Verify final position
    let final_position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(final_position.debt, 0, "Final position debt should be zero");
    assert_eq!(final_position.borrow_interest, 0, "Final position interest should be zero");
}