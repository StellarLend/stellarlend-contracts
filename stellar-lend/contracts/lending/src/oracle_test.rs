//! # Oracle Hardening Tests — Issue #428
//!
//! Covers `configure_oracle`, `set_primary_oracle`, `set_fallback_oracle`,
//! `update_price_feed`, and `get_price` with:
//! - Authorization enforcement (admin-only config; oracle-only updates)
//! - Stale price rejection at exact boundaries
//! - Fallback activation when primary is stale or missing
//! - Zero / negative price rejection
//! - Pause switch enforcement
//! - Self-referential oracle rejection
//! - Independent staleness per asset
//!
//! ## Security Notes
//! - A compromised primary oracle cannot update the fallback slot and vice versa.
//! - Stale prices are never silently accepted under any code path.
//! - `require_auth()` is called on every state-changing path.

use super::*;
use oracle::{OracleConfig, OracleError, OracleKey, PriceFeed};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn setup(env: &Env) -> (LendingContractClient<'_>, Address, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let asset = Address::generate(env);
    client.initialize(&admin, &1_000_000_000, &1000);
    (client, admin, asset, contract_id)
}

/// Write a price feed directly into storage to simulate staleness.
fn write_feed_at(
    env: &Env,
    contract_id: &Address,
    key: OracleKey,
    price: i128,
    timestamp: u64,
    oracle: &Address,
) {
    env.as_contract(contract_id, || {
        let feed = PriceFeed {
            price,
            last_updated: timestamp,
            oracle: oracle.clone(),
        };
        env.storage().persistent().set(&key, &feed);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// configure_oracle
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_configure_oracle_admin_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _asset, _cid) = setup(&env);

    let config = OracleConfig {
        max_staleness_seconds: 60,
    };
    client.configure_oracle(&admin, &config);
}

#[test]
fn test_configure_oracle_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _asset, _cid) = setup(&env);
    let stranger = Address::generate(&env);

    let config = OracleConfig {
        max_staleness_seconds: 60,
    };
    assert_eq!(
        client.try_configure_oracle(&stranger, &config),
        Err(Ok(OracleError::Unauthorized))
    );
}

#[test]
fn test_configure_oracle_zero_staleness_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _asset, _cid) = setup(&env);

    let config = OracleConfig {
        max_staleness_seconds: 0,
    };
    assert_eq!(
        client.try_configure_oracle(&admin, &config),
        Err(Ok(OracleError::InvalidPrice))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// set_primary_oracle / set_fallback_oracle
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_set_primary_oracle_admin_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
}

#[test]
fn test_set_primary_oracle_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, _cid) = setup(&env);
    let stranger = Address::generate(&env);
    let oracle = Address::generate(&env);

    assert_eq!(
        client.try_set_primary_oracle(&stranger, &asset, &oracle),
        Err(Ok(OracleError::Unauthorized))
    );
}

#[test]
fn test_set_primary_oracle_self_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, contract_id) = setup(&env);

    assert_eq!(
        client.try_set_primary_oracle(&admin, &asset, &contract_id),
        Err(Ok(OracleError::InvalidOracle))
    );
}

#[test]
fn test_set_fallback_oracle_admin_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    let fallback = Address::generate(&env);
    client.set_fallback_oracle(&admin, &asset, &fallback);
}

#[test]
fn test_set_fallback_oracle_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, _cid) = setup(&env);
    let stranger = Address::generate(&env);
    let fallback = Address::generate(&env);

    assert_eq!(
        client.try_set_fallback_oracle(&stranger, &asset, &fallback),
        Err(Ok(OracleError::Unauthorized))
    );
}

#[test]
fn test_set_fallback_oracle_self_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, contract_id) = setup(&env);

    assert_eq!(
        client.try_set_fallback_oracle(&admin, &asset, &contract_id),
        Err(Ok(OracleError::InvalidOracle))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// update_price_feed — authorization
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_update_price_feed_admin_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    client.update_price_feed(&admin, &asset, &100_000_000);
}

#[test]
fn test_update_price_feed_primary_oracle_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    let oracle = Address::generate(&env);

    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000);
}

#[test]
fn test_update_price_feed_fallback_oracle_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    let fallback = Address::generate(&env);

    client.set_fallback_oracle(&admin, &asset, &fallback);
    client.update_price_feed(&fallback, &asset, &100_000_000);
}

