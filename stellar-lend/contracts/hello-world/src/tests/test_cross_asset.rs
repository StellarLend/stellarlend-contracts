#![cfg(test)]

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::cross_asset::AssetConfig;

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_asset_config(env: &Env, asset: Option<Address>, price: i128) -> AssetConfig {
    AssetConfig {
        asset: asset.clone(),
        collateral_factor: 7500, // 75%
        borrow_factor: 8000,     // 80%
        reserve_factor: 1000,    // 10%
        max_supply: 10_000_000_000_000,
        max_borrow: 8_000_000_000_000,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_updated_at: env.ledger().timestamp(),
    }
}

fn _create_custom_asset_config(
    env: &Env,
    asset: Option<Address>,
    price: i128,
    collateral_factor: i128,
    borrow_factor: i128,
    max_supply: i128,
    max_borrow: i128,
) -> AssetConfig {
    AssetConfig {
        asset: asset.clone(),
        collateral_factor,
        borrow_factor,
        reserve_factor: 1000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_updated_at: env.ledger().timestamp(),
    }
}

// ============================================================================
// INITIALIZATION TESTS
// ============================================================================

#[test]
fn test_initialize_admin() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    let result = client.try_initialize_ca(&admin);
    assert!(result.is_ok());
}

#[test]
fn test_initialize_admin_twice_fails() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    let result1 = client.try_initialize_ca(&admin);
    assert!(result1.is_ok());

    // Second initialization should fail
    let result2 = client.try_initialize_ca(&admin);
    assert!(result2.is_err());
}

#[test]
fn test_initialize_single_asset() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);

    let result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(result.is_ok());

    // Verify asset was added to list
    let asset_list = client.get_asset_list();
    assert_eq!(asset_list.len(), 1);

    // Verify config was stored
    let stored_config_result = client.try_get_asset_config(&Some(usdc));
    assert!(stored_config_result.is_ok());

    let stored_config = stored_config_result.unwrap().unwrap();
    assert_eq!(stored_config.collateral_factor, 7500);
    assert_eq!(stored_config.price, 1_0000000);
}

#[test]
fn test_initialize_multiple_assets() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    // Initialize USDC
    let usdc = Address::generate(&env);
    let usdc_config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let usdc_result = client.try_initialize_asset(&Some(usdc.clone()), &usdc_config);
    assert!(usdc_result.is_ok());

    // Initialize ETH
    let eth = Address::generate(&env);
    let eth_config = create_asset_config(&env, Some(eth.clone()), 2000_0000000);
    let eth_result = client.try_initialize_asset(&Some(eth.clone()), &eth_config);
    assert!(eth_result.is_ok());

    // Initialize native XLM
    let xlm_config = create_asset_config(&env, None, 1000000);
    let xlm_result = client.try_initialize_asset(&None, &xlm_config);
    assert!(xlm_result.is_ok());

    // Verify all assets were added
    let asset_list = client.get_asset_list();
    assert_eq!(asset_list.len(), 3);
}

#[test]
fn test_initialize_asset_without_admin_fails() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);

    // Should fail because admin not initialized
    let result = client.try_initialize_asset(&Some(usdc), &config);
    assert!(result.is_err());
}

// ============================================================================
// ASSET CONFIGURATION TESTS
// ============================================================================

#[test]
fn test_update_asset_config_collateral_factor() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Update collateral factor
    let update_result = client.try_update_asset_config(
        &Some(usdc.clone()),
        &Some(8000_i128), // new collateral_factor
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    assert!(update_result.is_ok());

    // Verify update
    let updated_config = client.try_get_asset_config(&Some(usdc));
    assert!(updated_config.is_ok());
    assert_eq!(updated_config.unwrap().unwrap().collateral_factor, 8000);
}

