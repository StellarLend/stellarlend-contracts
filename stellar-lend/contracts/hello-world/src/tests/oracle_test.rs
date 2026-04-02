//! Oracle Price Feed Integration Tests
//!
//! This module contains comprehensive tests for the oracle price feed integration.
//! It covers:
//! - Price feed updates with validation
//! - Price staleness checks
//! - Price deviation validation
//! - Price caching with TTL
//! - Fallback oracle support with separate storage
//! - Admin authorization and oracle registration
//! - Edge cases and security scenarios
//!
//! # Security Assumptions
//! - Only admin can configure oracles and system parameters.
//! - Only registered primary/fallback oracles or admin can update prices.
//! - Price deviation checks prevent flash crash/manipulation attacks.
//! - Stale prices are rejected to prevent using outdated market data.
//!
//! # Test Scenarios
//! - `test_update_price_feed_success`: Basic price update by admin.
//! - `test_update_price_feed_by_oracle`: Update by registered primary oracle.
//! - `test_update_price_feed_malicious_caller`: Rejection of unauthorized oracle.
//! - `test_get_price_with_successful_fallback`: Fallback to secondary source when primary is stale.
//! - `test_price_deviation_*`: Validation of price change limits.
//! - `test_cache_*`: Validation of price caching and TTL.

use crate::oracle::{CachedPrice, OracleConfig, OracleDataKey, PriceFeed};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Map, Symbol,
};

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Creates a test environment with all auths mocked
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Sets up admin and initializes the contract
fn setup_contract_with_admin(env: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);

    // Initialize contract with admin
    client.initialize(&admin);

    (contract_id, admin, client)
}

/// Helper to get stored price feed directly from storage
fn get_price_feed(env: &Env, contract_id: &Address, asset: &Address) -> Option<PriceFeed> {
    env.as_contract(contract_id, || {
        let key = OracleDataKey::PriceFeed(asset.clone());
        env.storage()
            .persistent()
            .get::<OracleDataKey, PriceFeed>(&key)
    })
}

/// Helper to get cached price directly from storage
fn get_cached_price(env: &Env, contract_id: &Address, asset: &Address) -> Option<CachedPrice> {
    env.as_contract(contract_id, || {
        let key = OracleDataKey::PriceCache(asset.clone());
        env.storage()
            .persistent()
            .get::<OracleDataKey, CachedPrice>(&key)
    })
}

/// Helper to set oracle pause state
fn set_oracle_paused(env: &Env, contract_id: &Address, paused: bool) {
    env.as_contract(contract_id, || {
        let pause_key = OracleDataKey::PauseSwitches;
        let mut pause_map: Map<Symbol, bool> = Map::new(env);
        pause_map.set(Symbol::new(env, "pause_oracle"), paused);
        env.storage().persistent().set(&pause_key, &pause_map);
    });
}

// =============================================================================
// BASIC FUNCTIONALITY TESTS
// =============================================================================

/// Test successful price feed update by admin
#[test]
fn test_update_price_feed_success() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    let price = 100_000_000i128; // $1.00 with 8 decimals
    let decimals = 8u32;

    let result = client.update_price_feed(&admin, &asset, &price, &decimals, &oracle);
    assert_eq!(result, price);

    // Verify price feed was stored
    let stored_feed = get_price_feed(&env, &contract_id, &asset).unwrap();
    assert_eq!(stored_feed.price, price);
    assert_eq!(stored_feed.decimals, decimals);
    assert_eq!(stored_feed.oracle, oracle);
}

/// Test getting price for an asset
#[test]
fn test_get_price_success() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    let price = 250_000_000i128; // $2.50 with 8 decimals
    client.update_price_feed(&admin, &asset, &price, &8, &oracle);

    let retrieved_price = client.get_price(&asset);
    assert_eq!(retrieved_price, price);
}

/// Test price feed update by oracle (not admin)
#[test]
fn test_update_price_feed_by_oracle() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Set primary oracle
    client.set_primary_oracle(&admin, &asset, &oracle);

    // First update by admin
    let initial_price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &initial_price, &8, &oracle);

    // Second update by oracle itself
    let new_price = 101_000_000i128;
    let result = client.update_price_feed(&oracle, &asset, &new_price, &8, &oracle);
    assert_eq!(result, new_price);
}

// =============================================================================
// VALIDATION TESTS
// =============================================================================

/// Test zero price rejection
#[test]
#[should_panic(expected = "Oracle error")]
fn test_update_price_feed_zero_price() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    client.update_price_feed(&admin, &asset, &0, &8, &oracle);
}

