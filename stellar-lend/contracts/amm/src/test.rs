use super::*;
use crate::amm::{AmmDataKey, *};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env, Symbol, Vec};

// Minimal mock AMM contract for tests that require cross-contract swap calls.
// execute_amm_swap invokes env.invoke_contract("swap", ...) on the registered protocol.
#[soroban_sdk::contract]
pub struct MockAmm;

#[soroban_sdk::contractimpl]
impl MockAmm {
    /// Returns amount_in * 99 / 100 (simulates 1% fee).
    pub fn swap(
        _env: Env,
        _executor: Address,
        _token_in: Option<Address>,
        _token_out: Option<Address>,
        amount_in: i128,
        _min_amount_out: i128,
        _callback: AmmCallbackData,
    ) -> i128 {
        amount_in * 99 / 100
    }
}

fn create_amm_contract<'a>(env: &Env) -> AmmContractClient<'a> {
    AmmContractClient::new(env, &env.register(AmmContract {}, ()))
}

fn create_test_protocol_config(env: &Env) -> AmmProtocolConfig {
    let protocol_addr = env.register(MockAmm, ());
    let mut supported_pairs = Vec::new(env);
    supported_pairs.push_back(TokenPair {
        token_a: None,                         // Native XLM
        token_b: Some(Address::generate(env)), // Mock USDC
        pool_address: Address::generate(env),
    });

    AmmProtocolConfig {
        protocol_address: protocol_addr,
        protocol_name: Symbol::new(env, "TestAMM"),
        enabled: true,
        fee_tier: 30, // 0.3%
        min_swap_amount: 1000,
        max_swap_amount: 1_000_000_000,
        supported_pairs,
    }
}

// Mock AMM contract for testing
#[contract]
pub struct MockAmm;

#[contractimpl]
impl MockAmm {
    pub fn swap(
        _env: Env,
        _executor: Address,
        _token_in: Option<Address>,
        _token_out: Option<Address>,
        amount_in: i128,
        _min_amount_out: i128,
        _callback_data: AmmCallbackData,
    ) -> i128 {
        // Simulate 1% fee
        amount_in
            .checked_mul(9900)
            .and_then(|v| v.checked_div(10000))
            .unwrap_or(0)
    }
}

#[test]
fn test_initialize_amm_settings() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);

    // Initialize AMM settings - this should not panic
    contract.initialize_amm_settings(
        &admin, &100,   // 1% default slippage
        &1000,  // 10% max slippage
        &10000, // 10000 auto-swap threshold
    );

    // Verify settings were stored
    let settings = contract.get_amm_settings();
    assert!(settings.is_some());
    let settings = settings.unwrap();
    assert_eq!(settings.default_slippage, 100);
    assert_eq!(settings.max_slippage, 1000);
    assert_eq!(settings.auto_swap_threshold, 10000);
    assert!(settings.swap_enabled);
    assert!(settings.liquidity_enabled);
}

#[test]
fn test_add_amm_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let _protocol_addr = Address::generate(&env);

    // Initialize first
    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    // Create protocol config (registers MockAmm)
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();

    // Add protocol - this should not panic
    contract.add_amm_protocol(&admin, &protocol_config);

    // Verify protocol was added
    let protocols = contract.get_amm_protocols();
    assert!(protocols.is_some());
    let protocols = protocols.unwrap();
    assert!(protocols.contains_key(protocol_addr.clone()));
}

#[test]
fn test_update_amm_settings() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);

    // Initialize
    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    // Update settings
    let new_settings = AmmSettings {
        default_slippage: 200,
        max_slippage: 2000,
        swap_enabled: false,
        liquidity_enabled: true,
        auto_swap_threshold: 20000,
    };

    contract.update_amm_settings(&admin, &new_settings);

    // Verify settings were updated
    let settings = contract.get_amm_settings().unwrap();
    assert_eq!(settings.default_slippage, 200);
    assert_eq!(settings.max_slippage, 2000);
    assert!(!settings.swap_enabled);
    assert!(settings.liquidity_enabled);
    assert_eq!(settings.auto_swap_threshold, 20000);
}

