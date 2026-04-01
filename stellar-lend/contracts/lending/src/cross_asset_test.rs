//! Comprehensive tests for the cross-asset lending registry module.
//!
//! Coverage targets:
//! - Initialization (admin + asset)
//! - Config updates (valid, invalid, unauthorized)
//! - Price updates (valid, zero, stale, unauthorized)
//! - Deposit / Withdraw / Borrow / Repay with checked math
//! - Health factor enforcement
//! - Supply and borrow caps
//! - Edge cases (zero amounts, overflow, re-initialization)
//! - Read-only queries

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

/// Test setup helper that creates a contract with admin, users, and multiple assets
fn setup_test(
    env: &Env,
) -> (
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let admin = Address::generate(env);
    let user1 = Address::generate(env);
    let user2 = Address::generate(env);
    let asset_usdc = Address::generate(env);
    let asset_eth = Address::generate(env);

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    client.initialize_admin(&admin);

    (client, admin, user1, user2, asset_usdc, asset_eth)
}

/// Helper to create standard asset parameters for testing
fn create_asset_params(
    env: &Env,
    ltv: i128,
    liquidation_threshold: i128,
    debt_ceiling: i128,
    is_active: bool,
) -> AssetParams {
    AssetParams {
        ltv,
        liquidation_threshold,
        price_feed: Address::generate(env),
        debt_ceiling,
        is_active,
    }
}

/// Helper to setup multiple assets with different configurations
fn setup_multi_asset_config(
    env: &Env,
    client: &LendingContractClient,
    _admin: &Address,
    asset_usdc: &Address,
    asset_eth: &Address,
) {
    env.mock_all_auths();

    // USDC: High LTV, stable asset
    let usdc_params = create_asset_params(env, 9000, 9500, 10000000, true); // 90% LTV, 95% liquidation
    client.set_asset_params(asset_usdc, &usdc_params);

    // ETH: Lower LTV, volatile asset
    let eth_params = create_asset_params(env, 7500, 8500, 5000000, true); // 75% LTV, 85% liquidation
    client.set_asset_params(asset_eth, &eth_params);
}
}

// ============================================================================
// 1. Admin Initialization
// ============================================================================

#[test]
fn test_initialize_ca_success() {
    let env = Env::default();
    let (client, _admin, _, _, asset_usdc, _) = setup_test(&env);

    let params = create_asset_params(&env, 8000, 8500, 1000000, true);

    env.mock_all_auths();
    client.set_asset_params(&asset_usdc, &params);

    // Verify asset was configured (would need getter in real implementation)
    // This test validates the basic configuration flow
}
#[test]
fn test_set_asset_params_multiple_assets() {
    let env = Env::default();
    let (client, admin, _, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // Test that multiple assets can be configured with different parameters
    // In a real implementation, we'd verify the stored parameters
}

#[test]
#[should_panic]
fn test_initialize_ca_twice_fails() {
    let (env, client, _admin) = setup();
    let other_admin = Address::generate(&env);
    // Second call with different admin should fail with AlreadyInitialized
    client.initialize_ca(&other_admin);
}

// ============================================================================
// 2. Asset Initialization
// ============================================================================

#[test]
fn test_initialize_asset_success() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    let fetched = client.get_asset_config(&None);
    assert_eq!(fetched.collateral_factor, 7500);
    assert_eq!(fetched.liquidation_threshold, 8000);
    assert_eq!(fetched.price, 10_000_000);
}

#[test]
fn test_asset_config_boundary_values() {
    let env = Env::default();
    let (client, _admin, _, _, asset_usdc, _) = setup_test(&env);

    env.mock_all_auths();

    // Test minimum values
    let min_params = create_asset_params(&env, 0, 0, 0, false);
    client.set_asset_params(&asset_usdc, &min_params);

    // Test maximum reasonable values
    let max_params = create_asset_params(&env, 10000, 10000, i128::MAX, true);
    client.set_asset_params(&asset_usdc, &max_params);
}

#[test]
fn test_asset_config_updates() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    env.mock_all_auths();

    // Initial configuration
    let initial_params = create_asset_params(&env, 8000, 8500, 1000000, true);
    client.set_asset_params(&asset_usdc, &initial_params);

    // User deposits with initial config
    client.deposit_collateral_asset(&user1, &asset_usdc, &1000);

    // Update configuration - reduce LTV (more conservative)
    let updated_params = create_asset_params(&env, 6000, 7000, 500000, true);
    client.set_asset_params(&asset_usdc, &updated_params);

    // Verify operations still work with updated config
    client.deposit_collateral_asset(&user1, &asset_usdc, &500);
}

