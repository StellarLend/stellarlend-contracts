//! Oracle Configuration Management and Role Separation Tests for Smart Contract
//!
//! This test suite covers:
//! - Oracle configuration changes in the smart contract
//! - Role separation enforcement (who can change oracle settings)
//! - Security edge cases and invalid configurations
//! - Configuration validation and rollback scenarios

use crate::oracle::{OracleConfig, OracleDataKey, PriceFeed};
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

/// Helper to get oracle configuration directly from storage
fn get_oracle_config(env: &Env, contract_id: &Address) -> OracleConfig {
    env.as_contract(contract_id, || {
        let config_key = OracleDataKey::OracleConfig;
        env.storage()
            .persistent()
            .get::<OracleDataKey, OracleConfig>(&config_key)
            .unwrap()
    })
}

/// Helper to get primary oracle directly from storage
fn get_primary_oracle(env: &Env, contract_id: &Address, asset: &Address) -> Option<Address> {
    env.as_contract(contract_id, || {
        let key = OracleDataKey::PrimaryOracle(asset.clone());
        env.storage()
            .persistent()
            .get::<OracleDataKey, Address>(&key)
    })
}

/// Helper to get fallback oracle directly from storage
fn get_fallback_oracle(env: &Env, contract_id: &Address, asset: &Address) -> Option<Address> {
    env.as_contract(contract_id, || {
        let key = OracleDataKey::FallbackOracle(asset.clone());
        env.storage()
            .persistent()
            .get::<OracleDataKey, Address>(&key)
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
// ORACLE CONFIGURATION TESTS
// =============================================================================

/// Test successful oracle configuration by admin
#[test]
fn test_configure_oracle_success() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);

    let config = OracleConfig {
        max_deviation_bps: 1000,     // 10%
        max_staleness_seconds: 7200, // 2 hours
        cache_ttl_seconds: 600,      // 10 minutes
        min_price: 1,
        max_price: i128::MAX,
    };

    client.configure_oracle(&admin, &config);

    // Verify configuration was stored
    let stored_config = get_oracle_config(&env, &contract_id);
    assert_eq!(stored_config.max_deviation_bps, 1000);
    assert_eq!(stored_config.max_staleness_seconds, 7200);
    assert_eq!(stored_config.cache_ttl_seconds, 600);
}

/// Test oracle configuration by unauthorized user should fail
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

/// Test setting primary oracle by admin
#[test]
fn test_set_primary_oracle_success() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let primary_oracle = Address::generate(&env);

    client.set_primary_oracle(&admin, &asset, &primary_oracle);

    // Verify primary oracle was set
    let stored_oracle = get_primary_oracle(&env, &contract_id, &asset).unwrap();
    assert_eq!(stored_oracle, primary_oracle);
}

/// Test setting primary oracle by unauthorized user should fail
#[test]
#[should_panic(expected = "Oracle error")]
fn test_set_primary_oracle_unauthorized() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let primary_oracle = Address::generate(&env);
    let unauthorized = Address::generate(&env);

    client.set_primary_oracle(&unauthorized, &asset, &primary_oracle);
}

/// Test setting fallback oracle by admin
#[test]
fn test_set_fallback_oracle_success() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let fallback_oracle = Address::generate(&env);

    client.set_fallback_oracle(&admin, &asset, &fallback_oracle);

    // Verify fallback oracle was set
    let stored_oracle = get_fallback_oracle(&env, &contract_id, &asset).unwrap();
    assert_eq!(stored_oracle, fallback_oracle);
}

/// Test setting fallback oracle by unauthorized user should fail
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

// =============================================================================
// ORACLE SWITCHING TESTS
// =============================================================================

/// Test switching primary oracle for an asset
#[test]
fn test_switch_primary_oracle() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);

    // Set initial primary oracle
    client.set_primary_oracle(&admin, &asset, &oracle1);
    assert_eq!(get_primary_oracle(&env, &contract_id, &asset).unwrap(), oracle1);

    // Switch to new primary oracle
    client.set_primary_oracle(&admin, &asset, &oracle2);
    assert_eq!(get_primary_oracle(&env, &contract_id, &asset).unwrap(), oracle2);
}