#[test]
fn test_successful_swap() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let protocol_addr = env.register(MockAmm, ());
    let token_b = Address::generate(&env);
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
    contract.add_amm_protocol(&admin, &protocol_config);

    // Execute swap
    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(token_b.clone()),
        amount_in: 10000,
        min_amount_out: 9000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let amount_out = contract.execute_swap(&user, &params);
    assert_eq!(amount_out, 9900); // 10000 * 99 / 100 = 9900 from MockAmm

    // Verify swap history
    let history = contract.get_swap_history(&Some(user), &10).unwrap();
    assert_eq!(history.len(), 1);
    let record = history.get(0).unwrap();
    assert_eq!(record.amount_in, 10000);
    assert_eq!(record.amount_out, 9900);
}

#[test]
fn test_swap_failure_insufficient_output() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000,
        min_amount_out: 10000, // Too high for 1% mock slippage
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_swap_failure_deadline_exceeded() {
    let env = Env::default();
    env.mock_all_auths();

    // Set a known timestamp
    env.ledger().set_timestamp(1000);

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: 999, // Before current ledger timestamp (1000)
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_swap_failure_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let _protocol_addr = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut settings = contract.get_amm_settings().unwrap();
    settings.swap_enabled = false;
    contract.update_amm_settings(&admin, &settings);

    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_add_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);
    let token_b = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

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
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b.clone()),
        amount_a: 10000,
        amount_b: 10000,
        min_amount_a: 9000,
        min_amount_b: 9000,
        deadline: env.ledger().timestamp() + 3600,
    };

    let lp_tokens = contract.add_liquidity(&user, &params);
    assert_eq!(lp_tokens, 10000);

    let history = contract.get_liquidity_history(&Some(user), &10).unwrap();
    assert_eq!(history.len(), 1);
}

#[test]
fn test_remove_liquidity() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);
    let token_b = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
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
    contract.add_amm_protocol(&admin, &protocol_config);

    // Seed pool with an initial position so LP shares exist.
    let seed_params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b.clone()),
        amount_a: 10_000,
        amount_b: 10_000,
        min_amount_a: 10_000,
        min_amount_b: 10_000,
        deadline: env.ledger().timestamp() + 3600,
    };
    let minted = contract.add_liquidity(&user, &seed_params);
    assert_eq!(minted, 10_000);

    let (amount_a, amount_b) = contract.remove_liquidity(
        &user,
        &protocol_addr,
        &None,
        &Some(token_b.clone()),
        &5000,
        &4000,
        &4000,
        &(env.ledger().timestamp() + 3600),
    );

    assert_eq!(amount_a, 5000);
    assert_eq!(amount_b, 5000);

    let history = contract.get_liquidity_history(&Some(user), &10).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(
        history.get(0).unwrap().operation_type,
        Symbol::new(&env, "remove")
    );
}

#[test]
fn test_add_liquidity_rounding_and_share_math() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);
    let token_b = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

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
    contract.add_amm_protocol(&admin, &protocol_config);

    // Initial mint: floor(sqrt(100 * 400)) = 200 LP
    let first = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b.clone()),
        amount_a: 100,
        amount_b: 400,
        min_amount_a: 100,
        min_amount_b: 400,
        deadline: env.ledger().timestamp() + 3600,
    };
    let first_lp = contract.add_liquidity(&user, &first);
    assert_eq!(first_lp, 200);

    // Proportional mint: min(50*200/100, 150*200/400) = min(100,75) = 75 LP
    // Burn 75 LP should return floor(75*100/200)=37 and floor(75*400/200)=150.
    let second = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: Some(token_b),
        amount_a: 50,
        amount_b: 150,
        min_amount_a: 37,
        min_amount_b: 150,
        deadline: env.ledger().timestamp() + 3600,
    };
    let second_lp = contract.add_liquidity(&user, &second);
    assert_eq!(second_lp, 75);

    let (out_a, out_b) = contract.remove_liquidity(
        &user,
        &protocol_addr,
        &None,
        &second.token_b,
        &75,
        &37,
        &150,
        &(env.ledger().timestamp() + 3600),
    );
    assert_eq!(out_a, 37);
    assert_eq!(out_b, 150);
}

