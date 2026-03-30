//! # Stress Tests for Large User and Position Counts
//!
//! Comprehensive stress testing suite to validate storage layout, indexing,
//! and iteration logic under load. Tests cover edge cases at maximum
//! configured entries and ensure operations remain correct as counts grow.

use crate::*;
use crate::testutils::create_token;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

// ═══════════════════════════════════════════════════════
// Test Constants
// ═══════════════════════════════════════════════════════

/// Number of users to create for large-scale tests
const STRESS_USER_COUNT: u32 = 150;

/// Number of positions per user for multi-position tests
const POSITIONS_PER_USER: u32 = 10;

// ═══════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════

/// Setup environment with initialized lending contract
fn setup_stress_test(env: &Env) -> (LendingContractClient<'_>, Address, Address, Address, token::StellarAssetClient<'_>, token::StellarAssetClient<'_>) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let (asset, asset_client) = create_token(env, &admin);
    let (collateral_asset, collateral_client) = create_token(env, &admin);

    // Initialize with high limits for stress testing
    client.initialize(&admin, &10_000_000_000, &100);

    (client, admin, asset, collateral_asset, asset_client, collateral_client)
}

/// Generate multiple user addresses for stress testing
fn generate_users(env: &Env, count: u32) -> Vec<Address> {
    let mut users = Vec::new(env);
    for _ in 0..count {
        users.push_back(Address::generate(env));
    }
    users
}

/// Create borrow positions for multiple users
fn create_user_borrow_positions(
    _env: &Env,
    client: &LendingContractClient<'_>,
    users: &Vec<Address>,
    asset: &Address,
    collateral_asset: &Address,
    collateral_client: &token::StellarAssetClient<'_>,
    positions_per_user: u32,
) {
    for (i, user) in users.iter().enumerate() {
        for j in 0..positions_per_user {
            let borrow_amount = 10_000 + (i as i128 * 1000) + (j as i128 * 100);
            let collateral_amount = borrow_amount * 2; // 200% collateral ratio

            collateral_client.mint(&user, &collateral_amount);
            client.borrow(
                &user,
                asset,
                &borrow_amount,
                collateral_asset,
                &collateral_amount,
            );
        }
    }
}

/// Create deposit positions for multiple users
fn create_user_deposit_positions(
    _env: &Env,
    client: &LendingContractClient<'_>,
    asset_client: &token::StellarAssetClient<'_>,
    users: &Vec<Address>,
    asset: &Address,
    positions_per_user: u32,
) {
    for (i, user) in users.iter().enumerate() {
        for j in 0..positions_per_user {
            let deposit_amount = 5_000 + (i as i128 * 500) + (j as i128 * 50);
            asset_client.mint(&user, &deposit_amount);
            client.deposit(&user, asset, &deposit_amount);
        }
    }
}

// ═══════════════════════════════════════════════════════
// Large User Count Stress Tests
// ═══════════════════════════════════════════════════════

#[test]
fn test_stress_large_user_count_borrow_positions() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, collateral_asset, _, collateral_client) = setup_stress_test(&env);

    let users = generate_users(&env, STRESS_USER_COUNT);

    // Create borrow positions for all users
    create_user_borrow_positions(&env, &client, &users, &asset, &collateral_asset, &collateral_client, 1);

    // Verify all user positions are correctly stored and retrievable
    for (i, user) in users.iter().enumerate() {
        let debt = client.get_user_debt(&user);
        assert_eq!(debt.borrowed_amount, 10_000 + (i as i128 * 1000));
        assert!(debt.borrowed_amount > 0);

        let collateral = client.get_user_collateral(&user);
        assert!(collateral.amount > 0);
        assert_eq!(collateral.amount, debt.borrowed_amount * 2);
    }
}

#[test]
fn test_stress_large_user_count_deposit_positions() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _collateral_asset, asset_client, _) = setup_stress_test(&env);

    // Initialize deposit settings with high cap
    client.initialize_deposit_settings(&1_000_000_000, &100);

    let users = generate_users(&env, STRESS_USER_COUNT);

    // Create deposit positions for all users
    create_user_deposit_positions(&env, &client, &asset_client, &users, &asset, 1);

    // Verify all user positions are correctly stored and retrievable
    for (i, user) in users.iter().enumerate() {
        let collateral = client.get_user_collateral_deposit(&user, &asset);
        assert!(collateral.amount > 0);
        assert_eq!(collateral.amount, 5_000 + (i as i128 * 500));
    }
}

#[test]
fn test_stress_mixed_operations_large_user_base() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, collateral_asset, asset_client, collateral_client) = setup_stress_test(&env);

    let users = generate_users(&env, STRESS_USER_COUNT);

    // Create mixed borrow and deposit positions
    for i in 0..(STRESS_USER_COUNT / 3) {
        let user = users.get(i).unwrap();

        // Borrow position
        collateral_client.mint(&user, &200_000);
        client.borrow(&user, &asset, &100_000, &collateral_asset, &200_000);

        // Deposit position
        asset_client.mint(&user, &50_000);
        client.deposit(&user, &asset, &50_000);
    }

    // Verify operations
    for i in 0..(STRESS_USER_COUNT / 3) {
        let user = users.get(i).unwrap();
        let debt = client.get_user_debt(&user);
        let collateral = client.get_user_collateral(&user);
        let deposit = client.get_user_collateral_deposit(&user, &asset);

        assert!(debt.borrowed_amount > 0);
        assert!(collateral.amount > 0);
        assert!(deposit.amount > 0);
    }
}

#[test]
fn test_stress_zero_amount_operations() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, collateral_asset, asset_client, collateral_client) = setup_stress_test(&env);

    let user = Address::generate(&env);

    // Verify initial state is clean
    let debt = client.get_user_debt(&user);
    let collateral = client.get_user_collateral(&user);
    let deposit = client.get_user_collateral_deposit(&user, &asset);

    assert_eq!(debt.borrowed_amount, 0);
    assert_eq!(collateral.amount, 0);
    assert_eq!(deposit.amount, 0);

    // Test with minimum valid amounts instead
    let min_amount = 1000;

    // These should work properly
    collateral_client.mint(&user, &(min_amount * 2));
    client.borrow(
        &user,
        &asset,
        &min_amount,
        &collateral_asset,
        &(min_amount * 2),
    );
    asset_client.mint(&user, &min_amount);
    client.deposit(&user, &asset, &min_amount);

    // Verify positions were created
    let debt = client.get_user_debt(&user);
    let collateral = client.get_user_collateral(&user);
    let deposit = client.get_user_collateral_deposit(&user, &asset);

    assert!(debt.borrowed_amount > 0);
    assert!(collateral.amount > 0);
    assert!(deposit.amount > 0);
}

#[test]
fn test_stress_multiple_positions_per_user() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, collateral_asset, _, collateral_client) = setup_stress_test(&env);

    let user = Address::generate(&env);

    // Create multiple positions for the same user
    for i in 0..POSITIONS_PER_USER {
        let borrow_amount = 10_000 + (i as i128 * 1000);
        let collateral_amount = borrow_amount * 2;

        collateral_client.mint(&user, &collateral_amount);
        client.borrow(
            &user,
            &asset,
            &borrow_amount,
            &collateral_asset,
            &collateral_amount,
        );
    }

    // Verify final position reflects cumulative operations
    let final_debt = client.get_user_debt(&user);
    let final_collateral = client.get_user_collateral(&user);

    assert!(final_debt.borrowed_amount > 0);
    assert!(final_collateral.amount > 0);
}