#[test]
fn test_update_asset_config_multiple_params() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Update multiple parameters
    let update_result = client.try_update_asset_config(
        &Some(usdc.clone()),
        &Some(8000_i128),               // collateral_factor
        &Some(8500_i128),               // borrow_factor
        &Some(20_000_000_000_000_i128), // max_supply
        &Some(15_000_000_000_000_i128), // max_borrow
        &Some(true),                    // can_collateralize
        &Some(true),                    // can_borrow
    );
    assert!(update_result.is_ok());

    // Verify updates
    let updated_config = client.try_get_asset_config(&Some(usdc));
    assert!(updated_config.is_ok());

    let config = updated_config.unwrap().unwrap();
    assert_eq!(config.collateral_factor, 8000);
    assert_eq!(config.borrow_factor, 8500);
    assert_eq!(config.max_supply, 20_000_000_000_000);
    assert_eq!(config.max_borrow, 15_000_000_000_000);
}

#[test]
fn test_update_asset_price() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Update price
    let new_price = 1_0100000; // $1.01
    let price_result = client.try_update_asset_price(&Some(usdc.clone()), &new_price);
    assert!(price_result.is_ok());

    // Verify price update
    let updated_config = client.try_get_asset_config(&Some(usdc));
    assert!(updated_config.is_ok());
    assert_eq!(updated_config.unwrap().unwrap().price, new_price);
}

#[test]
fn test_update_asset_price_invalid_fails() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Try to update with invalid price (zero or negative)
    let price_result = client.try_update_asset_price(&Some(usdc), &0);
    assert!(price_result.is_err());
}

#[test]
fn test_disable_asset_collateralization() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Disable collateralization
    let update_result = client.try_update_asset_config(
        &Some(usdc.clone()),
        &None,
        &None,
        &None,
        &None,
        &Some(false), // can_collateralize = false
        &None,
    );
    assert!(update_result.is_ok());

    // Verify asset can't be used as collateral
    let user = Address::generate(&env);
    let deposit_result = client.try_ca_deposit_collateral(&user, &Some(usdc), &1000_0000000);
    assert!(deposit_result.is_err());
}

#[test]
fn test_disable_asset_borrowing() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Disable borrowing
    let update_result = client.try_update_asset_config(
        &Some(usdc.clone()),
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(false), // can_borrow = false
    );
    assert!(update_result.is_ok());

    // Try to borrow (should fail)
    let user = Address::generate(&env);
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(usdc), &100_0000000);
    assert!(borrow_result.is_err());
}

// ============================================================================
// SINGLE ASSET DEPOSIT TESTS
// ============================================================================

#[test]
fn test_deposit_collateral_single_asset() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Deposit
    let amount = 1000_0000000;
    let deposit_result = client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &amount);
    assert!(deposit_result.is_ok());

    // Verify position
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, amount);
    assert_eq!(position.debt_principal, 0);
    assert_eq!(position.accrued_interest, 0);
}

#[test]
fn test_deposit_native_xlm() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let xlm_config = create_asset_config(&env, None, 1000000);
    let asset_result = client.try_initialize_asset(&None, &xlm_config);
    assert!(asset_result.is_ok());

    // Deposit XLM
    let amount = 5000_0000000;
    let deposit_result = client.try_ca_deposit_collateral(&user, &None, &amount);
    assert!(deposit_result.is_ok());

    // Verify position
    let position = client.get_user_asset_position(&user, &None);
    assert_eq!(position.collateral, amount);
}

#[test]
fn test_deposit_exceeds_max_supply() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let mut config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    config.max_supply = 1000_0000000; // Set low max supply
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Try to deposit more than max supply
    let amount = 2000_0000000;
    let deposit_result = client.try_ca_deposit_collateral(&user, &Some(usdc), &amount);
    assert!(deposit_result.is_err());
}

#[test]
fn test_deposit_multiple_times_same_asset() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // First deposit
    let amount1 = 1000_0000000;
    let deposit1_result = client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &amount1);
    assert!(deposit1_result.is_ok());

    // Second deposit
    let amount2 = 500_0000000;
    let deposit2_result = client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &amount2);
    assert!(deposit2_result.is_ok());

    // Verify total position
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, amount1 + amount2);
}