#[test]
fn test_update_price_feed_unauthorized_stranger() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, _cid) = setup(&env);
    let stranger = Address::generate(&env);

    assert_eq!(
        client.try_update_price_feed(&stranger, &asset, &100_000_000),
        Err(Ok(OracleError::Unauthorized))
    );
}

#[test]
fn test_update_price_feed_zero_price_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);

    assert_eq!(
        client.try_update_price_feed(&admin, &asset, &0),
        Err(Ok(OracleError::InvalidPrice))
    );
}

#[test]
fn test_update_price_feed_negative_price_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);

    assert_eq!(
        client.try_update_price_feed(&admin, &asset, &-1),
        Err(Ok(OracleError::InvalidPrice))
    );
}

/// Fallback oracle cannot write to the primary slot.
#[test]
fn test_fallback_oracle_writes_to_fallback_slot_only() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, contract_id) = setup(&env);
    let fallback = Address::generate(&env);

    client.set_fallback_oracle(&admin, &asset, &fallback);
    client.update_price_feed(&fallback, &asset, &100_000_000);

    // Primary slot should be empty; fallback slot should have the price.
    env.as_contract(&contract_id, || {
        let primary: Option<PriceFeed> = env
            .storage()
            .persistent()
            .get(&OracleKey::PrimaryFeed(asset.clone()));
        let fb: Option<PriceFeed> = env
            .storage()
            .persistent()
            .get(&OracleKey::FallbackFeed(asset.clone()));
        assert!(
            primary.is_none(),
            "fallback oracle must not write primary slot"
        );
        assert!(fb.is_some(), "fallback oracle must write fallback slot");
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// get_price — no feed
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_price_no_feed_returns_no_price_feed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, _cid) = setup(&env);

    assert_eq!(
        client.try_get_price(&asset),
        Err(Ok(OracleError::NoPriceFeed))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// get_price — staleness boundary
// ─────────────────────────────────────────────────────────────────────────────

/// Price at exactly `max_staleness_seconds` age is still valid.
#[test]
fn test_get_price_at_exact_staleness_boundary_valid() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.update_price_feed(&admin, &asset, &100_000_000);

    // Exactly at default threshold (3600s)
    env.ledger().with_mut(|li| li.timestamp = 3600);
    assert_eq!(client.get_price(&asset), 100_000_000);
}

/// Price one second past threshold is stale.
#[test]
fn test_get_price_one_second_past_threshold_stale() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.update_price_feed(&admin, &asset, &100_000_000);

    env.ledger().with_mut(|li| li.timestamp = 3601);
    assert_eq!(
        client.try_get_price(&asset),
        Err(Ok(OracleError::StalePrice))
    );
}

/// Custom staleness threshold is respected.
#[test]
fn test_get_price_custom_staleness_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);

    client.configure_oracle(
        &admin,
        &OracleConfig {
            max_staleness_seconds: 60,
        },
    );

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.update_price_feed(&admin, &asset, &100_000_000);

    // At exactly 60s — valid
    env.ledger().with_mut(|li| li.timestamp = 60);
    assert_eq!(client.get_price(&asset), 100_000_000);

    // At 61s — stale
    env.ledger().with_mut(|li| li.timestamp = 61);
    assert_eq!(
        client.try_get_price(&asset),
        Err(Ok(OracleError::StalePrice))
    );
}

/// Future timestamp in feed is treated as stale.
#[test]
fn test_get_price_future_timestamp_treated_as_stale() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, contract_id) = setup(&env);
    let oracle = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);

    // Write a feed with a future timestamp directly
    write_feed_at(
        &env,
        &contract_id,
        OracleKey::PrimaryFeed(asset.clone()),
        100_000_000,
        2000, // future
        &oracle,
    );

    assert_eq!(
        client.try_get_price(&asset),
        Err(Ok(OracleError::StalePrice))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// get_price — fallback logic
// ─────────────────────────────────────────────────────────────────────────────

/// Fallback is used when primary is stale.
#[test]
fn test_get_price_fallback_used_when_primary_stale() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    let fallback = Address::generate(&env);

    client.set_fallback_oracle(&admin, &asset, &fallback);

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.update_price_feed(&admin, &asset, &100_000_000);

    // Move past staleness threshold
    env.ledger().with_mut(|li| li.timestamp = 4000);

    // Submit fresh fallback price
    client.update_price_feed(&fallback, &asset, &105_000_000);

    assert_eq!(client.get_price(&asset), 105_000_000);
}