#[test]
fn test_callback_validation() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let callback_data = AmmCallbackData {
        nonce: 999, // Wrong nonce
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_validate_amm_callback(&protocol_addr, &callback_data);
    assert!(result.is_err());
}

#[test]
fn test_auto_swap_for_collateral() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let token_out = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let mut supported_pairs = Vec::new(&env);
    supported_pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_out.clone()),
        pool_address: Address::generate(&env),
    });

    // Register MockAmm for auto-swap
    let protocol_addr = env.register(MockAmm, ());
    let protocol_config = AmmProtocolConfig {
        protocol_address: protocol_addr.clone(),
        protocol_name: Symbol::new(&env, "BestAMM"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 1000,
        max_swap_amount: 1_000_000_000,
        supported_pairs,
    };
    contract.add_amm_protocol(&admin, &protocol_config);

    let amount_out = contract.auto_swap_for_collateral(&user, &Some(token_out), &15000);
    assert_eq!(amount_out, 14850);
}

#[test]
fn test_swap_failure_unsupported_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(Address::generate(&env)),
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_swap_failure_invalid_token_pair() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: Some(Address::generate(&env)), // Not in supported pairs
        token_out: Some(Address::generate(&env)),
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_liquidity_failure_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut settings = contract.get_amm_settings().unwrap();
    settings.liquidity_enabled = false;
    contract.update_amm_settings(&admin, &settings);

    let protocol_config = create_test_protocol_config(&env);
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_a: 10000,
        amount_b: 10000,
        min_amount_a: 5000,
        min_amount_b: 5000,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_add_liquidity(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_get_history_with_limit() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    // Perform 3 swaps
    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    contract.execute_swap(&user, &params);
    contract.execute_swap(&user, &params);
    contract.execute_swap(&user, &params);

    // Get history with limit 2
    let history = contract.get_swap_history(&Some(user), &2).unwrap();
    assert_eq!(history.len(), 2);
}

#[test]
fn test_multiple_protocol_selection() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let token_out = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    // Protocol 1: Disabled
    let mut config1 = create_test_protocol_config(&env);
    config1.enabled = false;
    contract.add_amm_protocol(&admin, &config1);

    // Protocol 2: Enabled but doesn't support the pair
    let mut config2 = create_test_protocol_config(&env);
    config2.supported_pairs = Vec::new(&env); // No pairs supported
    contract.add_amm_protocol(&admin, &config2);

    // Protocol 3: Enabled and supports the pair
    let protocol3 = env.register(MockAmm, ());
    let mut supported_pairs = Vec::new(&env);
    supported_pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_out.clone()),
        pool_address: Address::generate(&env),
    });
    let config3 = AmmProtocolConfig {
        protocol_address: protocol3.clone(),
        protocol_name: Symbol::new(&env, "WorkingAMM"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 1000,
        max_swap_amount: 1_000_000_000,
        supported_pairs,
    };
    contract.add_amm_protocol(&admin, &config3);

    // Should pick Protocol 3
    let amount_out = contract.auto_swap_for_collateral(&user, &Some(token_out), &15000);
    assert_eq!(amount_out, 14850);
}

#[test]
fn test_swap_failure_max_input_exceeded() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut protocol_config = create_test_protocol_config(&env);
    protocol_config.max_swap_amount = 5000;
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000, // Exceeds max
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_swap_failure_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 0,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_admin_only_operations() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let new_settings = AmmSettings {
        default_slippage: 200,
        max_slippage: 2000,
        swap_enabled: true,
        liquidity_enabled: true,
        auto_swap_threshold: 20000,
    };

    let result = contract.try_update_amm_settings(&non_admin, &new_settings);
    assert!(result.is_err());
}