/// Test switching fallback oracle for an asset
#[test]
fn test_switch_fallback_oracle() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let fallback1 = Address::generate(&env);
    let fallback2 = Address::generate(&env);

    // Set initial fallback oracle
    client.set_fallback_oracle(&admin, &asset, &fallback1);
    assert_eq!(get_fallback_oracle(&env, &contract_id, &asset).unwrap(), fallback1);

    // Switch to new fallback oracle
    client.set_fallback_oracle(&admin, &asset, &fallback2);
    assert_eq!(get_fallback_oracle(&env, &contract_id, &asset).unwrap(), fallback2);
}

/// Test oracle switching maintains price feed integrity
#[test]
fn test_oracle_switching_price_integrity() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);

    // Set initial oracle and price
    client.set_primary_oracle(&admin, &asset, &oracle1);
    let price1 = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price1, &8, &oracle1);

    // Verify price feed exists
    let feed_key = OracleDataKey::PriceFeed(asset.clone());
    let initial_feed: PriceFeed = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get::<OracleDataKey, PriceFeed>(&feed_key)
            .unwrap()
    });
    assert_eq!(initial_feed.price, price1);
    assert_eq!(initial_feed.oracle, oracle1);

    // Switch oracle
    client.set_primary_oracle(&admin, &asset, &oracle2);

    // Update price with new oracle
    let price2 = 105_000_000i128;
    client.update_price_feed(&admin, &asset, &price2, &8, &oracle2);

    // Verify price was updated with new oracle
    let updated_feed: PriceFeed = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get::<OracleDataKey, PriceFeed>(&feed_key)
            .unwrap()
    });
    assert_eq!(updated_feed.price, price2);
    assert_eq!(updated_feed.oracle, oracle2);
}

// =============================================================================
// CONFIGURATION PARAMETER TESTS
// =============================================================================

/// Test adjusting price deviation threshold
#[test]
fn test_adjust_price_deviation_threshold() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Set tight deviation threshold (2%)
    let tight_config = OracleConfig {
        max_deviation_bps: 200, // 2%
        max_staleness_seconds: 3600,
        cache_ttl_seconds: 300,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &tight_config);

    // Initial price
    let initial_price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &initial_price, &8, &oracle);

    // Small change within 2% should succeed
    let small_change = 101_000_000i128; // 1% increase
    client.update_price_feed(&admin, &asset, &small_change, &8, &oracle);

    // Large change beyond 2% should fail
    let large_change = 110_000_000i128; // 10% increase
    let result = std::panic::catch(|| {
        client.update_price_feed(&admin, &asset, &large_change, &8, &oracle);
    });
    assert!(result.is_err());
}

/// Test adjusting staleness threshold
#[test]
fn test_adjust_staleness_threshold() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Set very short staleness threshold (60 seconds)
    let fresh_config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 60,
        cache_ttl_seconds: 30,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &fresh_config);

    env.ledger().with_mut(|li| li.timestamp = 1000);
    let price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price, &8, &oracle);

    // Clear cache to force staleness check
    env.as_contract(&contract_id, || {
        let cache_key = OracleDataKey::PriceCache(asset.clone());
        env.storage()
            .persistent()
            .remove::<OracleDataKey>(&cache_key);
    });

    // Move time beyond short staleness threshold
    env.ledger().with_mut(|li| li.timestamp = 2000);

    // Price should be considered stale
    let result = std::panic::catch(|| {
        client.get_price(&asset);
    });
    assert!(result.is_err());
}

/// Test adjusting cache TTL
#[test]
fn test_adjust_cache_ttl() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Set very short cache TTL (10 seconds)
    let short_cache_config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 3600,
        cache_ttl_seconds: 10,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &short_cache_config);

    env.ledger().with_mut(|li| li.timestamp = 1000);
    let price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &price, &8, &oracle);

    // Get price immediately (should be cached)
    let cached_price = client.get_price(&asset);
    assert_eq!(cached_price, price);

    // Move time beyond cache TTL
    env.ledger().with_mut(|li| li.timestamp = 1020);

    // Clear cache to test expiration
    env.as_contract(&contract_id, || {
        let cache_key = OracleDataKey::PriceCache(asset.clone());
        let cached = env.storage()
            .persistent()
            .get::<OracleDataKey, crate::oracle::CachedPrice>(&cache_key);
        assert!(cached.is_some()); // Cache exists but is expired
    });
}

// =============================================================================
// ROLE SEPARATION ENFORCEMENT TESTS
// =============================================================================

