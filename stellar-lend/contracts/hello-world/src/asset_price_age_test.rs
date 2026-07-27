//! # Asset Price Age Unit & Integration Tests
//!
//! Verifies the staleness reporting functionality for per-asset oracle prices.
//! Asserts age equals `now - last_update_ts`, fresh updates reset age to zero,
//! unknown assets return typed errors without panic, and age grows monotonically.

#![cfg(test)]

use crate::cross_asset::{
    get_asset_price_age, initialize_asset, update_asset_price, AssetConfig, CrossAssetError,
    NoOpContract,
};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{Env, Address};

/// Helper to set up a test Soroban environment and register the test contract execution context.
fn make_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Helper to execute code inside a registered contract context.
fn with_contract<F, R>(env: &Env, f: F) -> R
where
    F: FnOnce() -> R,
{
    let contract_id = env.register(NoOpContract {}, ());
    env.as_contract(&contract_id, f)
}

/// Returns a default valid [`AssetConfig`] for test registration.
fn default_config(price: i128, price_decimals: u32) -> AssetConfig {
    AssetConfig {
        collateral_factor_bps: 7_500,
        liquidation_threshold: 8_000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_decimals,
        last_update_ts: 0,
    }
}

/// Asserts that `get_asset_price_age` returns `now - last_update_ts` after an `update_asset_price`.
#[test]
fn test_get_asset_price_age_equals_now_minus_ts() {
    let env = make_env();
    with_contract(&env, || {
        // Set initial ledger timestamp
        env.ledger().set_timestamp(1_000);

        // Register asset (initial timestamp set to 1000)
        initialize_asset(&env, None, default_config(1_000_000, 6)).unwrap();

        // Initial age at timestamp 1000 should be 0
        let age_init = get_asset_price_age(&env, None).unwrap();
        assert_eq!(age_init, 0);

        // Update asset price at timestamp 2_000
        env.ledger().set_timestamp(2_000);
        update_asset_price(&env, None, 1_050_000).unwrap();

        // Advance ledger timestamp to 2_500
        env.ledger().set_timestamp(2_500);

        // Age should equal 2500 - 2000 = 500
        let age = get_asset_price_age(&env, None).unwrap();
        assert_eq!(age, 500);
    });
}

/// Asserts that a fresh `update_asset_price` resets the age to 0 (near zero).
#[test]
fn test_fresh_update_resets_age() {
    let env = make_env();
    with_contract(&env, || {
        env.ledger().set_timestamp(10_000);
        initialize_asset(&env, None, default_config(1_000_000, 6)).unwrap();

        // Advance timestamp by 3,600 seconds (1 hour)
        env.ledger().set_timestamp(13_600);
        let age_before = get_asset_price_age(&env, None).unwrap();
        assert_eq!(age_before, 3_600);

        // A fresh price update at timestamp 13_600 resets age to 0
        update_asset_price(&env, None, 1_100_000).unwrap();
        let age_after = get_asset_price_age(&env, None).unwrap();
        assert_eq!(age_after, 0);
    });
}

/// Asserts that querying age for an unknown asset returns `CrossAssetError::AssetNotFound` without panicking.
#[test]
fn test_unknown_asset_returns_error_without_panic() {
    let env = make_env();
    with_contract(&env, || {
        let dummy_token = Address::generate(&env);
        let result = get_asset_price_age(&env, Some(dummy_token));
        assert_eq!(result, Err(CrossAssetError::AssetNotFound));
    });
}

/// Asserts monotonic age growth as ledger timestamp advances without price updates.
#[test]
fn test_monotonic_age_growth() {
    let env = make_env();
    with_contract(&env, || {
        env.ledger().set_timestamp(5_000);
        initialize_asset(&env, None, default_config(2_000_000, 6)).unwrap();

        let mut prev_age = get_asset_price_age(&env, None).unwrap();
        assert_eq!(prev_age, 0);

        for step in 1..=5 {
            let next_ts = 5_000 + (step * 100);
            env.ledger().set_timestamp(next_ts);
            let current_age = get_asset_price_age(&env, None).unwrap();

            assert_eq!(current_age, step * 100);
            assert!(current_age > prev_age, "Age must grow monotonically");
            prev_age = current_age;
        }
    });
}

/// Asserts that `HelloContractClient` contract wrapper exposes `get_asset_price_age` correctly.
#[test]
fn test_contract_client_get_asset_price_age() {
    let env = make_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    env.ledger().set_timestamp(50_000);
    client.initialize_asset(&None, &default_config(1_000_000, 6));

    // Update price at t=60,000
    env.ledger().set_timestamp(60_000);
    client.update_asset_price(&None, &1_200_000);

    // Check age at t=60_300
    env.ledger().set_timestamp(60_300);
    let age = client.get_asset_price_age(&None);
    assert_eq!(age, 300);

    // Query unregistered asset via client
    let unknown_token = Address::generate(&env);
    let unknown_res = client.try_get_asset_price_age(&Some(unknown_token));
    assert!(unknown_res.is_err());
}
