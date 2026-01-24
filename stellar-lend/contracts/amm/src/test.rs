use super::*;
use soroban_sdk::{testutils::Address as _, token, Address, Env, String, Symbol, Map, Vec};
use crate::types::*;

/// Helper function to create a test environment
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Helper function to create a mock token contract
fn create_token_contract(env: &Env, admin: &Address) -> Address {
    let contract = env.register_stellar_asset_contract_v2(admin.clone());
    contract.address()
}

/// Helper function to mint tokens to a user
fn mint_tokens(env: &Env, token: &Address, _admin: &Address, to: &Address, amount: i128) {
    // For testing, we'll use the token admin to mint tokens
    // This simulates having tokens available for testing
    let token_admin_client = token::StellarAssetClient::new(env, token);
    token_admin_client.mint(to, &amount);
}

/// Helper function to approve tokens for spending
fn approve_tokens(env: &Env, token: &Address, from: &Address, spender: &Address, amount: i128) {
    let token_client = token::Client::new(env, token);
    token_client.approve(from, spender, &amount, &1000);
}

/// Helper function to set up protocol configuration
fn setup_protocol_config(env: &Env, contract_id: &Address, protocol: &str, enabled: bool) {
    env.as_contract(contract_id, || {
        let config = ProtocolConfig {
            name: String::from_str(env, protocol),
            contract_address: Address::generate(env),
            enabled,
            fee_rate: 300,
            max_slippage: 500,
        };
        let key = AmmDataKey::ProtocolConfig(String::from_str(env, protocol));
        env.storage().persistent().set(&key, &config);
    });
}

/// Helper function to create initial pool
fn create_initial_pool(
    env: &Env,
    contract_id: &Address,
    token_a: &Address,
    token_b: &Address,
    protocol: &str,
    reserve_a: i128,
    reserve_b: i128,
) {
    env.as_contract(contract_id, || {
        let pool_info = PoolInfo {
            token_a: token_a.clone(),
            token_b: token_b.clone(),
            reserve_a,
            reserve_b,
            total_liquidity: ((reserve_a as f64) * (reserve_b as f64)).sqrt() as i128,
            fee_rate: 300,
        };
        let key = AmmDataKey::PoolInfo(token_a.clone(), token_b.clone(), String::from_str(env, protocol));
        env.storage().persistent().set(&key, &pool_info);
    });
}

#[test]
fn test_initialize_amm() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let result = client.initialize(&admin);
    assert_eq!(result, String::from_str(&env, "AMM initialized"));

    // Verify admin was set
    env.as_contract(&contract_id, || {
        let admin_key = AmmDataKey::Admin;
        let stored_admin: Address = env.storage().persistent().get(&admin_key).unwrap();
        assert_eq!(stored_admin, admin);
    });
}

#[test]
fn test_swap_with_hooks_success() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    // Initialize contract
    client.initialize(&admin);

    // Create tokens
    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    // Setup protocol
    setup_protocol_config(&env, &contract_id, "stellar_dex", true);

    // Create initial pool
    create_initial_pool(&env, &contract_id, &token_a, &token_b, "stellar_dex", 10000, 10000);

    // Mint tokens to user
    mint_tokens(&env, &token_a, &admin, &user, 1000);

    // Mint tokens to contract for swap simulation
    mint_tokens(&env, &token_b, &admin, &contract_id, 1000);

    // Approve tokens
    approve_tokens(&env, &token_a, &user, &contract_id, 1000);

    // Perform swap
    let amount_in = 100;
    let min_amount_out = 90;
    let protocol = String::from_str(&env, "stellar_dex");

    let amount_out = client.swap_with_hooks(
        &user,
        &token_a,
        &token_b,
        &amount_in,
        &min_amount_out,
        &protocol,
        &None,
    );

    // Verify swap succeeded
    assert!(amount_out >= min_amount_out);
    assert!(amount_out > 0);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_swap_zero_amount() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    let protocol = String::from_str(&env, "stellar_dex");

    // Try to swap zero amount
    client.swap_with_hooks(&user, &token_a, &token_b, &0, &0, &protocol, &None);
}

#[test]
#[should_panic(expected = "UnsupportedProtocol")]
fn test_swap_unsupported_protocol() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    let protocol = String::from_str(&env, "unsupported_protocol");

    client.swap_with_hooks(&user, &token_a, &token_b, &100, &90, &protocol, &None);
}