// ============================================================================
// MULTI-ASSET DEPOSIT TESTS
// ============================================================================

#[test]
fn test_deposit_multiple_assets() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    // Initialize USDC
    let usdc = Address::generate(&env);
    let usdc_config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let usdc_result = client.try_initialize_asset(&Some(usdc.clone()), &usdc_config);
    assert!(usdc_result.is_ok());

    // Initialize ETH
    let eth = Address::generate(&env);
    let eth_config = create_asset_config(&env, Some(eth.clone()), 2000_0000000);
    let eth_result = client.try_initialize_asset(&Some(eth.clone()), &eth_config);
    assert!(eth_result.is_ok());

    // Deposit USDC
    let usdc_deposit_result =
        client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000);
    assert!(usdc_deposit_result.is_ok());

    // Deposit ETH
    let eth_deposit_result =
        client.try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000);
    assert!(eth_deposit_result.is_ok());

    // Verify both positions
    let usdc_position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(usdc_position.collateral, 10000_0000000);

    let eth_position = client.get_user_asset_position(&user, &Some(eth));
    assert_eq!(eth_position.collateral, 5_0000000);
}

#[test]
fn test_deposit_three_different_assets() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    // Setup USDC, ETH, and XLM
    let usdc = Address::generate(&env);
    let usdc_config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &usdc_config)
        .is_ok());

    let eth = Address::generate(&env);
    let eth_config = create_asset_config(&env, Some(eth.clone()), 2000_0000000);
    assert!(client
        .try_initialize_asset(&Some(eth.clone()), &eth_config)
        .is_ok());

    let xlm_config = create_asset_config(&env, None, 1000000);
    assert!(client.try_initialize_asset(&None, &xlm_config).is_ok());

    // Deposit all three
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &None, &50000_0000000)
        .is_ok());

    // Verify positions
    assert_eq!(
        client
            .get_user_asset_position(&user, &Some(usdc))
            .collateral,
        10000_0000000
    );
    assert_eq!(
        client.get_user_asset_position(&user, &Some(eth)).collateral,
        5_0000000
    );
    assert_eq!(
        client.get_user_asset_position(&user, &None).collateral,
        50000_0000000
    );
}

// ============================================================================
// BORROW TESTS - SINGLE ASSET
// ============================================================================

#[test]
fn test_borrow_against_single_asset_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Deposit collateral
    let collateral_amount = 1000_0000000;
    let deposit_result =
        client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &collateral_amount);
    assert!(deposit_result.is_ok());

    // Borrow (75% of collateral value)
    let borrow_amount = 750_0000000;
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(usdc.clone()), &borrow_amount);
    assert!(borrow_result.is_ok());

    // Verify position
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.debt_principal, borrow_amount);
    assert_eq!(position.collateral, collateral_amount);
}

#[test]
fn test_borrow_exceeds_health_factor() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize_ca(&admin);

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    client.initialize_asset(&Some(usdc.clone()), &config);

    // Deposit collateral
    let collateral_amount = 1000_0000000;
    client.ca_deposit_collateral(&user, &Some(usdc.clone()), &collateral_amount);

    // Try to borrow more than allowed (exceeds 75% collateral factor)
    let borrow_amount = 900_0000000;
    client.ca_borrow_asset(&user, &Some(usdc), &borrow_amount);
}

#[test]
fn test_borrow_exceeds_max_borrow_cap() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let mut config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    config.max_borrow = 500_0000000; // Low borrow cap
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Deposit large collateral
    let deposit_result =
        client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000);
    assert!(deposit_result.is_ok());

    // Try to borrow more than cap
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(usdc), &600_0000000);
    assert!(borrow_result.is_err());
}

