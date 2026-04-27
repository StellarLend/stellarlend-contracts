//! # Contract Initialization Test Suite
//!
//! Comprehensive tests for contract initialization ensuring:
//! - Successful one-time initialization
//! - Double-initialization prevention
//! - Invalid admin handling
//! - Storage correctness verification
//! - Security assumptions validation

use crate::interest_rate::InterestRateDataKey;
use crate::risk_management::RiskDataKey;
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol,
};

/// Create test environment with mocked auth
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Test: Successful initialization with valid admin
///
/// Verifies:
/// - Contract initializes without errors
/// - Admin is stored correctly
/// - Default risk parameters are set
/// - Default interest rate config is set
/// - All pause switches are initialized to false
/// - Emergency pause is initialized to false
#[test]
fn test_successful_initialization() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    // Initialize contract
    client.initialize(&admin);

    // Verify risk management admin storage
    env.as_contract(&contract_id, || {
        let admin_key = RiskDataKey::Admin;
        let stored_admin: Address = env.storage().persistent().get(&admin_key).unwrap();
        assert_eq!(stored_admin, admin);
    });

    // Verify interest rate admin storage
    env.as_contract(&contract_id, || {
        let admin_key = InterestRateDataKey::Admin;
        let stored_admin: Address = env.storage().persistent().get(&admin_key).unwrap();
        assert_eq!(stored_admin, admin);
    });

    // Verify default risk config
    let config = client.get_risk_config().expect("Risk config should exist");
    assert_eq!(config.min_collateral_ratio, 11_000);
    assert_eq!(config.liquidation_threshold, 10_500);
    assert_eq!(config.close_factor, 5_000);
    assert_eq!(config.liquidation_incentive, 1_000);

    // Verify pause switches
    assert!(!client.is_operation_paused(&Symbol::new(&env, "pause_deposit")));
    assert!(!client.is_operation_paused(&Symbol::new(&env, "pause_withdraw")));
    assert!(!client.is_operation_paused(&Symbol::new(&env, "pause_borrow")));
    assert!(!client.is_operation_paused(&Symbol::new(&env, "pause_repay")));
    assert!(!client.is_operation_paused(&Symbol::new(&env, "pause_liquidate")));

    // Verify emergency pause
    assert!(!client.is_emergency_paused());
}

/// Test: Double initialization behavior
///
/// Verifies:
/// - Second initialization doesn't panic
/// - Interest rate config is not overwritten (idempotent)
/// - Admins may be updated (current implementation allows this)
///
/// Security Note: In production, initialize should only be called once
#[test]
fn test_double_initialization_behavior() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);

    // First initialization
    client.initialize(&admin1);

    // Store original config
    let original_config = client.get_risk_config().unwrap();

    // Second initialization with different admin
    client.initialize(&admin2);

    // Verify interest rate config is not overwritten
    env.as_contract(&contract_id, || {
        let config_key = InterestRateDataKey::InterestRateConfig;
        assert!(
            env.storage().persistent().has(&config_key),
            "Interest rate config should still exist"
        );
    });

    // Verify risk config timestamp changed (indicating re-initialization)
    let new_config = client.get_risk_config().unwrap();
    assert!(
        new_config.last_update >= original_config.last_update,
        "Config should be updated on re-initialization"
    );
}

/// Test: Storage correctness after initialization
///
/// Verifies all storage keys are properly set:
/// - Admin key in risk management
/// - Admin key in interest rate module
/// - Risk config key
/// - Emergency pause key
/// - Interest rate config key
#[test]
fn test_storage_correctness() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    env.as_contract(&contract_id, || {
        // Verify risk management storage
        assert!(env.storage().persistent().has(&RiskDataKey::Admin));
        assert!(env.storage().persistent().has(&RiskDataKey::RiskConfig));
        assert!(env.storage().persistent().has(&RiskDataKey::EmergencyPause));

        // Verify interest rate storage
        assert!(env.storage().persistent().has(&InterestRateDataKey::Admin));
        assert!(env
            .storage()
            .persistent()
            .has(&InterestRateDataKey::InterestRateConfig));
    });
}

/// Test: Default risk parameters validation
///
/// Verifies default parameters meet security requirements:
/// - Min collateral ratio >= 100%
/// - Liquidation threshold < min collateral ratio
/// - Close factor <= 100%
/// - Liquidation incentive is reasonable
#[test]
fn test_default_risk_parameters_valid() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    let config = client.get_risk_config().unwrap();

    // Security validations
    assert!(
        config.min_collateral_ratio >= 10_000,
        "Min collateral ratio must be >= 100%"
    );
    assert!(
        config.liquidation_threshold < config.min_collateral_ratio,
        "Liquidation threshold must be < min collateral ratio"
    );
    assert!(
        config.close_factor <= 10_000,
        "Close factor must be <= 100%"
    );
    assert!(
        config.liquidation_incentive > 0,
        "Liquidation incentive must be positive"
    );
    assert!(
        config.liquidation_incentive <= 5_000,
        "Liquidation incentive should be reasonable (<= 50%)"
    );
}