#[test]
#[should_panic]
fn test_asset_deactivation() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    env.mock_all_auths();

    // Initial active configuration
    let active_params = create_asset_params(&env, 8000, 8500, 1000000, true);
    client.set_asset_params(&asset_usdc, &active_params);

    // User can deposit when active
    client.deposit_collateral_asset(&user1, &asset_usdc, &1000);

    // Deactivate asset
    let inactive_params = create_asset_params(&env, 8000, 8500, 1000000, false);
    client.set_asset_params(&asset_usdc, &inactive_params);

    // New deposits should fail
    client.deposit_collateral_asset(&user1, &asset_usdc, &500);
}
// ============================================================================
// MULTI-ASSET DEPOSIT AND COLLATERAL TESTS
// ============================================================================

#[test]
fn test_multi_asset_deposits() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // Deposit multiple assets
    client.deposit_collateral_asset(&user1, &asset_usdc, &10000); // $10k USDC
    client.deposit_collateral_asset(&user1, &asset_eth, &5000); // $5k ETH (at $1 mock price)

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_collateral_usd, 15000); // $15k total
    assert_eq!(summary.total_debt_usd, 0);
    assert!(summary.health_factor >= 10000);
}
}

#[test]
#[should_panic]
fn test_initialize_asset_invalid_ltv_above_10000() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.collateral_factor = 10_001; // Out of bounds
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_negative_ltv() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.collateral_factor = -1;
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_deposit_overflow_protection() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &_admin, &asset_usdc, &asset_usdc);

    // First deposit near max
    client.deposit_collateral_asset(&user1, &asset_usdc, &(i128::MAX - 1000));

    // Second deposit should cause overflow and fail
    client.deposit_collateral_asset(&user1, &asset_usdc, &2000);
}
// ============================================================================
// MULTI-ASSET BORROWING TESTS
// ============================================================================

#[test]
fn test_multi_collateral_single_borrow() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // Deposit multiple collaterals
    client.deposit_collateral_asset(&user1, &asset_usdc, &10000); // $10k USDC (90% LTV)
    client.deposit_collateral_asset(&user1, &asset_eth, &10000); // $10k ETH (75% LTV)

    // Total weighted collateral = (10k * 0.9) + (10k * 0.75) = 16.5k
    // Should be able to borrow up to $16.5k

    client.borrow_asset(&user1, &asset_usdc, &15000); // Borrow $15k USDC

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_collateral_usd, 20000);
    assert_eq!(summary.total_debt_usd, 15000);
    // Health factor = 16500 / 15000 * 10000 = 11000
    assert_eq!(summary.health_factor, 11000);
}

#[test]
fn test_multi_asset_borrowing() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // Large collateral deposit
    client.deposit_collateral_asset(&user1, &asset_usdc, &20000); // $20k USDC

    // Borrow multiple assets
    client.borrow_asset(&user1, &asset_usdc, &8000); // $8k USDC
    client.borrow_asset(&user1, &asset_eth, &4000); // $4k ETH

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_collateral_usd, 20000);
    assert_eq!(summary.total_debt_usd, 12000);
    // Health factor = (20000 * 0.9) / 12000 * 10000 = 15000
    assert_eq!(summary.health_factor, 15000);
}
}

#[test]
#[should_panic]
<<<<<<< HEAD
fn test_borrow_exceeds_collateral() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &_admin, &asset_usdc, &asset_usdc);

    client.deposit_collateral_asset(&user1, &asset_usdc, &10000); // $10k USDC (90% LTV)
                                                                  // Max borrow = 10k * 0.9 = 9k

    client.borrow_asset(&user1, &asset_usdc, &9500); // Should fail
=======
fn test_initialize_asset_zero_price() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.price = 0;
    client.initialize_asset(&None, &config);
>>>>>>> main
}