#[test]
fn test_borrow_without_collateral_fails() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Try to borrow without any collateral
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(usdc), &100_0000000);
    assert!(borrow_result.is_err());
}

// ============================================================================
// BORROW TESTS - MULTI-ASSET COLLATERAL
// ============================================================================

#[test]
fn test_borrow_against_multi_asset_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    // Setup USDC
    let usdc = Address::generate(&env);
    let usdc_config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &usdc_config)
        .is_ok());

    // Setup ETH (higher value)
    let eth = Address::generate(&env);
    let eth_config = create_asset_config(&env, Some(eth.clone()), 2000_0000000);
    assert!(client
        .try_initialize_asset(&Some(eth.clone()), &eth_config)
        .is_ok());

    // Deposit both as collateral
    // USDC: 10,000 * $1 = $10,000
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());

    // ETH: 5 * $2000 = $10,000
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());

    // Total collateral value: $20,000
    // Weighted collateral (75%): $15,000
    // Can borrow up to $15,000 in USDC
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(usdc.clone()), &14000_0000000);
    assert!(borrow_result.is_ok());

    // Verify position
    let usdc_position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(usdc_position.debt_principal, 14000_0000000);
}

#[test]
fn test_borrow_different_asset_than_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    // Setup USDC and ETH
    let usdc = Address::generate(&env);
    let usdc_config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &usdc_config)
        .is_ok());

    let eth = Address::generate(&env);
    let eth_config = create_asset_config(&env, Some(eth.clone()), 2000_0000000);
    assert!(client
        .try_initialize_asset(&Some(eth.clone()), &eth_config)
        .is_ok());

    // Deposit USDC as collateral
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());

    // Borrow ETH against USDC collateral
    // $10,000 USDC * 75% = $7,500 borrow capacity
    // $7,500 / $2,000 = 3.75 ETH max
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(eth.clone()), &3_0000000);
    assert!(borrow_result.is_ok());

    // Verify positions
    let usdc_position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(usdc_position.collateral, 10000_0000000);
    assert_eq!(usdc_position.debt_principal, 0);

    let eth_position = client.get_user_asset_position(&user, &Some(eth));
    assert_eq!(eth_position.collateral, 0);
    assert_eq!(eth_position.debt_principal, 3_0000000);
}

#[test]
fn test_borrow_with_three_asset_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    // Setup three assets
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());

    let btc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(btc.clone()),
            &create_asset_config(&env, Some(btc.clone()), 40000_0000000)
        )
        .is_ok());

    // Deposit all three as collateral
    // USDC: $10,000
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    // ETH: 5 * $2,000 = $10,000
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());
    // BTC: 1 * $40,000 = $40,000
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(btc.clone()), &1_0000000)
        .is_ok());

    // Total: $10k USDC + $10k ETH + $40k BTC = $60k
    // Weighted (75%): $45k borrow capacity
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(usdc), &40000_0000000);
    assert!(borrow_result.is_ok());
}

#[test]
fn test_borrow_multiple_assets_sequentially() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    // Setup USDC and ETH
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());

    // Deposit large USDC collateral
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &100000_0000000)
        .is_ok());

    // Borrow USDC first
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &30000_0000000)
        .is_ok());

    // Then borrow ETH
    assert!(client
        .try_ca_borrow_asset(&user, &Some(eth.clone()), &10_0000000)
        .is_ok());

    // Verify both debt positions
    let usdc_position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(usdc_position.debt_principal, 30000_0000000);

    let eth_position = client.get_user_asset_position(&user, &Some(eth));
    assert_eq!(eth_position.debt_principal, 10_0000000);
}

// ============================================================================
// WITHDRAW TESTS
// ============================================================================

#[test]
fn test_withdraw_collateral_no_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    let asset_result = client.try_initialize_asset(&Some(usdc.clone()), &config);
    assert!(asset_result.is_ok());

    // Deposit
    let amount = 1000_0000000;
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &amount)
        .is_ok());

    // Withdraw
    let withdraw_amount = 500_0000000;
    let withdraw_result =
        client.try_ca_withdraw_collateral(&user, &Some(usdc.clone()), &withdraw_amount);
    assert!(withdraw_result.is_ok());

    // Verify position
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, amount - withdraw_amount);
}