/// Fallback is used when primary feed is missing entirely.
#[test]
fn test_get_price_fallback_used_when_primary_missing() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    let fallback = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);
    client.set_fallback_oracle(&admin, &asset, &fallback);
    client.update_price_feed(&fallback, &asset, &99_000_000);

    assert_eq!(client.get_price(&asset), 99_000_000);
}

/// Stale fallback is also rejected.
#[test]
fn test_get_price_stale_fallback_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, contract_id) = setup(&env);
    let fallback = Address::generate(&env);

    client.set_fallback_oracle(&admin, &asset, &fallback);

    // Write stale fallback feed directly at t=0
    write_feed_at(
        &env,
        &contract_id,
        OracleKey::FallbackFeed(asset.clone()),
        105_000_000,
        0,
        &fallback,
    );

    // Move past staleness for both
    env.ledger().with_mut(|li| li.timestamp = 5000);

    assert_eq!(
        client.try_get_price(&asset),
        Err(Ok(OracleError::StalePrice))
    );
}

/// Both primary and fallback stale → StalePrice.
#[test]
fn test_get_price_both_stale_returns_stale_price() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);
    let fallback = Address::generate(&env);

    client.set_fallback_oracle(&admin, &asset, &fallback);

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.update_price_feed(&admin, &asset, &100_000_000);
    client.update_price_feed(&fallback, &asset, &105_000_000);

    env.ledger().with_mut(|li| li.timestamp = 5000);

    assert_eq!(
        client.try_get_price(&asset),
        Err(Ok(OracleError::StalePrice))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Pause
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_update_price_feed_paused_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);

    client.set_oracle_paused(&admin, &true);

    assert_eq!(
        client.try_update_price_feed(&admin, &asset, &100_000_000),
        Err(Ok(OracleError::OraclePaused))
    );
}

#[test]
fn test_update_price_feed_after_unpause_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _cid) = setup(&env);

    client.set_oracle_paused(&admin, &true);
    client.set_oracle_paused(&admin, &false);

    client.update_price_feed(&admin, &asset, &100_000_000);
}

#[test]
fn test_set_oracle_paused_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _asset, _cid) = setup(&env);
    let stranger = Address::generate(&env);

    assert_eq!(
        client.try_set_oracle_paused(&stranger, &true),
        Err(Ok(OracleError::Unauthorized))
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Multiple assets — independent staleness
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_multiple_assets_independent_staleness() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset1, _cid) = setup(&env);
    let asset2 = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.update_price_feed(&admin, &asset1, &100_000_000);

    env.ledger().with_mut(|li| li.timestamp = 2000);
    client.update_price_feed(&admin, &asset2, &200_000_000);

    // Move to where asset1 is stale but asset2 is not
    env.ledger().with_mut(|li| li.timestamp = 4000);

    assert_eq!(
        client.try_get_price(&asset1),
        Err(Ok(OracleError::StalePrice))
    );
    assert_eq!(client.get_price(&asset2), 200_000_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// views.rs integration — collateral/debt value uses oracle module
// ─────────────────────────────────────────────────────────────────────────────

/// Collateral value is 0 when no price feed is configured.
#[test]
fn test_collateral_value_zero_when_no_price_feed() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _asset, _cid) = setup(&env);
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &20_000);

    assert_eq!(client.get_collateral_value(&user), 0);
}

/// Collateral value is computed correctly when a fresh price is available.
#[test]
fn test_collateral_value_with_fresh_price() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _asset, _cid) = setup(&env);
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);
    // Price = 1.0 (100_000_000 with 8 decimals)
    client.update_price_feed(&admin, &collateral_asset, &100_000_000);

    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &20_000);

    // value = 20_000 * 100_000_000 / 100_000_000 = 20_000
    assert_eq!(client.get_collateral_value(&user), 20_000);
}

/// Collateral value falls to 0 when price becomes stale.
#[test]
fn test_collateral_value_zero_when_price_stale() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _asset, _cid) = setup(&env);
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 0);
    client.update_price_feed(&admin, &collateral_asset, &100_000_000);
    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &20_000);

    // Move past staleness threshold
    env.ledger().with_mut(|li| li.timestamp = 5000);

    assert_eq!(client.get_collateral_value(&user), 0);
}