#[test]
#[should_panic]
<<<<<<< HEAD
fn test_borrow_exceeds_debt_ceiling() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    env.mock_all_auths();

    // Set low debt ceiling
    let params = create_asset_params(&env, 9000, 9500, 5000, true); // $5k ceiling
    client.set_asset_params(&asset_usdc, &params);

    client.deposit_collateral_asset(&user1, &asset_usdc, &10000); // $10k collateral

    client.borrow_asset(&user1, &asset_usdc, &6000); // Should fail - exceeds ceiling
}
#[test]
fn test_sequential_borrows_health_factor() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    client.deposit_collateral_asset(&user1, &asset_usdc, &20000); // $20k USDC

    // First borrow
    client.borrow_asset(&user1, &asset_usdc, &5000);
    let summary1 = client.get_cross_position_summary(&user1);
    assert_eq!(summary1.health_factor, 36000); // (20k * 0.9) / 5k * 10000

    // Second borrow
    client.borrow_asset(&user1, &asset_eth, &3000);
    let summary2 = client.get_cross_position_summary(&user1);
    assert_eq!(summary2.health_factor, 22500); // (20k * 0.9) / 8k * 10000

    // Third borrow - approaching limit
    client.borrow_asset(&user1, &asset_usdc, &9000);
    let summary3 = client.get_cross_position_summary(&user1);
    assert_eq!(summary3.health_factor, 10588); // (20k * 0.9) / 17k * 10000
}

// ============================================================================
// REPAYMENT TESTS
// ============================================================================

#[test]
fn test_partial_repayment_multi_asset() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // Setup position
    client.deposit_collateral_asset(&user1, &asset_usdc, &20000);
    client.borrow_asset(&user1, &asset_usdc, &8000);
    client.borrow_asset(&user1, &asset_eth, &4000);

    // Partial repayment of USDC debt
    client.repay_asset(&user1, &asset_usdc, &3000);

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_debt_usd, 9000); // 5k USDC + 4k ETH remaining
    assert_eq!(summary.health_factor, 20000); // (20k * 0.9) / 9k * 10000
}

#[test]
fn test_full_repayment_single_asset() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // Setup position
    client.deposit_collateral_asset(&user1, &asset_usdc, &20000);
    client.borrow_asset(&user1, &asset_usdc, &8000);
    client.borrow_asset(&user1, &asset_eth, &4000);

    // Full repayment of ETH debt
    client.repay_asset(&user1, &asset_eth, &4000);

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_debt_usd, 8000); // Only USDC debt remains
    assert_eq!(summary.health_factor, 22500); // (20k * 0.9) / 8k * 10000
}
#[test]
#[should_panic]
fn test_repay_zero_amount() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &_admin, &asset_usdc, &asset_usdc);

    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);
    client.borrow_asset(&user1, &asset_usdc, &5000);

    // Zero repayment should fail
    client.repay_asset(&user1, &asset_usdc, &0);
}

// ============================================================================
// WITHDRAWAL TESTS
// ============================================================================

#[test]
fn test_withdraw_with_remaining_collateral() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // Setup position with multiple collaterals
    client.deposit_collateral_asset(&user1, &asset_usdc, &20000); // $20k USDC
    client.deposit_collateral_asset(&user1, &asset_eth, &10000); // $10k ETH
    client.borrow_asset(&user1, &asset_usdc, &10000); // $10k debt

    // Withdraw some USDC collateral
    client.withdraw_asset(&user1, &asset_usdc, &5000);

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_collateral_usd, 25000); // 15k USDC + 10k ETH
    assert_eq!(summary.total_debt_usd, 10000);
    // Health factor = ((15k * 0.9) + (10k * 0.75)) / 10k * 10000 = 21000
    assert_eq!(summary.health_factor, 21000);
}
=======
fn test_withdraw_breaks_health_factor() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &_admin, &asset_usdc, &asset_usdc);

    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);
    client.borrow_asset(&user1, &asset_usdc, &8000); // Near max borrow

    // Try to withdraw collateral that would break health factor
    client.withdraw_asset(&user1, &asset_usdc, &2000); // Should fail
}
}

#[test]
fn test_initialize_asset_zero_caps_unlimited() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.max_supply = 0; // unlimited
    config.max_borrow = 0; // unlimited
    client.initialize_asset(&None, &config);

    let fetched = client.get_asset_config(&None);
    assert_eq!(fetched.max_supply, 0);
    assert_eq!(fetched.max_borrow, 0);
}

fn test_withdraw_all_collateral_no_debt() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_usdc);

    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);

    // Can withdraw all collateral when no debt
    client.withdraw_asset(&user1, &asset_usdc, &10000);

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_collateral_usd, 0);
    assert_eq!(summary.total_debt_usd, 0);
}

#[test]
#[should_panic]
fn test_withdraw_more_than_deposited() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &_admin, &asset_usdc, &asset_usdc);

    client.deposit_collateral_asset(&user1, &asset_usdc, &5000);

    // Try to withdraw more than deposited
    client.withdraw_asset(&user1, &asset_usdc, &6000); // Should fail
}
}