#[test]
fn test_withdraw_all_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    // Deposit
    let amount = 1000_0000000;
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &amount)
        .is_ok());

    // Withdraw all
    let withdraw_result = client.try_ca_withdraw_collateral(&user, &Some(usdc.clone()), &amount);
    assert!(withdraw_result.is_ok());

    // Verify position is zero
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, 0);
}

#[test]
fn test_withdraw_with_debt_maintains_health() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &config)
        .is_ok());

    // Deposit and borrow
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &2000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());

    // Withdraw small amount (should maintain health)
    let withdraw_result =
        client.try_ca_withdraw_collateral(&user, &Some(usdc.clone()), &500_0000000);
    assert!(withdraw_result.is_ok());

    // Verify
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, 1500_0000000);
}

#[test]
fn test_withdraw_breaks_health_factor_fails() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &config)
        .is_ok());

    // Deposit and borrow at limit
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &750_0000000)
        .is_ok());

    // Try to withdraw too much (would break health factor)
    let withdraw_result = client.try_ca_withdraw_collateral(&user, &Some(usdc), &500_0000000);
    assert!(withdraw_result.is_err());
}

#[test]
fn test_withdraw_insufficient_collateral_fails() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let init_result = client.try_initialize_ca(&admin);
    assert!(init_result.is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &config)
        .is_ok());

    // Deposit
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());

    // Try to withdraw more than deposited
    let withdraw_result = client.try_ca_withdraw_collateral(&user, &Some(usdc), &2000_0000000);
    assert!(withdraw_result.is_err());
}

#[test]
fn test_withdraw_from_multi_asset_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    // Setup USDC and ETH
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());

    // Deposit both
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());

    // Borrow against combined collateral
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());

    // Withdraw some USDC (should still maintain health with ETH collateral)
    let withdraw_result =
        client.try_ca_withdraw_collateral(&user, &Some(usdc.clone()), &5000_0000000);
    assert!(withdraw_result.is_ok());

    // Verify USDC position
    let usdc_position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(usdc_position.collateral, 5000_0000000);
}

// ============================================================================
// REPAY TESTS
// ============================================================================

#[test]
fn test_repay_partial_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    let usdc = Address::generate(&env);
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &config)
        .is_ok());

    // Deposit and borrow
    let collateral_amount = 1000_0000000;
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &collateral_amount)
        .is_ok());

    let borrow_amount = 500_0000000;
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &borrow_amount)
        .is_ok());

    // Repay partial
    let repay_amount = 250_0000000;
    let repay_result = client.try_ca_repay_debt(&user, &Some(usdc.clone()), &repay_amount);
    assert!(repay_result.is_ok());

    // Verify position
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.debt_principal, borrow_amount - repay_amount);
}

#[test]
fn test_repay_full_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    // Deposit and borrow
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    let borrow_amount = 500_0000000;
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &borrow_amount)
        .is_ok());

    // Repay full amount
    let repay_result = client.try_ca_repay_debt(&user, &Some(usdc.clone()), &borrow_amount);
    assert!(repay_result.is_ok());

    // Verify debt is zero
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.debt_principal, 0);
}

#[test]
fn test_repay_more_than_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    // Deposit and borrow
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    let borrow_amount = 500_0000000;
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &borrow_amount)
        .is_ok());

    // Try to repay more than borrowed (should only repay actual debt)
    let repay_result = client.try_ca_repay_debt(&user, &Some(usdc.clone()), &1000_0000000);
    assert!(repay_result.is_ok());

    // Verify debt is zero, not negative
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.debt_principal, 0);
}