/// Test negative price rejection
#[test]
#[should_panic(expected = "Oracle error")]
fn test_update_price_feed_negative_price() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    client.update_price_feed(&admin, &asset, &(-100_000_000), &8, &oracle);
}

/// Test unauthorized caller rejection
#[test]
#[should_panic(expected = "Oracle error")]
fn test_update_price_feed_unauthorized() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);
    let unauthorized = Address::generate(&env);

    client.update_price_feed(&unauthorized, &asset, &100_000_000, &8, &oracle);
}

/// Test malicious update where caller passes themselves as oracle
#[test]
#[should_panic(expected = "Oracle error")]
fn test_update_price_feed_malicious_caller() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let malicious = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Establish primary oracle first
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&admin, &asset, &100_000_000, &8, &oracle);

    // Malicious user tries to overwrite price by claiming to be a new oracle for this asset
    // This now fails because malicious user is not authorized.
    // Use a price within deviation range to ensure it fails due to authorization, not deviation.
    client.update_price_feed(&malicious, &asset, &101_000_000, &8, &malicious);
}

/// Test authorized oracle for asset A cannot update asset B.
#[test]
#[should_panic(expected = "Oracle error")]
fn test_update_price_feed_cross_asset_oracle_misuse() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);
    let oracle_a = Address::generate(&env);

    client.set_primary_oracle(&admin, &asset_a, &oracle_a);

    // oracle_a is only authorized for asset_a and must be rejected for asset_b.
    client.update_price_feed(&oracle_a, &asset_b, &100_000_000, &8, &oracle_a);
}

/// Test price deviation arithmetic overflow is rejected.
#[test]
#[should_panic(expected = "Oracle error")]
fn test_price_deviation_overflow_rejected() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    client.configure_oracle(
        &admin,
        &OracleConfig {
            max_deviation_bps: 10_000,
            max_staleness_seconds: 3600,
            cache_ttl_seconds: 300,
            min_price: 1,
            max_price: i128::MAX,
        },
    );

    // Seed with small non-zero old price so deviation math is exercised.
    client.update_price_feed(&admin, &asset, &1, &8, &oracle);

    // diff * 10000 overflows i128 and must be rejected.
    client.update_price_feed(&admin, &asset, &i128::MAX, &8, &oracle);
}

// =============================================================================
// PRICE DEVIATION TESTS
// =============================================================================

/// Test price deviation within acceptable range
#[test]
fn test_price_deviation_within_range() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Initial price
    let initial_price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &initial_price, &8, &oracle);

    // Price change within 5% (default max deviation)
    let new_price = 104_000_000i128; // 4% increase
    let result = client.update_price_feed(&admin, &asset, &new_price, &8, &oracle);
    assert_eq!(result, new_price);
}

/// Test price deviation exceeds maximum (should fail)
#[test]
#[should_panic(expected = "Oracle error")]
fn test_price_deviation_exceeds_maximum() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Initial price
    let initial_price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &initial_price, &8, &oracle);

    // Price change exceeds 5% (default max deviation)
    let new_price = 110_000_000i128; // 10% increase
    client.update_price_feed(&admin, &asset, &new_price, &8, &oracle);
}

/// Test price deviation with price decrease
#[test]
fn test_price_deviation_decrease_within_range() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Initial price
    let initial_price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &initial_price, &8, &oracle);

    // Price decrease within 5%
    let new_price = 96_000_000i128; // 4% decrease
    let result = client.update_price_feed(&admin, &asset, &new_price, &8, &oracle);
    assert_eq!(result, new_price);
}

// =============================================================================
// CACHING TESTS
// =============================================================================

/// Test price caching functionality
#[test]
fn test_price_caching() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);

    let price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price, &8, &oracle);

    // Verify cache was created
    let cached = get_cached_price(&env, &contract_id, &asset).unwrap();
    assert_eq!(cached.price, price);
    assert_eq!(cached.cached_at, 1000);
}

/// Test cache retrieval within TTL
#[test]
fn test_get_price_from_cache() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);

    let price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price, &8, &oracle);

    // Move time forward but still within TTL (default 300 seconds)
    env.ledger().with_mut(|li| li.timestamp = 1200);

    // Price should come from cache
    let retrieved_price = client.get_price(&asset);
    assert_eq!(retrieved_price, price);
}