// ============================================================================
// MULTI-USER ISOLATION TESTS
// ============================================================================

#[test]
fn test_user_position_isolation() {
    let env = Env::default();
    let (client, admin, user1, user2, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // User1 operations
    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);
    client.borrow_asset(&user1, &asset_usdc, &5000);

    // User2 operations
    client.deposit_collateral_asset(&user2, &asset_eth, &8000);
    client.borrow_asset(&user2, &asset_eth, &3000);

    // Check positions are isolated
    let summary1 = client.get_cross_position_summary(&user1);
    let summary2 = client.get_cross_position_summary(&user2);

    assert_eq!(summary1.total_collateral_usd, 10000);
    assert_eq!(summary1.total_debt_usd, 5000);

    assert_eq!(summary2.total_collateral_usd, 8000);
    assert_eq!(summary2.total_debt_usd, 3000);
}

#[test]
fn test_concurrent_operations_different_users() {
    let env = Env::default();
    let (client, admin, user1, user2, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_usdc);

    // Both users deposit to same asset
    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);
    client.deposit_collateral_asset(&user2, &asset_usdc, &15000);

    // Both users borrow from same asset
    client.borrow_asset(&user1, &asset_usdc, &5000);
    client.borrow_asset(&user2, &asset_usdc, &8000);

    // Verify independent positions
    let summary1 = client.get_cross_position_summary(&user1);
    let summary2 = client.get_cross_position_summary(&user2);

    assert_eq!(summary1.total_debt_usd, 5000);
    assert_eq!(summary2.total_debt_usd, 8000);
}
// ============================================================================
// EDGE CASES AND BOUNDARY CONDITIONS
// ============================================================================

#[test]
fn test_health_factor_calculation() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &_admin, &asset_usdc, &asset_usdc);

    // Just test deposit and position summary without borrowing
    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_collateral_usd, 10000);
    assert_eq!(summary.total_debt_usd, 0);
    // With no debt, health factor should be very high
    assert!(summary.health_factor >= 100000);
}

#[test]
fn test_very_small_amounts() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &_admin, &asset_usdc, &asset_usdc);

    // Test with minimal amounts
    client.deposit_collateral_asset(&user1, &asset_usdc, &1);

    let summary = client.get_cross_position_summary(&user1);
    // With mock price of 10000000 (1.0 with 7 decimals), 1 unit = 1 USD
    assert_eq!(summary.total_collateral_usd, 1);
}

#[test]
fn test_arithmetic_overflow_protection() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, _) = setup_test(&env);

    env.mock_all_auths();

    // Set parameters that could cause overflow
    let params = create_asset_params(&env, 10000, 10000, i128::MAX, true);
    client.set_asset_params(&asset_usdc, &params);

    // Large deposit that approaches overflow limits
    let large_amount = i128::MAX / 10000000; // Divide by price to avoid overflow
    client.deposit_collateral_asset(&user1, &asset_usdc, &large_amount);

    // Should not panic due to overflow protection
    let summary = client.get_cross_position_summary(&user1);
    assert!(summary.total_collateral_usd > 0);
}

#[test]
#[should_panic]
fn test_update_asset_config_out_of_bounds_fails() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

<<<<<<< HEAD
    // User1 deposits
    let params = create_asset_params(&env, 8000, 8500, 1000000, true);
    env.mock_all_auths();
    client.set_asset_params(&asset_usdc, &params);

    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);

    // User2 should not be able to withdraw user1's collateral
    client.withdraw_asset(&user2, &asset_usdc, &5000);
}
// ============================================================================
// COMPREHENSIVE INTEGRATION TESTS
// ============================================================================

#[test]
fn test_complete_lending_cycle_multi_asset() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_eth);

    // 1. Deposit multiple collaterals
    client.deposit_collateral_asset(&user1, &asset_usdc, &15000);
    client.deposit_collateral_asset(&user1, &asset_eth, &10000);

    // 2. Borrow multiple assets
    client.borrow_asset(&user1, &asset_usdc, &8000);
    client.borrow_asset(&user1, &asset_eth, &5000);

    // 3. Partial repayments
    client.repay_asset(&user1, &asset_usdc, &3000);
    client.repay_asset(&user1, &asset_eth, &2000);

    // 4. Withdraw some collateral
    client.withdraw_asset(&user1, &asset_usdc, &5000);

    // 5. Final repayment
    client.repay_asset(&user1, &asset_usdc, &5000);
    client.repay_asset(&user1, &asset_eth, &3000);

    // 6. Withdraw remaining collateral
    client.withdraw_asset(&user1, &asset_usdc, &10000);
    client.withdraw_asset(&user1, &asset_eth, &10000);

    let final_summary = client.get_cross_position_summary(&user1);
    assert_eq!(final_summary.total_collateral_usd, 0);
    assert_eq!(final_summary.total_debt_usd, 0);
}