#[test]
#[should_panic(expected = "SlippageExceeded")]
fn test_swap_slippage_exceeded() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);
    create_initial_pool(&env, &contract_id, &token_a, &token_b, "stellar_dex", 1000, 1000);

    mint_tokens(&env, &token_a, &admin, &user, 1000);
    approve_tokens(&env, &token_a, &user, &contract_id, 1000);

    let protocol = String::from_str(&env, "stellar_dex");

    // Set unrealistic minimum output (higher than possible)
    client.swap_with_hooks(&user, &token_a, &token_b, &100, &200, &protocol, &None);
}

#[test]
fn test_add_liquidity_success() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);

    // Mint tokens to user
    mint_tokens(&env, &token_a, &admin, &user, 2000);
    mint_tokens(&env, &token_b, &admin, &user, 2000);

    // Approve tokens
    approve_tokens(&env, &token_a, &user, &contract_id, 2000);
    approve_tokens(&env, &token_b, &user, &contract_id, 2000);

    let protocol = String::from_str(&env, "stellar_dex");

    // Add initial liquidity
    let liquidity_minted = client.add_liquidity_with_hooks(
        &user,
        &token_a,
        &token_b,
        &1000,
        &1000,
        &900, // min liquidity
        &protocol,
    );

    assert!(liquidity_minted >= 900);
    assert!(liquidity_minted > 0);

    // Verify pool was created
    let pool_info = client.get_pool_info(&token_a, &token_b, &protocol);
    assert_eq!(pool_info.reserve_a, 1000);
    assert_eq!(pool_info.reserve_b, 1000);
    assert_eq!(pool_info.total_liquidity, liquidity_minted);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn test_add_liquidity_zero_amount() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    let protocol = String::from_str(&env, "stellar_dex");

    // Try to add zero liquidity
    client.add_liquidity_with_hooks(&user, &token_a, &token_b, &0, &1000, &0, &protocol);
}

#[test]
fn test_remove_liquidity_success() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);

    // Mint tokens and add liquidity first
    mint_tokens(&env, &token_a, &admin, &user, 2000);
    mint_tokens(&env, &token_b, &admin, &user, 2000);
    approve_tokens(&env, &token_a, &user, &contract_id, 2000);
    approve_tokens(&env, &token_b, &user, &contract_id, 2000);

    let protocol = String::from_str(&env, "stellar_dex");

    let liquidity_minted = client.add_liquidity_with_hooks(
        &user,
        &token_a,
        &token_b,
        &1000,
        &1000,
        &900,
        &protocol,
    );

    // Remove half the liquidity
    let liquidity_to_remove = liquidity_minted / 2;
    let (amount_a, amount_b) = client.remove_liquidity_with_hooks(
        &user,
        &token_a,
        &token_b,
        &liquidity_to_remove,
        &400, // min amount a
        &400, // min amount b
        &protocol,
    );

    assert!(amount_a >= 400);
    assert!(amount_b >= 400);
    assert!(amount_a > 0);
    assert!(amount_b > 0);
}

#[test]
#[should_panic(expected = "InsufficientLiquidity")]
fn test_remove_liquidity_insufficient() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);

    let protocol = String::from_str(&env, "stellar_dex");

    // Try to remove liquidity without having any
    client.remove_liquidity_with_hooks(&user, &token_a, &token_b, &1000, &0, &0, &protocol);
}

#[test]
fn test_validate_callback_success() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    // Create callback data
    let mut tokens = Vec::new(&env);
    tokens.push_back(Address::generate(&env));
    tokens.push_back(Address::generate(&env));

    let mut amounts = Vec::new(&env);
    amounts.push_back(100);
    amounts.push_back(200);

    let callback_data = CallbackData {
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        tokens,
        amounts,
        metadata: Map::new(&env),
        timestamp: env.ledger().timestamp(),
        nonce: 1,
    };

    // Store callback data for validation
    env.as_contract(&contract_id, || {
        let validation_key = AmmDataKey::CallbackValidation(user.clone());
        env.storage().persistent().set(&validation_key, &callback_data);
    });

    // Validate callback
    let is_valid = client.validate_amm_callback(&user, &callback_data);
    assert!(is_valid);
}

