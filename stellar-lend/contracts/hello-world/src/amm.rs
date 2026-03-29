use soroban_sdk::{Address, Env};
pub use stellarlend_amm::{
    AmmCallbackData, AmmError, AmmProtocolConfig, AmmSettings, LiquidityParams, MockAmm, SwapParams,
    TokenPair, AmmContract, AmmContractClient
};

/// Initialize AMM settings (admin only)
pub fn initialize_amm(
    env: Env,
    admin: Address,
    default_slippage: i128,
    max_slippage: i128,
    auto_swap_threshold: i128,
) -> Result<(), AmmError> {
    stellarlend_amm::initialize_amm_settings(
        &env,
        admin,
        default_slippage,
        max_slippage,
        auto_swap_threshold,
    )
}

/// Set AMM pool configuration (admin only)
pub fn set_amm_pool(
    env: Env,
    admin: Address,
    protocol_config: AmmProtocolConfig,
) -> Result<(), AmmError> {
    // In a real scenario, this would call the deployed AMM contract.
    // Since we are integrating it, we can use the library logic.
    // However, to make it truly integrated as a wrapper, we might want to store the state here
    // or call another contract.
    // For this implementation, we will use the library functions from stellarlend_amm.

    stellarlend_amm::add_amm_protocol(&env, admin, protocol_config)
}

/// Execute swap through AMM
pub fn amm_swap(env: Env, user: Address, params: SwapParams) -> Result<i128, AmmError> {
    stellarlend_amm::execute_swap(&env, user, params)
}

/// Add liquidity to AMM pool
pub fn amm_add_liquidity(
    env: Env,
    user: Address,
    params: LiquidityParams,
) -> Result<i128, AmmError> {
    stellarlend_amm::add_liquidity(&env, user, params)
}

/// Remove liquidity from AMM pool
pub fn amm_remove_liquidity(
    env: Env,
    user: Address,
    protocol: Address,
    token_a: Option<Address>,
    token_b: Option<Address>,
    lp_tokens: i128,
    min_amount_a: i128,
    min_amount_b: i128,
    deadline: u64,
) -> Result<(i128, i128), AmmError> {
    stellarlend_amm::remove_liquidity(
        &env,
        user,
        protocol,
        token_a,
        token_b,
        lp_tokens,
        min_amount_a,
        min_amount_b,
        deadline,
    )
}