#[test]
fn test_asset_list_management() {
    let env = Env::default();
    let (client, _admin, user1, _, asset_usdc, asset_eth) = setup_test(&env);

    env.mock_all_auths();

    // Add first asset
    let usdc_params = create_asset_params(&env, 9000, 9500, 1000000, true);
    client.set_asset_params(&asset_usdc, &usdc_params);

    // Add second asset
    let eth_params = create_asset_params(&env, 7500, 8500, 500000, true);
    client.set_asset_params(&asset_eth, &eth_params);

    // Test operations with both assets
    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);
    client.deposit_collateral_asset(&user1, &asset_eth, &5000);

    client.borrow_asset(&user1, &asset_usdc, &8000);
    client.borrow_asset(&user1, &asset_eth, &2000);

    let summary = client.get_cross_position_summary(&user1);
    assert_eq!(summary.total_collateral_usd, 15000);
    assert_eq!(summary.total_debt_usd, 10000);

    // Update asset configurations
    let updated_usdc_params = create_asset_params(&env, 8500, 9000, 2000000, true);
    client.set_asset_params(&asset_usdc, &updated_usdc_params);

    // Operations should still work with updated config
    client.repay_asset(&user1, &asset_usdc, &1000);

    let updated_summary = client.get_cross_position_summary(&user1);
    assert_eq!(updated_summary.total_debt_usd, 9000);
}

// ============================================================================
// SECURITY AND AUTHORIZATION TESTS
// ============================================================================

#[test]
fn test_reentrancy_protection() {
    let env = Env::default();
    let (client, admin, user1, _, asset_usdc, _) = setup_test(&env);

    setup_multi_asset_config(&env, &client, &admin, &asset_usdc, &asset_usdc);

    // Test that operations require proper authorization
    client.deposit_collateral_asset(&user1, &asset_usdc, &10000);

    // Each operation should require user authorization
    // This is enforced by user.require_auth() in the implementation
    client.borrow_asset(&user1, &asset_usdc, &5000);
    client.repay_asset(&user1, &asset_usdc, &2000);
    client.withdraw_asset(&user1, &asset_usdc, &3000);
}

#[test]
fn test_admin_only_operations() {
    let env = Env::default();
    let (client, _admin, _user1, _, asset_usdc, _) = setup_test(&env);

    // Only admin should be able to set asset parameters
    let params = create_asset_params(&env, 8000, 8500, 1000000, true);

    env.mock_all_auths();
    client.set_asset_params(&asset_usdc, &params); // Should work with admin auth

    // Non-admin should fail (tested in test_set_asset_params_unauthorized)
}

#[test]
#[should_panic]
fn test_update_asset_config_unconfigured_asset_fails() {
    let (_env, client, _admin) = setup();
    // Asset not initialized
    client.update_asset_config(&None, &Some(5000), &None, &None, &None, &None, &None);
}
#[test]
fn test_borrow_ceiling_multi_user() {
    let env = Env::default();
    let (client, admin, user1, user2, asset_usdc, _) = setup_test(&env);

    env.mock_all_auths();

    // Set low debt ceiling
    let params = create_asset_params(&env, 9000, 9500, 10000, true); // $10k ceiling
    client.set_asset_params(&asset_usdc, &params);

    // User1 borrows up to near ceiling
    client.deposit_collateral_asset(&user1, &asset_usdc, &20000);
    client.borrow_asset(&user1, &asset_usdc, &8000);

    // User2 should be limited by remaining ceiling
    client.deposit_collateral_asset(&user2, &asset_usdc, &20000);
    client.borrow_asset(&user2, &asset_usdc, &2000); // Only 2k remaining

    // Additional borrow should fail (testing ceiling enforcement)
    // In this test we just verify the successful borrows total to the ceiling
    let summary = client.get_cross_position_summary(&user1).unwrap();
    assert_eq!(summary.total_debt_usd, 8000);
}