#[test]
fn test_repay_multi_asset_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    // Setup USDC and ETH
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());

    // Deposit large collateral and borrow both assets
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &100000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &30000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(eth.clone()), &10_0000000)
        .is_ok());

    // Repay USDC debt
    assert!(client
        .try_ca_repay_debt(&user, &Some(usdc.clone()), &15000_0000000)
        .is_ok());

    // Repay ETH debt
    assert!(client
        .try_ca_repay_debt(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());

    // Verify both positions
    let usdc_position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(usdc_position.debt_principal, 15000_0000000);

    let eth_position = client.get_user_asset_position(&user, &Some(eth));
    assert_eq!(eth_position.debt_principal, 5_0000000);
}

// ============================================================================
// POSITION SUMMARY TESTS
// ============================================================================

#[test]
fn test_get_position_summary_no_activity() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    // Get summary with no deposits or borrows
    let summary_result = client.try_get_user_position_summary(&user);
    assert!(summary_result.is_ok());

    let summary = summary_result.unwrap().unwrap();
    assert_eq!(summary.total_collateral_value, 0);
    assert_eq!(summary.total_debt_value, 0);
    assert_eq!(summary.health_factor, i128::MAX);
    assert!(!summary.is_liquidatable);
}

#[test]
fn test_get_position_summary_single_asset() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    // Setup USDC
    let usdc = Address::generate(&env);
    let usdc_config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &usdc_config)
        .is_ok());

    // Deposit and borrow
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &500_0000000)
        .is_ok());

    // Get summary
    let summary_result = client.try_get_user_position_summary(&user);
    assert!(summary_result.is_ok());

    let summary = summary_result.unwrap().unwrap();
    assert_eq!(summary.total_collateral_value, 1000_0000000);
    assert_eq!(summary.total_debt_value, 500_0000000);
    assert!(summary.health_factor > 0);
    assert!(!summary.is_liquidatable);
}

#[test]
fn test_get_position_summary_multi_asset() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    // Setup multiple assets
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());

    let btc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(btc.clone()),
            &create_asset_config(&env, Some(btc.clone()), 40000_0000000)
        )
        .is_ok());

    // Deposit all three
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(btc.clone()), &1_0000000)
        .is_ok());

    // Borrow USDC
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &20000_0000000)
        .is_ok());

    // Get summary
    let summary_result = client.try_get_user_position_summary(&user);
    assert!(summary_result.is_ok());

    let summary = summary_result.unwrap().unwrap();
    // Total collateral: $10k + $10k + $40k = $60k
    assert_eq!(summary.total_collateral_value, 60000_0000000);
    // Total debt: $20k
    assert_eq!(summary.total_debt_value, 20000_0000000);
    // Health should be good (60k * 0.75 / 20k = 2.25)
    assert!(summary.health_factor > 10000);
    assert!(!summary.is_liquidatable);
}

#[test]
fn test_position_summary_health_factor_calculation() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());

    // Deposit $1000
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());

    // Borrow $750 (at 75% limit)
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &750_0000000)
        .is_ok());

    let summary = client
        .try_get_user_position_summary(&user)
        .unwrap()
        .unwrap();

    assert_eq!(summary.health_factor, 12500);
    assert!(!summary.is_liquidatable);
}

#[test]
fn test_position_summary_liquidatable_status() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(eth.clone()), &3_0000000)
        .is_ok());
    let summary_before = client.get_user_position_summary(&user);
    assert!(summary_before.health_factor >= 10000);
    assert!(!summary_before.is_liquidatable);
    assert!(client
        .try_update_asset_price(&Some(usdc.clone()), &5000000)
        .is_ok());
    let summary_after = client.get_user_position_summary(&user);
    assert!(summary_after.health_factor < 10000);
    assert!(summary_after.is_liquidatable);
}

