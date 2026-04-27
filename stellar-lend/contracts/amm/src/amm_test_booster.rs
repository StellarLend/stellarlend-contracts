use crate::amm::{
    add_liquidity, execute_swap, initialize_amm_settings, remove_liquidity, AmmError,
    LiquidityParams, SwapParams,
};
use crate::AmmContract;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

#[test]
fn test_amm_settings_already_initialized_coverage() {
    let env = Env::default();
    let contract_id = env.register(AmmContract, ());
    let admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        // First init
        let _ = initialize_amm_settings(&env, admin.clone(), 100, 500, 1000);

        // Second init should fail with AlreadyInitialized (handled by implementation, but let's trigger the branch)
        let res = initialize_amm_settings(&env, admin, 100, 500, 1000);
        assert!(res.is_err());
    });
}

#[test]
fn test_amm_slippage_exceeded_coverage() {
    let env = Env::default();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    let contract_id = env.register(AmmContract, ());
    let user = Address::generate(&env);
    let protocol = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let admin = Address::generate(&env);
        initialize_amm_settings(&env, admin, 100, 500, 1000).unwrap();

        // Test add_liquidity with deadline in the past
        let lp_params = LiquidityParams {
            protocol: protocol.clone(),
            token_a: Some(token_a.clone()),
            token_b: Some(token_b.clone()),
            amount_a: 1000,
            amount_b: 1000,
            min_amount_a: 900,
            min_amount_b: 900,
            deadline: 0, // Past
        };

        let res = add_liquidity(&env, user.clone(), lp_params);
        assert!(matches!(res, Err(AmmError::SlippageExceeded)));

        // Test remove_liquidity with deadline in the past
        let res_remove = remove_liquidity(
            &env,
            user.clone(),
            protocol.clone(),
            Some(token_a.clone()),
            Some(token_b.clone()),
            100, // lp_tokens
            0,   // min_amount_a
            0,   // min_amount_b
            0,   // deadline
        );
        assert!(matches!(res_remove, Err(AmmError::SlippageExceeded)));

        // Test execute_swap with deadline in the past
        let swap_params = SwapParams {
            protocol: protocol.clone(),
            token_in: Some(token_a.clone()),
            token_out: Some(token_b.clone()),
            amount_in: 500,
            min_amount_out: 450,
            slippage_tolerance: 100,
            deadline: 0,
        };

        let res_swap = execute_swap(&env, user.clone(), swap_params);
        assert!(matches!(res_swap, Err(AmmError::SlippageExceeded)));
    });
}
