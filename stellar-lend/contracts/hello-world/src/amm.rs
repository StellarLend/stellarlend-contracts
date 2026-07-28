use soroban_sdk::{contracttype, symbol_short, Address, Env};

use crate::amm_twap;

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

// ---------------------------------------------------------------------------
// Pool operations (used internally and by tests)
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

/// Execute a swap: if `a_for_b` is true, swap `amount` of token A for token B,
/// increasing reserve0 and decreasing reserve1. Otherwise swap token B for
/// token A, decreasing reserve0 and increasing reserve1.
pub fn swap(env: &Env, asset: &Address, amount: u128, a_for_b: bool) {
    assert!(amount > 0, "swap amount must be positive");
    let mut r = load_reserves(env, asset);
    assert!(r.reserve0 > 0 && r.reserve1 > 0, "pool not initialised");
    if a_for_b {
        r.reserve0 = r.reserve0.wrapping_add(amount);
        assert!(r.reserve1 > amount, "swap exceeds reserve1");
        r.reserve1 = r.reserve1.wrapping_sub(amount);
    } else {
        assert!(r.reserve0 > amount, "swap exceeds reserve0");
        r.reserve0 = r.reserve0.wrapping_sub(amount);
        r.reserve1 = r.reserve1.wrapping_add(amount);
    }
    commit_reserves(env, asset, &r);
}

/// Add liquidity to the pool, increasing both reserves.
pub fn add_liquidity(env: &Env, asset: &Address, add_a: u128, add_b: u128) {
    let mut r = load_reserves(env, asset);
    assert!(r.reserve0 > 0 && r.reserve1 > 0, "pool not initialised");
    r.reserve0 = r.reserve0.wrapping_add(add_a);
    r.reserve1 = r.reserve1.wrapping_add(add_b);
    commit_reserves(env, asset, &r);
}

/// Remove liquidity from the pool, decreasing both reserves.
pub fn remove_liquidity(env: &Env, asset: &Address, rem_a: u128, rem_b: u128) {
    let mut r = load_reserves(env, asset);
    assert!(r.reserve0 > rem_a, "remove exceeds reserve0");
    assert!(r.reserve1 > rem_b, "remove exceeds reserve1");
    r.reserve0 = r.reserve0.wrapping_sub(rem_a);
    r.reserve1 = r.reserve1.wrapping_sub(rem_b);
    commit_reserves(env, asset, &r);
}