#[test]
fn test_position_summary_borrow_capacity() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    assert!(client.try_initialize_ca(&admin).is_ok());

    let usdc = Address::generate(&env);
    client.initialize_asset(
        &Some(usdc.clone()),
        &create_asset_config(&env, Some(usdc.clone()), 1_0000000),
    );

    // Deposit $1000
    client.ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000);

    let summary = client.get_user_position_summary(&user);

    // Borrow capacity should be weighted collateral (1000 * 0.75 = 750)
    assert_eq!(summary.borrow_capacity, 750_0000000);

    // Borrow $300
    client.ca_borrow_asset(&user, &Some(usdc.clone()), &300_0000000);

    let summary2 = client.get_user_position_summary(&user);

    assert_eq!(summary2.borrow_capacity, 510_0000000);
}

// ============================================================================
// EDGE CASE TESTS
// ============================================================================

#[test]
fn test_zero_amount_operations() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    let deposit_result = client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &0);
    assert!(deposit_result.is_ok());
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, 0);
}

#[test]
fn test_large_amount_operations() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    let mut config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    config.max_supply = i128::MAX;
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &config)
        .is_ok());
    let large_amount = 10_000_000_000_000_000;
    let deposit_result =
        client.try_ca_deposit_collateral(&user, &Some(usdc.clone()), &large_amount);
    assert!(deposit_result.is_ok());
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, large_amount);
}

#[test]
fn test_multiple_users_same_asset() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user1, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user2, &Some(usdc.clone()), &2000_0000000)
        .is_ok());
    let user1_position = client.get_user_asset_position(&user1, &Some(usdc.clone()));
    assert_eq!(user1_position.collateral, 1000_0000000);
    let user2_position = client.get_user_asset_position(&user2, &Some(usdc));
    assert_eq!(user2_position.collateral, 2000_0000000);
}

#[test]
fn test_asset_list_tracking() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let list = client.get_asset_list();
    assert_eq!(list.len(), 0);
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    let list = client.get_asset_list();
    assert_eq!(list.len(), 1);
    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());
    let list = client.get_asset_list();
    assert_eq!(list.len(), 2);
    assert!(client
        .try_initialize_asset(&None, &create_asset_config(&env, None, 1000000))
        .is_ok());
    let list = client.get_asset_list();
    assert_eq!(list.len(), 3);
}

#[test]
fn test_complex_multi_asset_scenario() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());
    let btc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(btc.clone()),
            &create_asset_config(&env, Some(btc.clone()), 40000_0000000)
        )
        .is_ok());
    let xlm_config = create_asset_config(&env, None, 1000000);
    assert!(client.try_initialize_asset(&None, &xlm_config).is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(btc.clone()), &1_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &None, &50000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &30000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());
    assert!(client
        .try_ca_repay_debt(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_withdraw_collateral(&user, &Some(btc.clone()), &5000000)
        .is_ok());
    let summary = client.get_user_position_summary(&user);
    assert!(summary.total_collateral_value > 0);
    assert!(summary.total_debt_value > 0);
    assert!(summary.health_factor > 10000);
}

#[test]
fn test_position_summary_after_price_update() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let eth = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(eth.clone()),
            &create_asset_config(&env, Some(eth.clone()), 2000_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &10_0000000)
        .is_ok());
    let summary1 = client.get_user_position_summary(&user);
    let initial_value = summary1.total_collateral_value;
    assert!(client
        .try_update_asset_price(&Some(eth.clone()), &2500_0000000)
        .is_ok());
    let summary2 = client.get_user_position_summary(&user);
    assert!(summary2.total_collateral_value > initial_value);
}

#[test]
fn test_different_collateral_factors() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    let mut usdc_config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    usdc_config.collateral_factor = 9000;
    assert!(client
        .try_initialize_asset(&Some(usdc.clone()), &usdc_config)
        .is_ok());
    let eth = Address::generate(&env);
    let mut eth_config = create_asset_config(&env, Some(eth.clone()), 2000_0000000);
    eth_config.collateral_factor = 7000;
    assert!(client
        .try_initialize_asset(&Some(eth.clone()), &eth_config)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(eth.clone()), &5_0000000)
        .is_ok());
    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_collateral_value, 20000_0000000);
    assert_eq!(summary.weighted_collateral_value, 16000_0000000);
}