/// Test cache expiration
#[test]
fn test_cache_expiration() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);

    let price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price, &8, &oracle);

    // Move time forward beyond TTL (default 300 seconds)
    env.ledger().with_mut(|li| li.timestamp = 1400);

    // Price should still be retrieved from feed (cache expired but feed still valid)
    let retrieved_price = client.get_price(&asset);
    assert_eq!(retrieved_price, price);
}

// =============================================================================
// FALLBACK ORACLE TESTS
// =============================================================================

/// Test setting fallback oracle
#[test]
fn test_set_fallback_oracle() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let fallback_oracle = Address::generate(&env);

    // Should succeed without panic
    client.set_fallback_oracle(&admin, &asset, &fallback_oracle);
}

/// Test setting fallback oracle unauthorized
#[test]
#[should_panic(expected = "Oracle error")]
fn test_set_fallback_oracle_unauthorized() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let fallback_oracle = Address::generate(&env);
    let unauthorized = Address::generate(&env);

    client.set_fallback_oracle(&unauthorized, &asset, &fallback_oracle);
}

/// Test fallback oracle cannot be contract itself
#[test]
#[should_panic(expected = "Oracle error")]
fn test_set_fallback_oracle_self() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);

    // Try to set fallback oracle as the contract itself
    client.set_fallback_oracle(&admin, &asset, &contract_id);
}

/// Test primary and fallback may be configured to the same oracle address.
#[test]
fn test_primary_and_fallback_same_address_supported() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let shared_oracle = Address::generate(&env);

    client.set_primary_oracle(&admin, &asset, &shared_oracle);
    client.set_fallback_oracle(&admin, &asset, &shared_oracle);

    let price = 123_000_000i128;
    client.update_price_feed(&shared_oracle, &asset, &price, &8, &shared_oracle);
    assert_eq!(client.get_price(&asset), price);
}

// =============================================================================
// ORACLE CONFIGURATION TESTS
// =============================================================================

/// Test configuring oracle parameters
#[test]
fn test_configure_oracle() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);

    let config = OracleConfig {
        max_deviation_bps: 1000,     // 10%
        max_staleness_seconds: 7200, // 2 hours
        cache_ttl_seconds: 600,      // 10 minutes
        min_price: 1,
        max_price: i128::MAX,
    };

    // Should succeed without panic
    client.configure_oracle(&admin, &config);
}

/// Test configure oracle unauthorized
#[test]
#[should_panic(expected = "Oracle error")]
fn test_configure_oracle_unauthorized() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let unauthorized = Address::generate(&env);

    let config = OracleConfig {
        max_deviation_bps: 1000,
        max_staleness_seconds: 7200,
        cache_ttl_seconds: 600,
        min_price: 1,
        max_price: i128::MAX,
    };

    client.configure_oracle(&unauthorized, &config);
}

/// Test invalid deviation configuration (zero)
#[test]
#[should_panic(expected = "Oracle error")]
fn test_configure_oracle_invalid_deviation_zero() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);

    let config = OracleConfig {
        max_deviation_bps: 0, // Invalid - must be > 0
        max_staleness_seconds: 3600,
        cache_ttl_seconds: 300,
        min_price: 1,
        max_price: i128::MAX,
    };

    client.configure_oracle(&admin, &config);
}

/// Test invalid deviation configuration (too high)
#[test]
#[should_panic(expected = "Oracle error")]
fn test_configure_oracle_invalid_deviation_too_high() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);

    let config = OracleConfig {
        max_deviation_bps: 15000, // Invalid - > 10000 (100%)
        max_staleness_seconds: 3600,
        cache_ttl_seconds: 300,
        min_price: 1,
        max_price: i128::MAX,
    };

    client.configure_oracle(&admin, &config);
}

/// Test invalid staleness configuration (zero)
#[test]
#[should_panic(expected = "Oracle error")]
fn test_configure_oracle_invalid_staleness_zero() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);

    let config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 0, // Invalid - must be > 0
        cache_ttl_seconds: 300,
        min_price: 1,
        max_price: i128::MAX,
    };

    client.configure_oracle(&admin, &config);
}

// =============================================================================
// PAUSE FUNCTIONALITY TESTS
// =============================================================================

/// Test oracle updates when paused
#[test]
#[should_panic(expected = "Oracle error")]
fn test_update_price_feed_when_paused() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Pause oracle updates
    set_oracle_paused(&env, &contract_id, true);

    // Try to update price - should fail
    client.update_price_feed(&admin, &asset, &100_000_000, &8, &oracle);
}

