use soroban_sdk::{contracttype, symbol_short, Address, Env};
pub use stellarlend_amm::{AmmError, AmmProtocolConfig, LiquidityParams, SwapParams};

use crate::amm_twap;

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
    stellarlend_amm::add_amm_protocol(&env, admin, protocol_config)
}

/// Execute swap through AMM
pub fn amm_swap(env: Env, user: Address, params: SwapParams) -> Result<i128, AmmError> {
    let result = stellarlend_amm::execute_swap(&env, user, params)?;

    // Update TWAP accumulator after swap
    if let Some(asset) = get_swap_asset(&params) {
        if let Some((r0, r1)) = get_pool_reserves_after_swap(&env, &asset) {
            if r0 > 0 && r1 > 0 {
                amm_twap::update_twap_accumulators(&env, &asset, r0, r1);
            }
        }
    }

    Ok(result)
}

/// Add liquidity to AMM pool
pub fn amm_add_liquidity(
    env: Env,
    user: Address,
    params: LiquidityParams,
) -> Result<i128, AmmError> {
    let result = stellarlend_amm::add_liquidity(&env, user, params)?;

    // Update TWAP accumulator after liquidity change
    if let Some(asset) = get_liquidity_asset(&params) {
        if let Some((r0, r1)) = get_pool_reserves_after_swap(&env, &asset) {
            if r0 > 0 && r1 > 0 {
                amm_twap::update_twap_accumulators(&env, &asset, r0, r1);
            }
        }
    }

    Ok(result)
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
    let result = stellarlend_amm::remove_liquidity(
        &env,
        user,
        protocol,
        token_a.clone(),
        token_b,
        lp_tokens,
        min_amount_a,
        min_amount_b,
        deadline,
    )?;

    // Update TWAP accumulator after liquidity removal
    if let Some(asset) = token_a {
        if let Some((r0, r1)) = get_pool_reserves_after_swap(&env, &asset) {
            if r0 > 0 && r1 > 0 {
                amm_twap::update_twap_accumulators(&env, &asset, r0, r1);
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

/// Current reserve snapshot for a pool.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PoolReserves {
    /// Reserve of the base (tracked) token.
    pub reserve0: u128,
    /// Reserve of the paired (quote) token.
    pub reserve1: u128,
}

fn reserves_key(asset: &Address) -> (soroban_sdk::Symbol, Address) {
    (symbol_short!("AmmRes"), asset.clone())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_reserves(env: &Env, asset: &Address) -> PoolReserves {
    env.storage()
        .persistent()
        .get(&reserves_key(asset))
        .unwrap_or(PoolReserves {
            reserve0: 0,
            reserve1: 0,
        })
}

fn save_reserves(env: &Env, asset: &Address, reserves: &PoolReserves) {
    env.storage()
        .persistent()
        .set(&reserves_key(asset), reserves);
}

/// After any reserve mutation, persist the new state and update the TWAP
/// accumulator. Both operations are atomic within the same contract invocation.
fn commit_reserves(env: &Env, asset: &Address, r: &PoolReserves) {
    assert!(
        r.reserve0 > 0 && r.reserve1 > 0,
        "reserves must stay non-zero"
    );
    save_reserves(env, asset, r);
    amm_twap::update_twap_accumulators(env, asset, r.reserve0, r.reserve1);
}

/// Read reserves from storage (used for TWAP update after stellarlend_amm calls).
fn get_pool_reserves_after_swap(env: &Env, asset: &Address) -> Option<(u128, u128)> {
    let r = load_reserves(env, asset);
    if r.reserve0 > 0 && r.reserve1 > 0 {
        Some((r.reserve0, r.reserve1))
    } else {
        None
    }
}

/// Extract asset from SwapParams for TWAP update.
fn get_swap_asset(params: &SwapParams) -> Option<Address> {
    params.token_in.clone()
}

/// Extract asset from LiquidityParams for TWAP update.
fn get_liquidity_asset(params: &LiquidityParams) -> Option<Address> {
    params.token_a.clone()
}

// ---------------------------------------------------------------------------
// Direct pool operations (used internally and by tests)
// ---------------------------------------------------------------------------

/// Initialise a new pool with seed reserves. Can only be called once.
pub fn initialise_pool(env: &Env, asset: &Address, reserve0: u128, reserve1: u128) {
    assert!(reserve0 > 0 && reserve1 > 0, "seed reserves must be > 0");
    let existing: Option<PoolReserves> = env.storage().persistent().get(&reserves_key(asset));
    assert!(existing.is_none(), "pool already initialised");
    let r = PoolReserves { reserve0, reserve1 };
    commit_reserves(env, asset, &r);
}

/// Read the current reserves without mutating state.
pub fn get_reserves(env: &Env, asset: &Address) -> PoolReserves {
    load_reserves(env, asset)
}