#[test]
fn test_callback_validation_expired() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let callback_data = AmmCallbackData {
        nonce: 1,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: 500, // Past deadline
    };

    env.ledger().set_timestamp(1000);

    let result = contract.try_validate_amm_callback(&protocol_addr, &callback_data);
    assert!(result.is_err());
}

#[test]
fn test_callback_validation_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let token_b = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let protocol_addr = env.register(MockAmm, ());
    let mut supported_pairs = Vec::new(&env);
    supported_pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_b.clone()),
        pool_address: Address::generate(&env),
    });
    let protocol_config = AmmProtocolConfig {
        protocol_address: protocol_addr.clone(),
        protocol_name: Symbol::new(&env, "Test"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 10,
        max_swap_amount: 1000000,
        supported_pairs,
    };
    contract.add_amm_protocol(&admin, &protocol_config);

    // Trigger an operation to increment nonce
    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(token_b.clone()),
        amount_in: 1000,
        min_amount_out: 100,
        slippage_tolerance: 100,
        deadline: 2000,
    };
    env.ledger().set_timestamp(1000);
    contract.execute_swap(&user, &params);

    // `execute_swap` allocates nonce 1 and `validate_amm_callback_core` consumes it, leaving stored nonce 2.
    let callback_data = AmmCallbackData {
        nonce: 2,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: 2000,
    };

    contract.validate_amm_callback(&protocol_addr, &callback_data);
}

#[test]
fn test_validate_amm_callback_fails_without_caller_auth() {
    let env = Env::default();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    env.mock_all_auths();
    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let callback_data = AmmCallbackData {
        nonce: 0,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_validate_amm_callback(&protocol_addr, &callback_data);
    assert!(result.is_err());
}

#[test]
fn test_validate_amm_callback_succeeds_with_caller_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let callback_data = AmmCallbackData {
        nonce: 0,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };

    assert!(contract
        .try_validate_amm_callback(&protocol_addr, &callback_data)
        .unwrap()
        .is_ok());
}

#[test]
fn test_callback_replay_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let callback_data = AmmCallbackData {
        nonce: 0,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };

    assert!(contract
        .try_validate_amm_callback(&protocol_addr, &callback_data)
        .unwrap()
        .is_ok());
    let replay = contract.try_validate_amm_callback(&protocol_addr, &callback_data);
    assert!(replay.is_err());
}

#[test]
fn test_callback_disabled_protocol_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    protocol_config.enabled = false;
    contract.add_amm_protocol(&admin, &protocol_config);

    let callback_data = AmmCallbackData {
        nonce: 0,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_validate_amm_callback(&protocol_addr, &callback_data);
    assert!(result.is_err());
}

#[test]
fn test_callback_unregistered_protocol_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let other_protocol = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    contract.add_amm_protocol(&admin, &protocol_config);

    let callback_data = AmmCallbackData {
        nonce: 0,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_validate_amm_callback(&other_protocol, &callback_data);
    assert!(result.is_err());
}

#[test]
fn test_callback_nonce_overflow_on_increment() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    env.as_contract(&contract.address, || {
        let nonce_key = AmmDataKey::CallbackNonces(user.clone());
        env.storage().persistent().set(&nonce_key, &u64::MAX);
    });

    let callback_data = AmmCallbackData {
        nonce: u64::MAX,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_validate_amm_callback(&protocol_addr, &callback_data);
    assert!(result.is_err());
}

