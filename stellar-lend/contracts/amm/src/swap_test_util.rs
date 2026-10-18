//! Shared helpers for tests that call `swap_a_for_b` / `swap_b_for_a`.
//!
//! Swaps now move real tokens, so tests need Stellar Asset Contract tokens,
//! a funded trader, and a pool whose token balances back its reserves.

use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env};

/// Balance minted to every test trader, per token.
pub const TRADER_FUNDS: i128 = 1_000_000_000_000_000_000;

/// Register a fresh SAC token and return its address.
pub fn register_token(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

/// Mint `ra` / `rb` to the pool so its balances back virtual reserves set by
/// `init_pool`. Call after every `init_pool` that seeds non-zero reserves.
pub fn fund_pool(env: &Env, pool: &Address, ta: &Address, tb: &Address, ra: i128, rb: i128) {
    StellarAssetClient::new(env, ta).mint(pool, &ra);
    StellarAssetClient::new(env, tb).mint(pool, &rb);
}

/// Create a trader holding [`TRADER_FUNDS`] of both tokens.
pub fn new_trader(env: &Env, ta: &Address, tb: &Address) -> Address {
    let trader = Address::generate(env);
    StellarAssetClient::new(env, ta).mint(&trader, &TRADER_FUNDS);
    StellarAssetClient::new(env, tb).mint(&trader, &TRADER_FUNDS);
    trader
}