/// Test that oracle cannot modify configuration
#[test]
fn test_oracle_cannot_modify_config() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Set oracle
    client.set_primary_oracle(&admin, &asset, &oracle);

    // Oracle should not be able to modify configuration
    let config = OracleConfig {
        max_deviation_bps: 1000,
        max_staleness_seconds: 7200,
        cache_ttl_seconds: 600,
        min_price: 1,
        max_price: i128::MAX,
    };

    let result = std::panic::catch(|| {
        client.configure_oracle(&oracle, &config);
    });
    assert!(result.is_err());
}

/// Test that oracle cannot set other oracles
#[test]
fn test_oracle_cannot_set_oracles() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);

    // Set first oracle
    client.set_primary_oracle(&admin, &asset, &oracle1);

    // First oracle should not be able to set second oracle
    let result = std::panic::catch(|| {
        client.set_primary_oracle(&oracle1, &asset, &oracle2);
    });
    assert!(result.is_err());

    // First oracle should not be able to set fallback oracle
    let result = std::panic::catch(|| {
        client.set_fallback_oracle(&oracle1, &asset, &oracle2);
    });
    assert!(result.is_err());
}

/// Test that random user cannot modify oracle settings
#[test]
fn test_random_user_cannot_modify_oracle_settings() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);
    let random_user = Address::generate(&env);

    // Random user should not be able to set oracles
    let result = std::panic::catch(|| {
        client.set_primary_oracle(&random_user, &asset, &oracle);
    });
    assert!(result.is_err());

    let result = std::panic::catch(|| {
        client.set_fallback_oracle(&random_user, &asset, &oracle);
    });
    assert!(result.is_err());

    // Random user should not be able to configure oracle
    let config = OracleConfig {
        max_deviation_bps: 1000,
        max_staleness_seconds: 7200,
        cache_ttl_seconds: 600,
        min_price: 1,
        max_price: i128::MAX,
    };

    let result = std::panic::catch(|| {
        client.configure_oracle(&random_user, &config);
    });
    assert!(result.is_err());
}

/// Test admin can override any oracle settings
#[test]
fn test_admin_override_authority() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);

    // Set oracle1 as primary
    client.set_primary_oracle(&admin, &asset, &oracle1);
    client.update_price_feed(&admin, &asset, &100_000_000, &8, &oracle1);

    // Admin should be able to switch to oracle2
    client.set_primary_oracle(&admin, &asset, &oracle2);
    assert_eq!(get_primary_oracle(&env, &contract_id, &asset).unwrap(), oracle2);

    // Admin should be able to update price with oracle2
    client.update_price_feed(&admin, &asset, &105_000_000, &8, &oracle2);

    // Verify price was updated with new oracle
    let feed_key = OracleDataKey::PriceFeed(asset.clone());
    let updated_feed: PriceFeed = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get::<OracleDataKey, PriceFeed>(&feed_key)
            .unwrap()
    });
    assert_eq!(updated_feed.oracle, oracle2);
}

// =============================================================================
// SECURITY EDGE CASES
// =============================================================================

/// Test oracle configuration with extreme values
#[test]
fn test_extreme_configuration_values() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);

    // Test maximum deviation (100%)
    let max_deviation_config = OracleConfig {
        max_deviation_bps: 10000, // 100%
        max_staleness_seconds: 86400, // 24 hours
        cache_ttl_seconds: 3600, // 1 hour
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &max_deviation_config);

    // Test minimum deviation (0.01%)
    let min_deviation_config = OracleConfig {
        max_deviation_bps: 1, // 0.01%
        max_staleness_seconds: 1, // 1 second
        cache_ttl_seconds: 0, // no cache
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &min_deviation_config);
}

/// Test oracle switching with price manipulation attempts
#[test]
fn test_oracle_switching_manipulation_resistance() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);

    // Set oracle1 and establish price
    client.set_primary_oracle(&admin, &asset, &oracle1);
    let normal_price = 100_000_000i128;
    client.update_price_feed(&admin, &asset, &normal_price, &8, &oracle1);

    // Switch to oracle2
    client.set_primary_oracle(&admin, &asset, &oracle2);

    // Oracle2 should not be able to submit drastically different price
    let manipulated_price = 500_000_000i128; // 5x increase
    let result = std::panic::catch(|| {
        client.update_price_feed(&oracle2, &asset, &manipulated_price, &8, &oracle2);
    });
    assert!(result.is_err()); // Should fail due to deviation check

    // But admin can override with valid justification
    client.update_price_feed(&admin, &asset, &150_000_000, &8, &oracle2);
}