#[test]
fn test_generate_callback_nonce_overflow_on_swap() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let token_b = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let protocol_addr = env.register(MockAmm, ());
    let mut supported_pairs = Vec::new(&env);
    supported_pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_b.clone()),
        pool_address: Address::generate(&env),
    });
    let protocol_config = AmmProtocolConfig {
        protocol_address: protocol_addr.clone(),
        protocol_name: Symbol::new(&env, "Test"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 10,
        max_swap_amount: 1000000,
        supported_pairs,
    };
    contract.add_amm_protocol(&admin, &protocol_config);

    env.as_contract(&contract.address, || {
        let nonce_key = AmmDataKey::CallbackNonces(user.clone());
        env.storage().persistent().set(&nonce_key, &u64::MAX);
    });

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(token_b.clone()),
        amount_in: 1000,
        min_amount_out: 100,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_edge_case_max_slippage() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = env.register(MockAmm, ());
    let token_b = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &2000, &10000); // 20% max slippage allowed

    let mut supported_pairs = Vec::new(&env);
    supported_pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_b.clone()),
        pool_address: Address::generate(&env),
    });
    let protocol_config = AmmProtocolConfig {
        protocol_address: protocol_addr.clone(),
        protocol_name: Symbol::new(&env, "Test"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 1,
        max_swap_amount: 1000000,
        supported_pairs,
    };
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: Some(token_b.clone()),
        amount_in: 10000,
        min_amount_out: 1,        // High slippage tolerance
        slippage_tolerance: 2000, // 20%
        deadline: 2000,
    };
    env.ledger().set_timestamp(1000);
    let amount_out = contract.execute_swap(&user, &params);
    assert!(amount_out > 0);
}

#[test]
fn test_edge_case_min_swap_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut protocol_config = create_test_protocol_config(&env);
    protocol_config.min_swap_amount = 5000;
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 1000, // Below min
        min_amount_out: 100,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_swap_failure_unauthorized() {
    let env = Env::default();
    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_config.protocol_address.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    // try_execute_swap without mock_all_auths or require_auth should fail
    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_delete_amm_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    // Verify it exists
    assert!(contract
        .get_amm_protocols()
        .unwrap()
        .contains_key(protocol_addr.clone()));

    // Delete it
    contract.delete_amm_protocol(&admin, &protocol_addr);

    // Verify it's gone
    assert!(!contract
        .get_amm_protocols()
        .unwrap()
        .contains_key(protocol_addr));
}

#[test]
fn test_validate_amm_callback_failures() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    // Get a valid nonce first (simulated by calling execute_swap or manually)
    // Actually, we can just guess.
    // The contract expects nonce to match current session.

    // 1. Wrong Operation
    let callback_data_wrong_op = AmmCallbackData {
        nonce: 1,
        operation: Symbol::new(&env, "wrong"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: env.ledger().timestamp() + 3600,
    };
    assert!(contract
        .try_validate_amm_callback(&protocol_addr, &callback_data_wrong_op)
        .is_err());

    // 2. Expired callback
    env.ledger().set_timestamp(10);
    let callback_data_expired = AmmCallbackData {
        nonce: 1,
        operation: Symbol::new(&env, "swap"),
        user: user.clone(),
        expected_amounts: Vec::new(&env),
        deadline: 5, // Before current ledger timestamp of 10
    };
    assert!(contract
        .try_validate_amm_callback(&protocol_addr, &callback_data_expired)
        .is_err());
}

#[test]
fn test_calculate_effective_price_zero() {
    // This tests the library function directly or via a wrapper if exposed
    // Since it's pub(crate), we can test it in a test module in amm.rs or here if visible.
    // In Soroban tests, we can call it if it's in the same crate.
    let result = calculate_effective_price(0, 100);
    assert!(result.is_err());
}

#[test]
fn test_remove_liquidity_edge_cases() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let protocol_addr = env.register(MockAmm, ());
    let token_b = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

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
    contract.add_amm_protocol(&admin, &protocol_config);

    // 1. Zero LP tokens
    let result = contract.try_remove_liquidity(
        &user,
        &protocol_addr,
        &None,
        &Some(token_b.clone()),
        &0,
        &100,
        &100,
        &2000,
    );
    assert!(result.is_err());

    // 2. Expired deadline
    env.ledger().set_timestamp(1000);
    let result = contract.try_remove_liquidity(
        &user,
        &protocol_addr,
        &None,
        &Some(token_b.clone()),
        &1000,
        &100,
        &100,
        &999,
    );
    assert!(result.is_err());
}