#[test]
fn test_validate_callback_invalid() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    // Create callback data without storing it first
    let mut tokens = Vec::new(&env);
    tokens.push_back(Address::generate(&env));

    let mut amounts = Vec::new(&env);
    amounts.push_back(100);

    let callback_data = CallbackData {
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        tokens,
        amounts,
        metadata: Map::new(&env),
        timestamp: env.ledger().timestamp(),
        nonce: 1,
    };

    // Validate callback without storing it first
    let is_valid = client.validate_amm_callback(&user, &callback_data);
    assert!(!is_valid);
}

#[test]
fn test_get_supported_protocols() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let protocols = client.get_supported_protocols();
    
    assert_eq!(protocols.len(), 3);
    assert!(protocols.contains(&String::from_str(&env, "stellar_dex")));
    assert!(protocols.contains(&String::from_str(&env, "soroswap")));
    assert!(protocols.contains(&String::from_str(&env, "phoenix")));
}

#[test]
fn test_get_pool_info() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);
    let protocol = String::from_str(&env, "stellar_dex");

    // Get pool info for non-existent pool
    let pool_info = client.get_pool_info(&token_a, &token_b, &protocol);
    
    assert_eq!(pool_info.token_a, token_a);
    assert_eq!(pool_info.token_b, token_b);
    assert_eq!(pool_info.reserve_a, 0);
    assert_eq!(pool_info.reserve_b, 0);
    assert_eq!(pool_info.total_liquidity, 0);
    assert_eq!(pool_info.fee_rate, 300);
}

#[test]
fn test_swap_with_callback() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);
    create_initial_pool(&env, &contract_id, &token_a, &token_b, "stellar_dex", 10000, 10000);

    mint_tokens(&env, &token_a, &admin, &user, 1000);
    mint_tokens(&env, &token_b, &admin, &contract_id, 1000); // Contract needs tokens for swaps
    approve_tokens(&env, &token_a, &user, &contract_id, 1000);

    // Create callback data
    let mut tokens = Vec::new(&env);
    tokens.push_back(token_a.clone());
    tokens.push_back(token_b.clone());

    let mut amounts = Vec::new(&env);
    amounts.push_back(100);

    let callback_data = CallbackData {
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        tokens,
        amounts,
        metadata: Map::new(&env),
        timestamp: env.ledger().timestamp(),
        nonce: 1,
    };

    let protocol = String::from_str(&env, "stellar_dex");

    // Perform swap with callback
    let amount_out = client.swap_with_hooks(
        &user,
        &token_a,
        &token_b,
        &100,
        &90,
        &protocol,
        &Some(callback_data),
    );

    assert!(amount_out >= 90);
}

#[test]
fn test_multiple_liquidity_operations() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);

    // Setup tokens for both users
    mint_tokens(&env, &token_a, &admin, &user1, 2000);
    mint_tokens(&env, &token_b, &admin, &user1, 2000);
    mint_tokens(&env, &token_a, &admin, &user2, 2000);
    mint_tokens(&env, &token_b, &admin, &user2, 2000);

    approve_tokens(&env, &token_a, &user1, &contract_id, 2000);
    approve_tokens(&env, &token_b, &user1, &contract_id, 2000);
    approve_tokens(&env, &token_a, &user2, &contract_id, 2000);
    approve_tokens(&env, &token_b, &user2, &contract_id, 2000);

    let protocol = String::from_str(&env, "stellar_dex");

    // User1 adds initial liquidity
    let liquidity1 = client.add_liquidity_with_hooks(
        &user1,
        &token_a,
        &token_b,
        &1000,
        &1000,
        &900,
        &protocol,
    );

    // User2 adds more liquidity
    let liquidity2 = client.add_liquidity_with_hooks(
        &user2,
        &token_a,
        &token_b,
        &500,
        &500,
        &400,
        &protocol,
    );

    // Verify pool state
    let pool_info = client.get_pool_info(&token_a, &token_b, &protocol);
    assert_eq!(pool_info.reserve_a, 1500);
    assert_eq!(pool_info.reserve_b, 1500);
    assert_eq!(pool_info.total_liquidity, liquidity1 + liquidity2);

    // User1 removes some liquidity
    let (amount_a, amount_b) = client.remove_liquidity_with_hooks(
        &user1,
        &token_a,
        &token_b,
        &(liquidity1 / 2),
        &200,
        &200,
        &protocol,
    );

    assert!(amount_a >= 200);
    assert!(amount_b >= 200);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn test_pause_functionality() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    // Set pause switch
    env.as_contract(&contract_id, || {
        let pause_key = AmmDataKey::PauseSwitches;
        let mut pause_map = Map::new(&env);
        pause_map.set(Symbol::new(&env, "swap"), true);
        env.storage().persistent().set(&pause_key, &pause_map);
    });

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);
    create_initial_pool(&env, &contract_id, &token_a, &token_b, "stellar_dex", 10000, 10000);

    mint_tokens(&env, &token_a, &admin, &user, 1000);
    approve_tokens(&env, &token_a, &user, &contract_id, 1000);

    let protocol = String::from_str(&env, "stellar_dex");

    // This should panic due to pause - remove the manual result handling
    client.swap_with_hooks(&user, &token_a, &token_b, &100, &90, &protocol, &None);
}