/// Test oracle operations after unpausing
#[test]
fn test_update_price_feed_after_unpause() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Pause and then unpause
    set_oracle_paused(&env, &contract_id, true);
    set_oracle_paused(&env, &contract_id, false);

    // Should succeed after unpause
    let price = 100_000_000i128;
    let result = client.update_price_feed(&admin, &asset, &price, &8, &oracle);
    assert_eq!(result, price);
}

// =============================================================================
// STALENESS TESTS
// =============================================================================

/// Test price retrieval with stale primary feed and no fallback
#[test]
#[should_panic(expected = "Oracle error")]
fn test_get_price_stale_no_fallback() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);

    let price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price, &8, &oracle);

    // Clear cache
    env.as_contract(&contract_id, || {
        let cache_key = OracleDataKey::PriceCache(asset.clone());
        env.storage()
            .persistent()
            .remove::<OracleDataKey>(&cache_key);
    });

    // Move time far beyond staleness threshold (default 3600 seconds)
    env.ledger().with_mut(|li| li.timestamp = 10000);

    // Should fail - price is stale and no fallback configured
    client.get_price(&asset);
}

/// Test get_price fails when neither primary feed nor fallback oracle is configured.
#[test]
#[should_panic(expected = "Oracle error")]
fn test_get_price_no_feed_no_fallback_configured() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);

    client.get_price(&asset);
}

/// Test successful price retrieval from fallback oracle when primary is stale
#[test]
fn test_get_price_with_successful_fallback() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let primary_oracle = Address::generate(&env);
    let fallback_oracle = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);

    // Set fallback oracle
    client.set_fallback_oracle(&admin, &asset, &fallback_oracle);

    // Set primary oracle
    client.set_primary_oracle(&admin, &asset, &primary_oracle);

    // Set primary price
    client.update_price_feed(&admin, &asset, &100_000_000, &8, &primary_oracle);

    // Move time forward beyond staleness threshold (3600s)
    env.ledger().with_mut(|li| li.timestamp = 5000);

    // Set fallback price
    let fallback_price = 105_000_000i128;
    client.update_price_feed(
        &fallback_oracle,
        &asset,
        &fallback_price,
        &8,
        &fallback_oracle,
    );

    // Should return fallback price
    let retrieved_price = client.get_price(&asset);
    assert_eq!(retrieved_price, fallback_price);
}

/// Test retrieval fails when both primary and fallback are stale
#[test]
#[should_panic(expected = "Oracle error")]
fn test_get_price_both_stale() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let primary_oracle = Address::generate(&env);
    let fallback_oracle = Address::generate(&env);

    client.set_primary_oracle(&admin, &asset, &primary_oracle);
    client.set_fallback_oracle(&admin, &asset, &fallback_oracle);

    env.ledger().with_mut(|li| li.timestamp = 1000);
    client.update_price_feed(&admin, &asset, &100_000_000, &8, &primary_oracle);
    client.update_price_feed(&fallback_oracle, &asset, &105_000_000, &8, &fallback_oracle);

    // Move time beyond staleness threshold for both
    env.ledger().with_mut(|li| li.timestamp = 10000);

    client.get_price(&asset);
}

// =============================================================================
// MULTIPLE ASSETS TESTS
// =============================================================================

/// Test updating prices for multiple assets
#[test]
fn test_multiple_asset_prices() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let oracle = Address::generate(&env);

    let asset1 = Address::generate(&env);
    let asset2 = Address::generate(&env);
    let asset3 = Address::generate(&env);

    let price1 = 100_000_000i128; // Asset 1: $1.00
    let price2 = 250_000_000i128; // Asset 2: $2.50
    let price3 = 50_000_000i128; // Asset 3: $0.50

    client.update_price_feed(&admin, &asset1, &price1, &8, &oracle);
    client.update_price_feed(&admin, &asset2, &price2, &8, &oracle);
    client.update_price_feed(&admin, &asset3, &price3, &8, &oracle);

    assert_eq!(client.get_price(&asset1), price1);
    assert_eq!(client.get_price(&asset2), price2);
    assert_eq!(client.get_price(&asset3), price3);
}

/// Test different oracles for different assets
#[test]
fn test_different_oracles_per_asset() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);

    let asset1 = Address::generate(&env);
    let asset2 = Address::generate(&env);
    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);

    let price1 = 100_000_000i128;
    let price2 = 200_000_000i128;

    client.update_price_feed(&admin, &asset1, &price1, &8, &oracle1);
    client.update_price_feed(&admin, &asset2, &price2, &8, &oracle2);

    // Verify different oracles are stored
    let feed1 = get_price_feed(&env, &contract_id, &asset1).unwrap();
    let feed2 = get_price_feed(&env, &contract_id, &asset2).unwrap();

    assert_eq!(feed1.oracle, oracle1);
    assert_eq!(feed2.oracle, oracle2);
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