#[test]
fn test_update_amm_settings_individual() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let mut settings = contract.get_amm_settings().unwrap();
    settings.default_slippage = 150;
    contract.update_amm_settings(&admin, &settings);
    assert_eq!(contract.get_amm_settings().unwrap().default_slippage, 150);

    settings.swap_enabled = false;
    contract.update_amm_settings(&admin, &settings);
    assert!(!contract.get_amm_settings().unwrap().swap_enabled);
}

#[test]
fn test_swap_failure_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut settings = contract.get_amm_settings().unwrap();
    settings.swap_enabled = false;
    contract.update_amm_settings(&admin, &settings);

    let params = SwapParams {
        protocol: Address::generate(&env),
        token_in: None,
        token_out: Some(Address::generate(&env)),
        amount_in: 10000,
        min_amount_out: 5000,
        slippage_tolerance: 100,
        deadline: 2000,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_add_liquidity_zero_amounts() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_a: 0,
        amount_b: 1000,
        min_amount_a: 0,
        min_amount_b: 0,
        deadline: env.ledger().timestamp() + 3600,
    };
    let result = contract.try_add_liquidity(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_add_liquidity_expired_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(5000);

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let protocol_config = create_test_protocol_config(&env);
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = LiquidityParams {
        protocol: protocol_addr.clone(),
        token_a: None,
        token_b: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_a: 10000,
        amount_b: 10000,
        min_amount_a: 5000,
        min_amount_b: 5000,
        deadline: 4000, // Before current timestamp (5000)
    };
    let result = contract.try_add_liquidity(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_liquidity_failure_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut settings = contract.get_amm_settings().unwrap();
    settings.liquidity_enabled = false;
    contract.update_amm_settings(&admin, &settings);

    let params = LiquidityParams {
        protocol: Address::generate(&env),
        token_a: None,
        token_b: Some(Address::generate(&env)),
        amount_a: 10000,
        amount_b: 10000,
        min_amount_a: 5000,
        min_amount_b: 5000,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_add_liquidity(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_swap_with_max_amount_exceeded() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);
    let mut protocol_config = create_test_protocol_config(&env);
    protocol_config.max_swap_amount = 5000;
    let protocol_addr = protocol_config.protocol_address.clone();
    contract.add_amm_protocol(&admin, &protocol_config);

    let params = SwapParams {
        protocol: protocol_addr.clone(),
        token_in: None,
        token_out: protocol_config.supported_pairs.get(0).unwrap().token_b,
        amount_in: 10000, // exceeds max of 5000
        min_amount_out: 1000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3600,
    };

    let result = contract.try_execute_swap(&user, &params);
    assert!(result.is_err());
}

#[test]
fn test_get_swap_history_empty() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let history = contract.get_swap_history(&Some(user), &10);
    // Should return None or empty
    assert!(history.is_none() || history.unwrap().is_empty());
}

#[test]
fn test_get_liquidity_history_empty() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    contract.initialize_amm_settings(&admin, &100, &1000, &10000);

    let history = contract.get_liquidity_history(&Some(user), &10);
    assert!(history.is_none() || history.unwrap().is_empty());
}

#[test]
fn test_calculate_min_output_with_slippage_valid() {
    // Normal case: 10000 with 100 bps (1%) slippage -> 9900
    let result = calculate_min_output_with_slippage(10000, 100).unwrap();
    assert_eq!(result, 9900);

    // 0 bps slippage -> full amount
    let result = calculate_min_output_with_slippage(10000, 0).unwrap();
    assert_eq!(result, 10000);
}

#[test]
fn test_calculate_effective_price_normal() {
    // 10000 in, 9900 out -> price = 9900 * 10^18 / 10000
    let result = calculate_effective_price(10000, 9900).unwrap();
    assert_eq!(result, 990_000_000_000_000_000i128);
}