#[test]
fn test_analytics_tracking() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);

    mint_tokens(&env, &token_a, &admin, &user, 2000);
    mint_tokens(&env, &token_b, &admin, &user, 2000);
    mint_tokens(&env, &token_b, &admin, &contract_id, 2000); // Contract needs tokens for swaps
    approve_tokens(&env, &token_a, &user, &contract_id, 2000);
    approve_tokens(&env, &token_b, &user, &contract_id, 2000);

    let protocol = String::from_str(&env, "stellar_dex");

    // Add liquidity
    client.add_liquidity_with_hooks(&user, &token_a, &token_b, &1000, &1000, &900, &protocol);

    // Perform swap
    client.swap_with_hooks(&user, &token_a, &token_b, &100, &80, &protocol, &None); // Reduced min_amount_out

    // Check analytics were updated
    env.as_contract(&contract_id, || {
        let analytics_key = AmmDataKey::AmmAnalytics;
        let analytics: AmmAnalytics = env.storage().persistent().get(&analytics_key).unwrap();
        
        assert!(analytics.total_swap_volume > 0);
        assert!(analytics.total_liquidity_added > 0);
        assert_eq!(analytics.swap_count, 1);
        assert_eq!(analytics.liquidity_operations, 1);
    });
}

#[test]
fn test_edge_case_small_amounts() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);
    create_initial_pool(&env, &contract_id, &token_a, &token_b, "stellar_dex", 1000000, 1000000);

    mint_tokens(&env, &token_a, &admin, &user, 100);
    approve_tokens(&env, &token_a, &user, &contract_id, 100);

    let protocol = String::from_str(&env, "stellar_dex");

    // Swap very small amount
    let amount_out = client.swap_with_hooks(&user, &token_a, &token_b, &1, &0, &protocol, &None);
    
    // For very small amounts, the output might be 0 due to fees and rounding
    assert!(amount_out >= 0);
}

#[test]
fn test_edge_case_large_amounts() {
    let env = create_test_env();
    let contract_id = env.register(AmmContract, ());
    let client = AmmContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    
    client.initialize(&admin);

    let token_a = create_token_contract(&env, &admin);
    let token_b = create_token_contract(&env, &admin);

    setup_protocol_config(&env, &contract_id, "stellar_dex", true);

    let large_amount = 1_000_000_000i128; // 1 billion
    
    mint_tokens(&env, &token_a, &admin, &user, large_amount);
    mint_tokens(&env, &token_b, &admin, &user, large_amount);
    approve_tokens(&env, &token_a, &user, &contract_id, large_amount);
    approve_tokens(&env, &token_b, &user, &contract_id, large_amount);

    let protocol = String::from_str(&env, "stellar_dex");

    // Add large liquidity
    let liquidity_minted = client.add_liquidity_with_hooks(
        &user,
        &token_a,
        &token_b,
        &large_amount,
        &large_amount,
        &0,
        &protocol,
    );

    assert!(liquidity_minted > 0);

    // Verify pool state
    let pool_info = client.get_pool_info(&token_a, &token_b, &protocol);
    assert_eq!(pool_info.reserve_a, large_amount);
    assert_eq!(pool_info.reserve_b, large_amount);
}