/// Test price at minimum valid value
#[test]
fn test_price_minimum_valid() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    let min_price = 1i128;
    let result = client.update_price_feed(&admin, &asset, &min_price, &8, &oracle);
    assert_eq!(result, min_price);
}

/// Test price at maximum i128 value
#[test]
fn test_price_maximum_valid() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Use a large but safe price value
    let large_price = 1_000_000_000_000_000_000i128;
    let result = client.update_price_feed(&admin, &asset, &large_price, &8, &oracle);
    assert_eq!(result, large_price);
}

/// Test different decimal configurations
#[test]
fn test_different_decimals() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let oracle = Address::generate(&env);

    let asset_6_decimals = Address::generate(&env);
    let asset_8_decimals = Address::generate(&env);
    let asset_18_decimals = Address::generate(&env);

    client.update_price_feed(&admin, &asset_6_decimals, &1_000_000, &6, &oracle);
    client.update_price_feed(&admin, &asset_8_decimals, &100_000_000, &8, &oracle);
    client.update_price_feed(
        &admin,
        &asset_18_decimals,
        &1_000_000_000_000_000_000i128,
        &18,
        &oracle,
    );

    let feed_6 = get_price_feed(&env, &contract_id, &asset_6_decimals).unwrap();
    let feed_8 = get_price_feed(&env, &contract_id, &asset_8_decimals).unwrap();
    let feed_18 = get_price_feed(&env, &contract_id, &asset_18_decimals).unwrap();

    assert_eq!(feed_6.decimals, 6);
    assert_eq!(feed_8.decimals, 8);
    assert_eq!(feed_18.decimals, 18);
}

/// Test timestamp edge case - same timestamp update
#[test]
fn test_same_timestamp_update() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    env.ledger().with_mut(|li| li.timestamp = 1000);

    let price1 = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price1, &8, &oracle);

    // Update again with same timestamp but different price (within deviation)
    let price2 = 101_000_000i128;
    let result = client.update_price_feed(&admin, &asset, &price2, &8, &oracle);
    assert_eq!(result, price2);
}

/// Test price retrieval for non-existent asset
#[test]
#[should_panic(expected = "Oracle error")]
fn test_get_price_nonexistent_asset() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let nonexistent_asset = Address::generate(&env);

    client.get_price(&nonexistent_asset);
}

// =============================================================================
// SECURITY TESTS
// =============================================================================

/// Test that price bounds are enforced after configuration
#[test]
fn test_price_bounds_enforcement() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Configure with specific price bounds
    let config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 3600,
        cache_ttl_seconds: 300,
        min_price: 1_000_000,         // Minimum $0.01 with 8 decimals
        max_price: 1_000_000_000_000, // Maximum $10,000 with 8 decimals
    };
    client.configure_oracle(&admin, &config);

    // Valid price within bounds
    let valid_price = 100_000_000i128;
    let result = client.update_price_feed(&admin, &asset, &valid_price, &8, &oracle);
    assert_eq!(result, valid_price);
}

/// Test price below minimum bound is rejected
#[test]
#[should_panic(expected = "Oracle error")]
fn test_price_below_minimum_bound() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Configure with specific price bounds
    let config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 3600,
        cache_ttl_seconds: 300,
        min_price: 1_000_000, // Minimum $0.01 with 8 decimals
        max_price: 1_000_000_000_000,
    };
    client.configure_oracle(&admin, &config);

    // Price below minimum
    let below_min_price = 100i128; // Way below $0.01
    client.update_price_feed(&admin, &asset, &below_min_price, &8, &oracle);
}

/// Test sequential price updates maintain consistency
#[test]
fn test_sequential_price_updates() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Series of gradual price changes
    let prices: [i128; 5] = [
        100_000_000i128,
        102_000_000i128, // +2%
        104_000_000i128, // +2%
        103_000_000i128, // -1%
        105_000_000i128, // +2%
    ];

    for (i, price) in prices.iter().enumerate() {
        env.ledger()
            .with_mut(|li| li.timestamp = (i as u64 + 1) * 100);
        let result = client.update_price_feed(&admin, &asset, price, &8, &oracle);
        assert_eq!(result, *price);
    }
}
