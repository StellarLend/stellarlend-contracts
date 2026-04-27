use crate::{HelloContract, HelloContractClient, AmmProtocolConfig, TokenPair, SwapParams, LiquidityParams};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol, Vec,
};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn create_test_protocol_config(env: &Env, protocol_addr: &Address) -> AmmProtocolConfig {
    let mut supported_pairs = Vec::new(env);
    supported_pairs.push_back(TokenPair {
        token_a: None,                         // Native XLM
        token_b: Some(Address::generate(env)), // Mock USDC
        pool_address: Address::generate(env),
    });

    AmmProtocolConfig {
        protocol_address: protocol_addr.clone(),
        protocol_name: Symbol::new(env, "TestAMM"),
        enabled: true,
        fee_tier: 30, // 0.3%
        min_swap_amount: 1000,
        max_swap_amount: 1_000_000_000,
        supported_pairs,
    }
}

#[test]
fn test_amm_full_lifecycle() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);
    let token_b = Address::generate(&env);

    // 1. Initialize AMM settings
    client.initialize_amm(&admin, &100, &1000, &10000);

    // Verify settings
    // Note: We don't have a direct getter in HelloContract for all settings yet, 
    // but we can add one if needed or just test the operations.
    // The AMM library has get_amm_settings.

    // 2. Set AMM Pool
    let mut supported_pairs = Vec::new(&env);
    supported_pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_b.clone()),
        pool_address: Address::generate(&env),
    });

    let protocol_config = AmmProtocolConfig {
        protocol_address: protocol_addr.clone(),
        protocol_name: Symbol::new(&env, "TestAMM"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 1000,
        max_swap_amount: 1_000_000_000,
        supported_pairs,
    };
    client.set_amm_pool(&admin, &protocol_config);

    // 3. Test Swap
    let swap_params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(token_b.clone()),
        amount_in: 10000,
        min_amount_out: 9000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let amount_out = client.amm_swap(&user, &swap_params);
    assert_eq!(amount_out, 9900); // 1% slippage in mock

    // 4. Test Add Liquidity
    let lib_params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b.clone()),
        amount_a: 10000,
        amount_b: 10000,
        min_amount_a: 9000,
        min_amount_b: 9000,
        deadline: env.ledger().timestamp() + 3600,
    };

    let lp_tokens = client.amm_add_liquidity(&user, &lib_params);
    assert_eq!(lp_tokens, 10000);

    // 5. Test Remove Liquidity
    let (received_a, received_b) = client.amm_remove_liquidity(
        &user,
        &protocol_addr,
        &None,
        &Some(token_b.clone()),
        &5000,
        &4000,
        &4000,
        &(env.ledger().timestamp() + 3600),
    );
    assert_eq!(received_a, 5000);
    assert_eq!(received_b, 5000);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_amm_unauthorized_admin_operations() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let malicious_user = Address::generate(&env);

    client.initialize_amm(&admin, &100, &1000, &10000);

    let protocol_config = create_test_protocol_config(&env, &malicious_user);
    
    // Should fail because malicious_user is not admin
    client.set_amm_pool(&malicious_user, &protocol_config);
}

#[test]
fn test_amm_swap_invalid_params() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);

    client.initialize_amm(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env, &protocol_addr);
    client.set_amm_pool(&admin, &protocol_config);

    // Case 1: Deadline exceeded
    env.ledger().set_timestamp(2000);
    let swap_params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: 1000, // Past
    };

    let result = client.try_amm_swap(&user, &swap_params);
    assert!(result.is_err());
    // AmmError::SlippageExceeded is returned for deadline exceeded in this mock
}

#[test]
fn test_amm_swap_slippage_exceeded() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);

    client.initialize_amm(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env, &protocol_addr);
    client.set_amm_pool(&admin, &protocol_config);

    let swap_params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000,
        min_amount_out: 9950, // Mock will return 9900 (1% slippage), so this should fail
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = client.try_amm_swap(&user, &swap_params);
    assert!(result.is_err());
}

#[test]
fn test_amm_liquidity_invalid_token_pair() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);

    client.initialize_amm(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env, &protocol_addr);
    client.set_amm_pool(&admin, &protocol_config);

    let liq_params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: Some(Address::generate(&env)), // Not supported
        token_b: Some(Address::generate(&env)),
        amount_a: 10000,
        amount_b: 10000,
        min_amount_a: 5000,
        min_amount_b: 5000,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = client.try_amm_add_liquidity(&user, &liq_params);
    assert!(result.is_err());
}