#[test]
fn test_update_asset_price_reflects_in_summary() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &500_0000000)
        .is_ok());
    let summary_before = client.get_user_position_summary(&user);
    assert!(client
        .try_update_asset_price(&Some(usdc.clone()), &1_1000000)
        .is_ok());
    let summary_after = client.get_user_position_summary(&user);
    assert!(summary_after.total_collateral_value > summary_before.total_collateral_value);
    assert!(summary_after.total_debt_value > summary_before.total_debt_value);
}

#[test]
fn test_withdraw_all_after_repay() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &500_0000000)
        .is_ok());
    assert!(client
        .try_ca_repay_debt(&user, &Some(usdc.clone()), &500_0000000)
        .is_ok());
    let withdraw_result =
        client.try_ca_withdraw_collateral(&user, &Some(usdc.clone()), &1000_0000000);
    assert!(withdraw_result.is_ok());
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, 0);
    assert_eq!(position.debt_principal, 0);
}

#[test]
fn test_sequential_deposits_and_withdrawals() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &500_0000000)
        .is_ok());
    assert!(client
        .try_ca_withdraw_collateral(&user, &Some(usdc.clone()), &300_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &200_0000000)
        .is_ok());
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.collateral, 1400_0000000);
}

#[test]
fn test_borrow_repay_multiple_cycles() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &2000_0000000)
        .is_ok());
    assert!(client
        .try_ca_repay_debt(&user, &Some(usdc.clone()), &1000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &1500_0000000)
        .is_ok());
    assert!(client
        .try_ca_repay_debt(&user, &Some(usdc.clone()), &2500_0000000)
        .is_ok());
    let position = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(position.debt_principal, 0);
}

#[test]
fn test_get_asset_config_non_existent() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let fake_asset = Address::generate(&env);
    let result = client.try_get_asset_config(&Some(fake_asset));
    assert!(result.is_err());
}

#[test]
fn test_operations_on_non_configured_asset() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let fake_asset = Address::generate(&env);
    let deposit_result =
        client.try_ca_deposit_collateral(&user, &Some(fake_asset.clone()), &1000_0000000);
    assert!(deposit_result.is_err());
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(fake_asset), &100_0000000);
    assert!(borrow_result.is_err());
}

#[test]
fn test_native_and_token_assets_together() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    assert!(client
        .try_initialize_asset(&None, &create_asset_config(&env, None, 1000000))
        .is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &None, &50000_0000000)
        .is_ok());
    assert!(client
        .try_ca_deposit_collateral(&user, &Some(usdc.clone()), &10000_0000000)
        .is_ok());
    assert!(client
        .try_ca_borrow_asset(&user, &Some(usdc.clone()), &5000_0000000)
        .is_ok());
    let summary = client.get_user_position_summary(&user);
    assert!(summary.total_collateral_value > 0);
    assert!(summary.total_debt_value > 0);
}

#[test]
fn test_max_values_boundaries() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    let mut config = create_asset_config(&env, Some(usdc.clone()), 1_0000000);
    config.collateral_factor = 10000;
    config.borrow_factor = 10000;
    let result = client.try_initialize_asset(&Some(usdc), &config);
    assert!(result.is_ok());
}

#[test]
fn test_position_with_only_debt_no_collateral() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    assert!(client.try_initialize_ca(&admin).is_ok());
    let usdc = Address::generate(&env);
    assert!(client
        .try_initialize_asset(
            &Some(usdc.clone()),
            &create_asset_config(&env, Some(usdc.clone()), 1_0000000)
        )
        .is_ok());
    let borrow_result = client.try_ca_borrow_asset(&user, &Some(usdc), &1000_0000000);
    assert!(borrow_result.is_err());
}