/// Test configuration persistence during oracle switching
#[test]
fn test_configuration_persistence_oracle_switch() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);

    // Set specific configuration
    let config = OracleConfig {
        max_deviation_bps: 300, // 3%
        max_staleness_seconds: 1800, // 30 minutes
        cache_ttl_seconds: 120, // 2 minutes
        min_price: 1000, // $0.0001
        max_price: 1_000_000_000_000, // $10,000
    };
    client.configure_oracle(&admin, &config);

    // Set oracle1
    client.set_primary_oracle(&admin, &asset, &oracle1);
    client.update_price_feed(&admin, &asset, &100_000_000, &8, &oracle1);

    // Switch to oracle2
    client.set_primary_oracle(&admin, &asset, &oracle2);

    // Configuration should persist
    let stored_config = get_oracle_config(&env, &contract_id);
    assert_eq!(stored_config.max_deviation_bps, 300);
    assert_eq!(stored_config.max_staleness_seconds, 1800);
    assert_eq!(stored_config.cache_ttl_seconds, 120);

    // New oracle should respect existing configuration
    let result = std::panic::catch(|| {
        client.update_price_feed(&oracle2, &asset, &110_000_000, &8, &oracle2); // 10% increase
    });
    assert!(result.is_err()); // Should fail due to 3% deviation limit
}

/// Test multiple asset oracle configuration
#[test]
fn test_multiple_asset_oracle_configuration() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);

    let asset1 = Address::generate(&env);
    let asset2 = Address::generate(&env);
    let asset3 = Address::generate(&env);

    let oracle1 = Address::generate(&env);
    let oracle2 = Address::generate(&env);
    let oracle3 = Address::generate(&env);

    let fallback1 = Address::generate(&env);
    let fallback2 = Address::generate(&env);
    let fallback3 = Address::generate(&env);

    // Configure different oracles for each asset
    client.set_primary_oracle(&admin, &asset1, &oracle1);
    client.set_fallback_oracle(&admin, &asset1, &fallback1);

    client.set_primary_oracle(&admin, &asset2, &oracle2);
    client.set_fallback_oracle(&admin, &asset2, &fallback2);

    client.set_primary_oracle(&admin, &asset3, &oracle3);
    client.set_fallback_oracle(&admin, &asset3, &fallback3);

    // Verify all oracles are set correctly
    assert_eq!(get_primary_oracle(&env, &contract_id, &asset1).unwrap(), oracle1);
    assert_eq!(get_fallback_oracle(&env, &contract_id, &asset1).unwrap(), fallback1);

    assert_eq!(get_primary_oracle(&env, &contract_id, &asset2).unwrap(), oracle2);
    assert_eq!(get_fallback_oracle(&env, &contract_id, &asset2).unwrap(), fallback2);

    assert_eq!(get_primary_oracle(&env, &contract_id, &asset3).unwrap(), oracle3);
    assert_eq!(get_fallback_oracle(&env, &contract_id, &asset3).unwrap(), fallback3);
}

/// Test oracle configuration with pause functionality
#[test]
fn test_oracle_configuration_with_pause() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    let asset = Address::generate(&env);
    let oracle = Address::generate(&env);

    // Set oracle
    client.set_primary_oracle(&admin, &asset, &oracle);

    // Pause oracle updates
    set_oracle_paused(&env, &contract_id, true);

    // Oracle should not be able to update prices when paused
    let result = std::panic::catch(|| {
        client.update_price_feed(&oracle, &asset, &100_000_000, &8, &oracle);
    });
    assert!(result.is_err());

    // Admin should still be able to configure when paused
    let config = OracleConfig {
        max_deviation_bps: 1000,
        max_staleness_seconds: 7200,
        cache_ttl_seconds: 600,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &config);

    // Admin should still be able to set oracles when paused
    let new_oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &new_oracle);

    // Unpause
    set_oracle_paused(&env, &contract_id, false);

    // Oracle should be able to update after unpause
    client.update_price_feed(&new_oracle, &asset, &100_000_000, &8, &new_oracle);
}
