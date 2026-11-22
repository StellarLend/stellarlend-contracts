//! Shared helpers for tests that call `swap_a_for_b` / `swap_b_for_a`.
//!
//! Swaps now move real tokens, so tests need Stellar Asset Contract tokens,
//! a funded trader, and a pool whose token balances back its reserves.

use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env};
use crate::AmmContractClient;

/// Balance minted to every test trader, per token.
pub const TRADER_FUNDS: i128 = 1_000_000_000_000_000_000;

/// Register a fresh SAC token and return its address.
pub fn register_token(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

/// Mint `ra` / `rb` to the pool if balances do not already back the reserves.
pub fn fund_pool(env: &Env, pool: &Address, ta: &Address, tb: &Address, ra: i128, rb: i128) {
    let bal_a = TokenClient::new(env, ta).balance(pool);
    if bal_a < ra {
        StellarAssetClient::new(env, ta).mint(pool, &(ra - bal_a));
    }
    let bal_b = TokenClient::new(env, tb).balance(pool);
    if bal_b < rb {
        StellarAssetClient::new(env, tb).mint(pool, &(rb - bal_b));
    }
}

/// Bootstrap a pool by funding the admin and calling `init_pool`.
pub fn bootstrap_pool(
    env: &Env,
    client: &AmmContractClient,
    admin: &Address,
    ta: &Address,
    tb: &Address,
    ra: i128,
    rb: i128,
) {
    if ra > 0 {
        StellarAssetClient::new(env, ta).mint(admin, &ra);
    }
    if rb > 0 {
        StellarAssetClient::new(env, tb).mint(admin, &rb);
    }
    client.init_pool(admin, &ra, &rb, ta, tb);
}

/// Assert that the pool satisfies solvency and LP accounting invariants.
pub fn assert_pool_invariants(
    env: &Env,
    pool: &Address,
    token_a: &Address,
    token_b: &Address,
    user_lp_balances: &[i128],
) {
    let client = AmmContractClient::new(env, pool);
    let (ra, rb) = client.get_reserves();
    let bal_a = TokenClient::new(env, token_a).balance(pool);
    let bal_b = TokenClient::new(env, token_b).balance(pool);

    assert!(bal_a >= ra, "Solvency violated for Token A: balance {} < reserve {}", bal_a, ra);
    assert!(bal_b >= rb, "Solvency violated for Token B: balance {} < reserve {}", bal_b, rb);

    let total_supply = client.get_total_supply();
    let locked = client.get_locked_liquidity();
    let sum_balances: i128 = user_lp_balances.iter().sum();
    assert_eq!(
        total_supply,
        sum_balances + locked,
        "LP accounting invariant violated: total_supply {} != sum_balances {} + locked {}",
        total_supply,
        sum_balances,
        locked
    );
}

/// Create a trader holding [`TRADER_FUNDS`] of both tokens.
pub fn new_trader(env: &Env, ta: &Address, tb: &Address) -> Address {
    let trader = Address::generate(env);
    StellarAssetClient::new(env, ta).mint(&trader, &TRADER_FUNDS);
    StellarAssetClient::new(env, tb).mint(&trader, &TRADER_FUNDS);
    trader
}
