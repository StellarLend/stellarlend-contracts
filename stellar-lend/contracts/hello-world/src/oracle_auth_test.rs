#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

fn make_env() -> Env {
    Env::default()
}

fn with_contract<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce() -> T,
{
    let contract_id = env.register(crate::HelloContract {}, ());
    env.as_contract(&contract_id, f)
}

#[test]
fn update_price_feed_requires_caller_auth() {
    let env = make_env();
    with_contract(&env, || {
        let caller = Address::generate(&env);
        let asset = Address::generate(&env);
        let oracle = Address::generate(&env);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = crate::oracle::update_price_feed(&env, caller, asset, 1_000_000, 7, oracle);
        }));

        assert!(result.is_err(), "update_price_feed should require caller auth");
    });
}

#[test]
fn set_primary_oracle_requires_caller_auth() {
    let env = make_env();
    with_contract(&env, || {
        let caller = Address::generate(&env);
        let asset = Address::generate(&env);
        let primary_oracle = Address::generate(&env);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = crate::oracle::set_primary_oracle(&env, caller, asset, primary_oracle);
        }));

        assert!(result.is_err(), "set_primary_oracle should require caller auth");
    });
}
