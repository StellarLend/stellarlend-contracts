//! Minimal AMM (Automated Market Maker) module for the hello-world contract.
//!
//! Implements a constant-product (x * y = k) AMM used as the data source for
//! the TWAP oracle fallback path.  All operations call
//! [`crate::amm_twap::update_twap_accumulators`] so the TWAP history tracks
//! every reserve change.
//!
//! ## Notes
//!
//! * Token transfers are **not** performed here — this module only maintains
//!   the on-chain reserve accounting.  In a production deployment the caller
//!   (contract entrypoint) would perform the actual token transfers before or
//!   after calling these helpers.
//! * A 0.3% swap fee is applied: `amount_in_after_fee = amount_in * 997 / 1000`.

use soroban_sdk::{contracttype, Address, Env};

use crate::amm_twap;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmmDataKey {
    /// Pool reserve storage for a given asset.
    Pool(Address),
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// On-chain pool reserves.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmmPool {
    /// Base-token reserve.
    pub reserve0: u128,
    /// Quote-token reserve.
    pub reserve1: u128,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise a new AMM pool with the given reserves.
///
/// Overwrites any existing pool state (use only for setup/testing).
/// Calls [`amm_twap::update_twap_accumulators`] to anchor the TWAP history.
pub fn initialise_pool(env: &Env, asset: &Address, reserve0: u128, reserve1: u128) {
    let pool = AmmPool { reserve0, reserve1 };
    env.storage()
        .persistent()
        .set(&AmmDataKey::Pool(asset.clone()), &pool);

    amm_twap::update_twap_accumulators(env, asset, reserve0, reserve1);
}

/// Execute a constant-product swap.
///
/// * When `is_buy = true`  the caller is sending `amount_in` of the **base**
///   token (reserve0) and receiving quote token (reserve1).
/// * When `is_buy = false` the caller is sending `amount_in` of the **quote**
///   token (reserve1) and receiving base token (reserve0).
///
/// A 0.3% fee is applied to `amount_in`.
///
/// # Returns
///
/// The amount of the output token delivered to the caller.
pub fn swap(env: &Env, asset: &Address, amount_in: u128, is_buy: bool) -> u128 {
    let mut pool: AmmPool = env
        .storage()
        .persistent()
        .get(&AmmDataKey::Pool(asset.clone()))
        .expect("pool not initialised");

    // Apply 0.3% fee.
    let amount_in_with_fee = amount_in.saturating_mul(997) / 1000;

    let amount_out = if is_buy {
        // Selling reserve0, buying reserve1.
        // out = reserve1 * amount_in_fee / (reserve0 + amount_in_fee)
        let numerator = pool.reserve1.saturating_mul(amount_in_with_fee);
        let denominator = pool.reserve0.saturating_add(amount_in_with_fee);
        let out = numerator / denominator;
        pool.reserve0 = pool.reserve0.saturating_add(amount_in);
        pool.reserve1 = pool.reserve1.saturating_sub(out);
        out
    } else {
        // Selling reserve1, buying reserve0.
        let numerator = pool.reserve0.saturating_mul(amount_in_with_fee);
        let denominator = pool.reserve1.saturating_add(amount_in_with_fee);
        let out = numerator / denominator;
        pool.reserve1 = pool.reserve1.saturating_add(amount_in);
        pool.reserve0 = pool.reserve0.saturating_sub(out);
        out
    };

    env.storage()
        .persistent()
        .set(&AmmDataKey::Pool(asset.clone()), &pool);

    amm_twap::update_twap_accumulators(env, asset, pool.reserve0, pool.reserve1);

    amount_out
}

/// Add liquidity to the pool (proportional deposit).
///
/// Simply increases both reserves by the specified amounts.  In a production
/// AMM the caller would also mint LP tokens; that is omitted here.
pub fn add_liquidity(env: &Env, asset: &Address, amount0: u128, amount1: u128) {
    let mut pool: AmmPool = env
        .storage()
        .persistent()
        .get(&AmmDataKey::Pool(asset.clone()))
        .expect("pool not initialised");

    pool.reserve0 = pool.reserve0.saturating_add(amount0);
    pool.reserve1 = pool.reserve1.saturating_add(amount1);

    env.storage()
        .persistent()
        .set(&AmmDataKey::Pool(asset.clone()), &pool);

    amm_twap::update_twap_accumulators(env, asset, pool.reserve0, pool.reserve1);
}

/// Remove liquidity from the pool.
///
/// Decreases both reserves by the specified amounts.  In a production AMM
/// the caller would burn LP tokens first; that is omitted here.
pub fn remove_liquidity(env: &Env, asset: &Address, amount0: u128, amount1: u128) {
    let mut pool: AmmPool = env
        .storage()
        .persistent()
        .get(&AmmDataKey::Pool(asset.clone()))
        .expect("pool not initialised");

    pool.reserve0 = pool.reserve0.saturating_sub(amount0);
    pool.reserve1 = pool.reserve1.saturating_sub(amount1);

    env.storage()
        .persistent()
        .set(&AmmDataKey::Pool(asset.clone()), &pool);

    amm_twap::update_twap_accumulators(env, asset, pool.reserve0, pool.reserve1);
}