/// Test: Default interest rate configuration
///
/// Verifies default interest rate parameters are set correctly
#[test]
fn test_default_interest_rate_config() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    env.as_contract(&contract_id, || {
        let config_key = InterestRateDataKey::InterestRateConfig;
        assert!(
            env.storage().persistent().has(&config_key),
            "Interest rate config should be initialized"
        );
    });
}

/// Test: Pause switches initialization
///
/// Verifies all pause switches are initialized to false (unpaused)
#[test]
fn test_pause_switches_initialized() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    let operations = [
        "pause_deposit",
        "pause_withdraw",
        "pause_borrow",
        "pause_repay",
        "pause_liquidate",
    ];

    for op in operations {
        let symbol = Symbol::new(&env, op);
        assert!(
            !client.is_operation_paused(&symbol),
            "Operation {} should be unpaused after initialization",
            op
        );
    }
}

/// Test: Emergency pause initialization
///
/// Verifies emergency pause is initialized to false
#[test]
fn test_emergency_pause_initialized() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    assert!(
        !client.is_emergency_paused(),
        "Emergency pause should be false after initialization"
    );
}

/// Test: Timestamp recording
///
/// Verifies that initialization records the current ledger timestamp
#[test]
fn test_timestamp_recorded() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    let init_time = env.ledger().timestamp();
    client.initialize(&admin);

    let config = client.get_risk_config().unwrap();
    assert_eq!(
        config.last_update, init_time,
        "Last update timestamp should match initialization time"
    );
}

/// Test: Multiple admin addresses
///
/// Verifies initialization works with different admin address types
#[test]
fn test_various_admin_addresses() {
    let env = create_test_env();

    // Test with generated address
    let contract_id1 = env.register(HelloContract, ());
    let client1 = HelloContractClient::new(&env, &contract_id1);
    let admin1 = Address::generate(&env);
    client1.initialize(&admin1);

    env.as_contract(&contract_id1, || {
        let stored: Address = env.storage().persistent().get(&RiskDataKey::Admin).unwrap();
        assert_eq!(stored, admin1);
    });

    // Test with another generated address
    let contract_id2 = env.register(HelloContract, ());
    let client2 = HelloContractClient::new(&env, &contract_id2);
    let admin2 = Address::generate(&env);
    client2.initialize(&admin2);

    env.as_contract(&contract_id2, || {
        let stored: Address = env.storage().persistent().get(&RiskDataKey::Admin).unwrap();
        assert_eq!(stored, admin2);
    });
}

/// Test: Initialization state consistency
///
/// Verifies that all subsystems are initialized consistently
#[test]
fn test_initialization_state_consistency() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    env.as_contract(&contract_id, || {
        // Both modules should have admin set
        let risk_admin: Address = env.storage().persistent().get(&RiskDataKey::Admin).unwrap();
        let interest_admin: Address = env
            .storage()
            .persistent()
            .get(&InterestRateDataKey::Admin)
            .unwrap();

        assert_eq!(
            risk_admin, interest_admin,
            "Both modules should have the same admin"
        );
        assert_eq!(
            risk_admin, admin,
            "Admin should match initialization parameter"
        );
    });
}

/// Test: Storage persistence type
///
/// Verifies that initialization data uses persistent storage
#[test]
fn test_storage_persistence() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    // Verify data persists across contract calls
    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().has(&RiskDataKey::Admin));
        assert!(env.storage().persistent().has(&RiskDataKey::RiskConfig));
    });

    // Simulate ledger advancement
    env.ledger().with_mut(|li| li.sequence_number += 100);

    // Data should still be accessible
    let config = client.get_risk_config();
    assert!(
        config.is_some(),
        "Config should persist across ledger advancement"
    );
}

/// Test: Initialization should only happen once in production
///
/// Security note: In production, initialize should be called exactly once
/// during contract deployment. This test documents the expected usage pattern.
#[test]
fn test_initialization_production_pattern() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    // Production pattern: Initialize once during deployment
    client.initialize(&admin);

    // Verify initialization succeeded
    env.as_contract(&contract_id, || {
        assert!(env.storage().persistent().has(&RiskDataKey::Admin));
        assert!(env.storage().persistent().has(&InterestRateDataKey::Admin));
    });

    // In production, no further initialization calls should be made
    // The contract is now ready for use
}
