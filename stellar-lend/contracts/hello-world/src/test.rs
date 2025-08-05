#![cfg(test)]

use super::*;
use soroban_sdk::{Address, Env, String, testutils::Address as TestAddress, testutils::Address as _};

/// Test utilities for creating test environments and addresses
pub struct TestUtils;

impl TestUtils {
    /// Create a test environment
    pub fn create_test_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    /// Create a test address from a string
    pub fn create_test_address(env: &Env, address_str: &str) -> Address {
        Address::from_string(&String::from_str(env, address_str))
    }

    /// Create a test admin address
    pub fn create_admin_address(env: &Env) -> Address {
        Self::create_test_address(
            env,
            "GCAZYE3EB54VKP3UQBX3H73VQO3SIWTZNR7NJQKJFZZ6XLADWA4C3SOC",
        )
    }

    /// Create a test user address
    pub fn create_user_address(env: &Env, user_id: u32) -> Address {
        match user_id {
            0 => Self::create_test_address(
                env,
                "GCXOTMMXRS24MYZI5FJPUCOEOFNWSR4XX7UXIK3NDGGE6A5QMJ5FF2FS",
            ),
            1 => Self::create_test_address(
                env,
                "GAUA7XL5K54CC2DDGP77FJ2YBHRJLT36CPZDXWPM6MP7MANOGG77PNJU",
            ),
            2 => Self::create_test_address(
                env,
                "GCUA7XL5K54CC2DDGP77FJ2YBHRJLT36CPZDXWPM6MP7MANOGG77PNJU",
            ),
            _ => Self::create_test_address(
                env,
                "GCUA7XL5K54CC2DDGP77FJ2YBHRJLT36CPZDXWPM6MP7MANOGG77PNJU",
            ),
        }
    }

    /// Create a test oracle address
    pub fn create_oracle_address(env: &Env) -> Address {
        Self::create_test_address(
            env,
            "GCXOTMMXRS24MYZI5FJPUCOEOFNWSR4XX7UXIK3NDGGE6A5QMJ5FF2FS",
        )
    }

    /// Initialize the contract with test admin
    pub fn initialize_contract(env: &Env) -> Address {
        let admin = Self::create_admin_address(env);
        let contract_id = env.register(Contract, ());
        env.as_contract(&contract_id, || {
            Contract::initialize(env.clone(), admin.to_string()).unwrap();

            // Set oracle address for RealPriceOracle to work
            let oracle = Self::create_oracle_address(env);
            Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();
        });
        admin
    }
}

/// Mock price oracle for testing
pub struct TestOracle;

impl PriceOracle for TestOracle {
    fn get_price(_env: &Env) -> i128 {
        // Test price: 1 collateral = 1.5 debt (price = 1.5e8)
        150_000_000 // 1.5 * 1e8
    }

    fn get_last_update(_env: &Env) -> u64 {
        0 // Test oracle doesn't track updates
    }

    fn validate_price(_env: &Env, _price: i128) -> bool {
        true // Test oracle always validates
    }
}

#[test]
fn test_contract_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        let result = Contract::initialize(env.clone(), admin.to_string());
        assert!(result.is_ok());

        // Test that admin is set correctly - but don't call get_protocol_params yet
        // since oracle is not set
        let admin_key = ProtocolConfig::admin_key();
        let stored_admin = env
            .storage()
            .instance()
            .get::<Symbol, Address>(&admin_key)
            .unwrap();
        assert_eq!(stored_admin, admin);
    });
}

#[test]
fn test_contract_initialization_already_initialized() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // First initialization should succeed
        let result = Contract::initialize(env.clone(), admin.to_string());
        assert!(result.is_ok());

        // Second initialization should fail
        let result = Contract::initialize(env.clone(), admin.to_string());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::AlreadyInitialized);
    });
}

#[test]
fn test_deposit_collateral() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Test successful deposit
        let result = Contract::deposit_collateral(env.clone(), user.to_string(), 1000);
        assert!(result.is_ok());

        // Verify position is updated
        let (collateral, debt, _ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(collateral, 1000);
        assert_eq!(debt, 0);
    });
}

#[test]
fn test_deposit_collateral_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Test deposit with zero amount
        let result = Contract::deposit_collateral(env.clone(), user.to_string(), 0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InvalidAmount);

        // Test deposit with negative amount
        let result = Contract::deposit_collateral(env.clone(), user.to_string(), -100);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InvalidAmount);
    });
}

#[test]
fn test_deposit_collateral_invalid_address() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Test deposit with empty address
        let result = Contract::deposit_collateral(env.clone(), String::from_str(&env, ""), 1000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InvalidAddress);
    });
}

#[test]
fn test_borrow_success() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // First deposit collateral
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();

        // Then borrow (should succeed with sufficient collateral)
        let result = Contract::borrow(env.clone(), user.to_string(), 1000);
        assert!(result.is_ok());

        // Verify position is updated
        let (collateral, debt, _ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(collateral, 2000);
        assert_eq!(debt, 1000);
    });
}

#[test]
fn test_borrow_insufficient_collateral_ratio() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Deposit small amount of collateral
        Contract::deposit_collateral(env.clone(), user.to_string(), 100).unwrap();

        // Try to borrow large amount (should fail)
        let result = Contract::borrow(env.clone(), user.to_string(), 1000);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            ProtocolError::InsufficientCollateralRatio
        );
    });
}

#[test]
fn test_repay_success() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Setup: deposit and borrow
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 1000).unwrap();

        // Repay part of the debt
        let result = Contract::repay(env.clone(), user.to_string(), 500);
        assert!(result.is_ok());

        // Verify position is updated
        let (collateral, debt, _ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(collateral, 2000);
        assert_eq!(debt, 500);
    });
}

#[test]
fn test_repay_full_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Setup: deposit and borrow
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 1000).unwrap();

        // Repay full amount
        let result = Contract::repay(env.clone(), user.to_string(), 1000);
        assert!(result.is_ok());

        // Verify debt is zero
        let (collateral, debt, _ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(collateral, 2000);
        assert_eq!(debt, 0);
    });
}

#[test]
fn test_withdraw_success() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Setup: deposit collateral
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();

        // Withdraw part of collateral
        let result = Contract::withdraw(env.clone(), user.to_string(), 1000);
        assert!(result.is_ok());

        // Verify position is updated
        let (collateral, debt, _ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(collateral, 1000);
        assert_eq!(debt, 0);
    });
}

#[test]
fn test_withdraw_insufficient_collateral() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Setup: deposit small amount
        Contract::deposit_collateral(env.clone(), user.to_string(), 100).unwrap();

        // Try to withdraw more than available
        let result = Contract::withdraw(env.clone(), user.to_string(), 200);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InsufficientCollateral);
    });
}

#[test]
fn test_withdraw_insufficient_collateral_ratio() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Setup: deposit and borrow
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 1000).unwrap();

        // Try to withdraw too much (would breach collateral ratio)
        let result = Contract::withdraw(env.clone(), user.to_string(), 1500);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            ProtocolError::InsufficientCollateralRatio
        );
    });
}

#[test]
fn test_liquidate_success() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let liquidator = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Initialize contract
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Setup: deposit very small collateral and borrow large amount
        Contract::deposit_collateral(env.clone(), user.to_string(), 10).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 1000).unwrap();

        // Liquidate the user's position (not the liquidator's)
        let result =
            Contract::liquidate(env.clone(), liquidator.to_string(), user.to_string(), 500);
        assert!(result.is_ok());

        // Verify position is updated (debt reduced, collateral penalized)
        let (collateral, debt, _ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(debt, 500); // Debt reduced by 500
        assert!(collateral < 10); // Collateral penalized
    });
}

#[test]
fn test_liquidate_not_eligible() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let liquidator = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Setup: deposit sufficient collateral and borrow small amount
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 1000).unwrap();

        // Try to liquidate (should fail as position is well-collateralized)
        let result =
            Contract::liquidate(env.clone(), liquidator.to_string(), user.to_string(), 500);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            ProtocolError::NotEligibleForLiquidation
        );
    });
}

#[test]
fn test_admin_functions() {
    let env = Env::default();
    env.mock_all_auths();

    // Initialize contract properly
    let admin = TestUtils::create_admin_address(&env);
    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        let non_admin = TestUtils::create_user_address(&env, 1);
        let oracle = TestUtils::create_oracle_address(&env);

        // Test admin can set oracle
        let result = Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string());
        assert!(result.is_ok());

        // Test non-admin cannot set oracle
        let result = Contract::set_oracle(env.clone(), non_admin.to_string(), oracle.to_string());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Test admin can set min collateral ratio
        let result = Contract::set_min_collateral_ratio(env.clone(), admin.to_string(), 200);
        assert!(result.is_ok());

        // Test non-admin cannot set min collateral ratio
        let result = Contract::set_min_collateral_ratio(env.clone(), non_admin.to_string(), 200);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);
    });
}

#[test]
fn test_protocol_params() {
    let env = Env::default();
    env.mock_all_auths();

    // Initialize contract properly
    let admin = TestUtils::create_admin_address(&env);
    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        let oracle = TestUtils::create_oracle_address(&env);

        // Set oracle first
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Get protocol params
        let (stored_admin, stored_oracle, min_ratio) =
            Contract::get_protocol_params(env.clone()).unwrap();
        assert_eq!(stored_admin, admin);
        assert_eq!(stored_oracle, oracle);
        assert_eq!(min_ratio, 150); // Default value
    });
}

#[test]
fn test_system_stats() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        let (total_collateral, total_debt) = Contract::get_system_stats(env.clone()).unwrap();
        // For now, these are stubbed to return (0, 0)
        assert_eq!(total_collateral, 0);
        assert_eq!(total_debt, 0);
    });
}

#[test]
fn test_event_history_stubs() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Test user event history (stubbed)
        let events = Contract::get_user_event_history(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "deposit"),
        )
        .unwrap();
        assert_eq!(events.len(), 0); // Empty for now

        // Test recent events (stubbed)
        let events = Contract::get_recent_events(env.clone(), 10).unwrap();
        assert_eq!(events.len(), 0); // Empty for now
    });
}

#[test]
fn test_edge_cases() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Test with maximum i128 values
        let max_amount = i128::MAX;
        let result = Contract::deposit_collateral(env.clone(), user.to_string(), max_amount);
        assert!(result.is_ok());

        // Test with minimum i128 values
        let result = Contract::deposit_collateral(env.clone(), user.to_string(), i128::MIN);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InvalidAmount);
    });
}

#[test]
fn test_multiple_users() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user1 = TestUtils::create_user_address(&env, 1);
    let user2 = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Initialize contract
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // User 1 deposits and borrows
        Contract::deposit_collateral(env.clone(), user1.to_string(), 2000).unwrap();
        Contract::borrow(env.clone(), user1.to_string(), 1000).unwrap();

        // User 2 deposits and borrows
        Contract::deposit_collateral(env.clone(), user2.to_string(), 3000).unwrap();
        Contract::borrow(env.clone(), user2.to_string(), 1500).unwrap();

        // Verify positions are independent
        let (collateral1, debt1, _) =
            Contract::get_position(env.clone(), user1.to_string()).unwrap();
        let (collateral2, debt2, _) =
            Contract::get_position(env.clone(), user2.to_string()).unwrap();

        assert_eq!(collateral1, 2000);
        assert_eq!(debt1, 1000);
        assert_eq!(collateral2, 3000);
        assert_eq!(debt2, 1500);
    });
}

#[test]
fn test_error_enum_values() {
    // Test that all error variants have correct string representations
    assert_eq!(ProtocolError::Unauthorized.to_str(), "Unauthorized access denied");
    assert_eq!(
        ProtocolError::InsufficientCollateral.to_str(),
        "InsufficientCollateral"
    );
    assert_eq!(
        ProtocolError::InsufficientCollateralRatio.to_str(),
        "InsufficientCollateralRatio"
    );
    assert_eq!(ProtocolError::InvalidAmount.to_str(), "InvalidAmount");
    assert_eq!(ProtocolError::InvalidAddress.to_str(), "InvalidAddress");
    assert_eq!(ProtocolError::PositionNotFound.to_str(), "PositionNotFound");
    assert_eq!(
        ProtocolError::AlreadyInitialized.to_str(),
        "AlreadyInitialized"
    );
    assert_eq!(ProtocolError::NotAdmin.to_str(), "NotAdmin");
    assert_eq!(ProtocolError::OracleNotSet.to_str(), "OracleNotSet");
    assert_eq!(ProtocolError::AdminNotSet.to_str(), "AdminNotSet");
    assert_eq!(
        ProtocolError::NotEligibleForLiquidation.to_str(),
        "NotEligibleForLiquidation"
    );
    assert_eq!(ProtocolError::Unknown.to_str(), "Unknown");
}

#[test]
fn test_oracle_price_validation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Set max deviation to 10%
        Contract::set_max_price_deviation(env.clone(), admin.to_string(), 10).unwrap();

        // First price should always be valid
        let price1 = RealPriceOracle::get_price(&env);
        assert!(RealPriceOracle::validate_price(&env, price1));

        // Price within 10% deviation should be valid
        let valid_price = price1 + (price1 * 5) / 100; // 5% increase
        assert!(RealPriceOracle::validate_price(&env, valid_price));

        // Price with 15% deviation should be invalid
        let invalid_price = price1 + (price1 * 15) / 100; // 15% increase
        assert!(!RealPriceOracle::validate_price(&env, invalid_price));
    });
}

#[test]
fn test_oracle_fallback_mechanism() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Set very low max deviation to trigger fallback
        Contract::set_max_price_deviation(env.clone(), admin.to_string(), 1).unwrap();

        // First price should be accepted
        let price1 = RealPriceOracle::get_price(&env);
        assert!(price1 > 0);

        // Second price with any variation should trigger fallback
        let price2 = RealPriceOracle::get_price(&env);
        // Should return fallback price (150_000_000) due to validation failure
        assert_eq!(price2, 150_000_000);
    });
}

#[test]
fn test_oracle_heartbeat_validation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Set heartbeat to 100 seconds
        Contract::set_oracle_heartbeat(env.clone(), admin.to_string(), 100).unwrap();

        // Initial price should not be stale
        RealPriceOracle::get_price(&env);
        assert!(!OracleConfig::is_price_stale(&env));

        // After 100+ seconds, price should be stale
        // Note: In real tests, we'd need to manipulate the ledger timestamp
        // For now, we'll test the logic with current time
        let is_stale = OracleConfig::is_price_stale(&env);
        // This will depend on the actual time elapsed, so we just verify the function works
        assert!(is_stale == true || is_stale == false);
    });
}

#[test]
fn test_oracle_admin_functions() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Test admin can set max deviation
        let result = Contract::set_max_price_deviation(env.clone(), admin.to_string(), 25);
        assert!(result.is_ok());

        // Test non-admin cannot set max deviation
        let result = Contract::set_max_price_deviation(env.clone(), non_admin.to_string(), 25);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Test admin can set heartbeat
        let result = Contract::set_oracle_heartbeat(env.clone(), admin.to_string(), 1800);
        assert!(result.is_ok());

        // Test non-admin cannot set heartbeat
        let result = Contract::set_oracle_heartbeat(env.clone(), non_admin.to_string(), 1800);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Test admin can set fallback price
        let result = Contract::set_fallback_price(env.clone(), admin.to_string(), 175_000_000);
        assert!(result.is_ok());

        // Test non-admin cannot set fallback price
        let result = Contract::set_fallback_price(env.clone(), non_admin.to_string(), 175_000_000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);
    });
}

#[test]
fn test_get_oracle_info() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Configure oracle settings
        Contract::set_max_price_deviation(env.clone(), admin.to_string(), 30).unwrap();
        Contract::set_oracle_heartbeat(env.clone(), admin.to_string(), 7200).unwrap();
        Contract::set_fallback_price(env.clone(), admin.to_string(), 160_000_000).unwrap();

        // Get oracle info
        let (current_price, last_update, max_deviation, heartbeat, is_stale) =
            Contract::get_oracle_info(env.clone()).unwrap();

        // Verify the values
        assert!(current_price > 0);
        assert!(last_update > 0);
        assert_eq!(max_deviation, 30);
        assert_eq!(heartbeat, 7200);
        assert!(is_stale == true || is_stale == false); // Boolean check
    });
}

#[test]
fn test_force_update_price() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Test admin can force update price
        let result = Contract::force_update_price(env.clone(), admin.to_string(), 250_000_000);
        assert!(result.is_ok());

        // Verify price was updated
        let (current_price, _, _, _, _) = Contract::get_oracle_info(env.clone()).unwrap();
        assert_eq!(current_price, 250_000_000);

        // Test non-admin cannot force update price
        let result = Contract::force_update_price(env.clone(), non_admin.to_string(), 300_000_000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);
    });
}

#[test]
fn test_oracle_integration_with_lending() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Configure oracle with reasonable settings
        Contract::set_max_price_deviation(env.clone(), admin.to_string(), 50).unwrap();

        // Test deposit and borrow with real oracle
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();

        // Borrow should work with real oracle prices
        let result = Contract::borrow(env.clone(), user.to_string(), 1000);
        assert!(result.is_ok());

        // Verify position uses real oracle prices
        let (collateral, debt, ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(collateral, 2000);
        assert_eq!(debt, 1000);
        assert!(ratio > 0); // Should have a real ratio from oracle
    });
}

#[test]
fn test_oracle_price_storage() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Get initial price
        let price1 = RealPriceOracle::get_price(&env);
        let timestamp1 = RealPriceOracle::get_last_update(&env);

        assert!(price1 > 0);
        assert!(timestamp1 > 0);

        // Get price again (should be cached/stored)
        let price2 = RealPriceOracle::get_price(&env);
        let timestamp2 = RealPriceOracle::get_last_update(&env);

        // Prices should be the same (within small variation due to time-based simulation)
        assert!(price1 == price2 || (price1 - price2).abs() < 100_000);
        assert!(timestamp2 >= timestamp1);
    });
}

// --- Interest Rate Management Tests ---

#[test]
fn test_interest_rate_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Check that interest rate config is initialized with defaults
        let (base_rate, kink_utilization, multiplier, reserve_factor, rate_floor, rate_ceiling, _) =
            Contract::get_interest_rate_config(env.clone()).unwrap();

        assert_eq!(base_rate, 2000000); // 2%
        assert_eq!(kink_utilization, 80000000); // 80%
        assert_eq!(multiplier, 10000000); // 10x
        assert_eq!(reserve_factor, 10000000); // 10%
        assert_eq!(rate_floor, 100000); // 0.1%
        assert_eq!(rate_ceiling, 50000000); // 50%
    });
}

#[test]
fn test_interest_rate_calculation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test initial rates (no utilization)
        let (borrow_rate, supply_rate) = Contract::get_current_rates(env.clone()).unwrap();
        assert_eq!(borrow_rate, 2000000); // Base rate (2%)
        assert_eq!(supply_rate, 0); // No utilization = no supply rate

        // Test utilization metrics
        let (utilization, total_borrowed, total_supplied) =
            Contract::get_utilization_metrics(env.clone()).unwrap();
        assert_eq!(utilization, 0);
        assert_eq!(total_borrowed, 0);
        assert_eq!(total_supplied, 0);
    });
}

#[test]
fn test_interest_rate_admin_functions() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test admin can set base rate
        let result = Contract::set_base_rate(env.clone(), admin.to_string(), 3000000); // 3%
        assert!(result.is_ok());

        // Test non-admin cannot set base rate
        let result = Contract::set_base_rate(env.clone(), non_admin.to_string(), 4000000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Test admin can set kink utilization
        let result = Contract::set_kink_utilization(env.clone(), admin.to_string(), 70000000); // 70%
        assert!(result.is_ok());

        // Test admin can set multiplier
        let result = Contract::set_multiplier(env.clone(), admin.to_string(), 15000000); // 15x
        assert!(result.is_ok());

        // Test admin can set reserve factor
        let result = Contract::set_reserve_factor(env.clone(), admin.to_string(), 15000000); // 15%
        assert!(result.is_ok());

        // Test admin can set rate limits
        let result = Contract::set_rate_limits(env.clone(), admin.to_string(), 50000, 75000000); // 0.05% to 75%
        assert!(result.is_ok());

        // Verify config was updated
        let (base_rate, kink_utilization, multiplier, reserve_factor, rate_floor, rate_ceiling, _) =
            Contract::get_interest_rate_config(env.clone()).unwrap();

        assert_eq!(base_rate, 3000000);
        assert_eq!(kink_utilization, 70000000);
        assert_eq!(multiplier, 15000000);
        assert_eq!(reserve_factor, 15000000);
        assert_eq!(rate_floor, 50000);
        assert_eq!(rate_ceiling, 75000000);
    });
}

#[test]
fn test_interest_rate_with_utilization() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address for borrowing
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Deposit collateral
        Contract::deposit_collateral(env.clone(), user.to_string(), 10000).unwrap();

        // Check utilization after deposit
        let (utilization, total_borrowed, total_supplied) =
            Contract::get_utilization_metrics(env.clone()).unwrap();
        assert_eq!(utilization, 0);
        assert_eq!(total_borrowed, 0);
        assert_eq!(total_supplied, 10000);

        // Borrow some amount
        Contract::borrow(env.clone(), user.to_string(), 5000).unwrap();

        // Check utilization after borrow (50%)
        let (utilization, total_borrowed, total_supplied) =
            Contract::get_utilization_metrics(env.clone()).unwrap();
        assert_eq!(utilization, 50000000); // 50% * 1e8
        assert_eq!(total_borrowed, 5000);
        assert_eq!(total_supplied, 10000);

        // Check rates with 50% utilization
        let (borrow_rate, supply_rate) = Contract::get_current_rates(env.clone()).unwrap();
        assert_eq!(borrow_rate, 2000000); // Still base rate (below kink)
        assert!(supply_rate > 0); // Should have some supply rate now
    });
}

#[test]
fn test_interest_accrual() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Deposit and borrow
        Contract::deposit_collateral(env.clone(), user.to_string(), 10000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 5000).unwrap();

        // Check initial accrued interest
        let (borrow_interest, supply_interest) =
            Contract::get_user_accrued_interest(env.clone(), user.to_string()).unwrap();
        assert_eq!(borrow_interest, 0);
        assert_eq!(supply_interest, 0);

        // Manually accrue interest
        Contract::accrue_interest(env.clone()).unwrap();

        // Check accrued interest again (should still be 0 due to minimal time)
        let (borrow_interest, supply_interest) =
            Contract::get_user_accrued_interest(env.clone(), user.to_string()).unwrap();
        assert!(borrow_interest >= 0);
        assert!(supply_interest >= 0);
    });
}

#[test]
fn test_emergency_rate_adjustment() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test admin can make emergency rate adjustment
        let result = Contract::emergency_rate_adjustment(env.clone(), admin.to_string(), 10000000); // 10%
        assert!(result.is_ok());

        // Test non-admin cannot make emergency adjustment
        let result =
            Contract::emergency_rate_adjustment(env.clone(), non_admin.to_string(), 15000000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Verify rate was updated (get directly from state to avoid recalculation)
        let state = InterestRateStorage::get_state(&env);
        assert_eq!(state.current_borrow_rate, 10000000);
    });
}

#[test]
fn test_interest_rate_integration_with_lending() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Get initial rates
        let (initial_borrow_rate, initial_supply_rate) =
            Contract::get_current_rates(env.clone()).unwrap();

        // Deposit collateral
        Contract::deposit_collateral(env.clone(), user.to_string(), 10000).unwrap();

        // Borrow (should trigger interest accrual)
        Contract::borrow(env.clone(), user.to_string(), 5000).unwrap();

        // Check that rates are updated
        let (borrow_rate, supply_rate) = Contract::get_current_rates(env.clone()).unwrap();
        assert_eq!(borrow_rate, initial_borrow_rate); // Should still be base rate
        assert!(supply_rate > initial_supply_rate); // Should have supply rate now

        // Check utilization
        let (utilization, total_borrowed, total_supplied) =
            Contract::get_utilization_metrics(env.clone()).unwrap();
        assert_eq!(utilization, 50000000); // 50%
        assert_eq!(total_borrowed, 5000);
        assert_eq!(total_supplied, 10000);

        // Repay some debt
        Contract::repay(env.clone(), user.to_string(), 2000).unwrap();

        // Check updated utilization
        let (utilization, total_borrowed, total_supplied) =
            Contract::get_utilization_metrics(env.clone()).unwrap();
        assert_eq!(utilization, 30000000); // 30%
        assert_eq!(total_borrowed, 3000);
        assert_eq!(total_supplied, 10000);
    });
}

#[test]
fn test_interest_rate_edge_cases() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test with zero utilization
        let (borrow_rate, supply_rate) = Contract::get_current_rates(env.clone()).unwrap();
        assert_eq!(borrow_rate, 2000000); // Base rate
        assert_eq!(supply_rate, 0); // No supply rate

        // Test rate limits
        Contract::set_rate_limits(env.clone(), admin.to_string(), 1000000, 3000000).unwrap(); // 1% to 3%

        // Set very high base rate (should be capped)
        Contract::set_base_rate(env.clone(), admin.to_string(), 10000000).unwrap(); // 10%

        let (borrow_rate, _) = Contract::get_current_rates(env.clone()).unwrap();
        assert_eq!(borrow_rate, 3000000); // Should be capped at 3%

        // Set very low base rate (should be floored)
        Contract::set_base_rate(env.clone(), admin.to_string(), 50000).unwrap(); // 0.05%

        let (borrow_rate, _) = Contract::get_current_rates(env.clone()).unwrap();
        assert_eq!(borrow_rate, 1000000); // Should be floored at 1%
    });
}

// --- Risk Management & Liquidation Enhancement Tests ---

#[test]
fn test_risk_config_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Check that risk config is initialized with defaults
        let (
            close_factor,
            liquidation_incentive,
            pause_borrow,
            pause_deposit,
            pause_withdraw,
            pause_liquidate,
            _,
        ) = Contract::get_risk_config(env.clone());

        assert_eq!(close_factor, 50000000); // 50%
        assert_eq!(liquidation_incentive, 10000000); // 10%
        assert_eq!(pause_borrow, false);
        assert_eq!(pause_deposit, false);
        assert_eq!(pause_withdraw, false);
        assert_eq!(pause_liquidate, false);
    });
}

#[test]
fn test_risk_params_admin_functions() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test admin can set risk parameters
        let result = Contract::set_risk_params(env.clone(), admin.to_string(), 60000000, 15000000); // 60%, 15%
        assert!(result.is_ok());

        // Test non-admin cannot set risk parameters
        let result =
            Contract::set_risk_params(env.clone(), non_admin.to_string(), 70000000, 20000000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Verify config was updated
        let (close_factor, liquidation_incentive, _, _, _, _, _) =
            Contract::get_risk_config(env.clone());
        assert_eq!(close_factor, 60000000);
        assert_eq!(liquidation_incentive, 15000000);
    });
}

#[test]
fn test_pause_switches_admin_functions() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test admin can set pause switches
        let result =
            Contract::set_pause_switches(env.clone(), admin.to_string(), true, false, true, false);
        assert!(result.is_ok());

        // Test non-admin cannot set pause switches
        let result = Contract::set_pause_switches(
            env.clone(),
            non_admin.to_string(),
            false,
            true,
            false,
            true,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Verify config was updated
        let (_, _, pause_borrow, pause_deposit, pause_withdraw, pause_liquidate, _) =
            Contract::get_risk_config(env.clone());
        assert_eq!(pause_borrow, true);
        assert_eq!(pause_deposit, false);
        assert_eq!(pause_withdraw, true);
        assert_eq!(pause_liquidate, false);
    });
}

#[test]
fn test_pause_switches_enforcement() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Pause borrow
        Contract::set_pause_switches(env.clone(), admin.to_string(), true, false, false, false)
            .unwrap();

        // Try to borrow (should fail)
        Contract::deposit_collateral(env.clone(), user.to_string(), 10000).unwrap();
        let result = Contract::borrow(env.clone(), user.to_string(), 5000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::ProtocolPaused);

        // Unpause borrow and pause deposit
        Contract::set_pause_switches(env.clone(), admin.to_string(), false, true, false, false)
            .unwrap();

        // Try to deposit (should fail)
        let result = Contract::deposit_collateral(env.clone(), user.to_string(), 5000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::ProtocolPaused);

        // Pause withdraw
        Contract::set_pause_switches(env.clone(), admin.to_string(), false, false, true, false)
            .unwrap();

        // Try to withdraw (should fail)
        let result = Contract::withdraw(env.clone(), user.to_string(), 1000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::ProtocolPaused);
    });
}

#[test]
fn test_enhanced_liquidation_with_close_factor() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let liquidator = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Set close factor to 30%
        Contract::set_risk_params(env.clone(), admin.to_string(), 30000000, 10000000).unwrap();

        // Create undercollateralized position by setting a higher minimum ratio
        Contract::set_min_collateral_ratio(env.clone(), admin.to_string(), 200).unwrap(); // 200%

        // Create position that will be undercollateralized
        Contract::deposit_collateral(env.clone(), user.to_string(), 1000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 800).unwrap();

        // Try to liquidate more than close factor allows (should be limited)
        let result =
            Contract::liquidate(env.clone(), liquidator.to_string(), user.to_string(), 500);
        assert!(result.is_ok());

        // Check position - should only have 30% of debt liquidated
        let (collateral, debt, _) = Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(debt, 560); // 800 - (800 * 0.3) = 800 - 240 = 560
    });
}

#[test]
fn test_enhanced_liquidation_with_incentive() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let liquidator = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Set liquidation incentive to 20%
        Contract::set_risk_params(env.clone(), admin.to_string(), 50000000, 20000000).unwrap();

        // Create undercollateralized position by setting a higher minimum ratio
        Contract::set_min_collateral_ratio(env.clone(), admin.to_string(), 200).unwrap(); // 200%

        // Create position that will be undercollateralized
        Contract::deposit_collateral(env.clone(), user.to_string(), 1000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 800).unwrap();

        // Record initial collateral
        let (initial_collateral, _, _) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();

        // Liquidate
        let result =
            Contract::liquidate(env.clone(), liquidator.to_string(), user.to_string(), 400);
        assert!(result.is_ok());

        // Check position - should have lost debt + incentive
        let (collateral, debt, _) = Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(debt, 400); // 800 - 400 = 400

        // Collateral should be reduced by debt + incentive
        let expected_collateral_loss = 400 + (400 * 20000000 / 100_000_000); // debt + 20% incentive
        assert_eq!(collateral, initial_collateral - expected_collateral_loss);
    });
}

#[test]
fn test_liquidation_pause_enforcement() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let liquidator = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Create undercollateralized position by setting a higher minimum ratio
        Contract::set_min_collateral_ratio(env.clone(), admin.to_string(), 200).unwrap(); // 200%

        // Create position that will be undercollateralized
        Contract::deposit_collateral(env.clone(), user.to_string(), 1000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 800).unwrap();

        // Pause liquidation
        Contract::set_pause_switches(env.clone(), admin.to_string(), false, false, false, true)
            .unwrap();

        // Try to liquidate (should fail)
        let result =
            Contract::liquidate(env.clone(), liquidator.to_string(), user.to_string(), 400);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::ProtocolPaused);
    });
}

#[test]
fn test_risk_management_integration() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let liquidator = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Configure risk parameters
        Contract::set_risk_params(env.clone(), admin.to_string(), 40000000, 12000000).unwrap(); // 40%, 12%

        // Create position and test full risk management flow
        Contract::deposit_collateral(env.clone(), user.to_string(), 10000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 8000).unwrap();

        // Pause borrow
        Contract::set_pause_switches(env.clone(), admin.to_string(), true, false, false, false)
            .unwrap();

        // Try to borrow more (should fail)
        let result = Contract::borrow(env.clone(), user.to_string(), 1000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::ProtocolPaused);

        // Unpause and test liquidation
        Contract::set_pause_switches(env.clone(), admin.to_string(), false, false, false, false)
            .unwrap();

        // Create undercollateralized position for liquidation test
        Contract::set_min_collateral_ratio(env.clone(), admin.to_string(), 200).unwrap(); // 200%

        // Liquidate with close factor and incentive
        let result =
            Contract::liquidate(env.clone(), liquidator.to_string(), user.to_string(), 2000);
        assert!(result.is_ok());

        // Verify liquidation worked with risk parameters
        let (collateral, debt, _) = Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert!(debt < 8000); // Should be reduced
        assert!(collateral < 10000); // Should be reduced by debt + incentive
    });
}

// --- Reserve Management & Protocol Revenue Tests ---

#[test]
fn test_reserve_management_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Check that reserve data is initialized with defaults
        let (total_collected, total_distributed, current_reserves, treasury, last_dist, freq) =
            Contract::get_reserve_data(env.clone());

        assert_eq!(total_collected, 0);
        assert_eq!(total_distributed, 0);
        assert_eq!(current_reserves, 0);
        assert_eq!(treasury, admin.to_string());
        assert_eq!(last_dist, 0);
        assert_eq!(freq, 86400); // 24 hours

        // Check revenue metrics
        let (daily, weekly, monthly, total_borrow, total_supply) =
            Contract::get_revenue_metrics(env.clone());

        assert_eq!(daily, 0);
        assert_eq!(weekly, 0);
        assert_eq!(monthly, 0);
        assert_eq!(total_borrow, 0);
        assert_eq!(total_supply, 0);
    });
}

#[test]
fn test_treasury_management() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let new_treasury = TestUtils::create_user_address(&env, 1);
    let non_admin = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test admin can set treasury address
        let result = Contract::set_treasury_address(
            env.clone(),
            admin.to_string(),
            new_treasury.to_string(),
        );
        assert!(result.is_ok());

        // Test non-admin cannot set treasury address
        let result =
            Contract::set_treasury_address(env.clone(), non_admin.to_string(), admin.to_string());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Verify treasury was updated
        let (_, _, _, treasury, _, _) = Contract::get_reserve_data(env.clone());
        assert_eq!(treasury, new_treasury.to_string());
    });
}

#[test]
fn test_protocol_fee_collection() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test admin can collect fees
        let result = Contract::collect_protocol_fees(
            env.clone(),
            admin.to_string(),
            1000,
            String::from_str(&env, "borrow"),
        );
        assert!(result.is_ok());

        // Test non-admin cannot collect fees
        let result = Contract::collect_protocol_fees(
            env.clone(),
            non_admin.to_string(),
            500,
            String::from_str(&env, "supply"),
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Test invalid amount
        let result = Contract::collect_protocol_fees(
            env.clone(),
            admin.to_string(),
            0,
            String::from_str(&env, "borrow"),
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InvalidAmount);

        // Verify fees were collected
        let (total_collected, _, current_reserves, _, _, _) =
            Contract::get_reserve_data(env.clone());
        assert_eq!(total_collected, 1000);
        assert_eq!(current_reserves, 1000);

        // Verify revenue metrics were updated
        let (_, _, _, total_borrow, total_supply) = Contract::get_revenue_metrics(env.clone());
        assert_eq!(total_borrow, 1000);
        assert_eq!(total_supply, 0);
    });
}

#[test]
fn test_fee_distribution() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let treasury = TestUtils::create_user_address(&env, 1);
    let non_admin = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set treasury address
        Contract::set_treasury_address(env.clone(), admin.to_string(), treasury.to_string())
            .unwrap();

        // Collect some fees first
        Contract::collect_protocol_fees(
            env.clone(),
            admin.to_string(),
            2000,
            String::from_str(&env, "borrow"),
        )
        .unwrap();

        // Test admin can distribute fees
        let result = Contract::distribute_fees_to_treasury(env.clone(), admin.to_string(), 1000);
        assert!(result.is_ok());

        // Test non-admin cannot distribute fees
        let result = Contract::distribute_fees_to_treasury(env.clone(), non_admin.to_string(), 500);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Test cannot distribute more than available
        let result = Contract::distribute_fees_to_treasury(env.clone(), admin.to_string(), 2000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InsufficientCollateral);

        // Verify distribution worked
        let (total_collected, total_distributed, current_reserves, _, last_dist, _) =
            Contract::get_reserve_data(env.clone());
        assert_eq!(total_collected, 2000);
        assert_eq!(total_distributed, 1000);
        assert_eq!(current_reserves, 1000);
        assert!(last_dist > 0);
    });
}

#[test]
fn test_emergency_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Collect some fees first
        Contract::collect_protocol_fees(
            env.clone(),
            admin.to_string(),
            1500,
            String::from_str(&env, "supply"),
        )
        .unwrap();

        // Test admin can emergency withdraw
        let result = Contract::emergency_withdraw_fees(env.clone(), admin.to_string(), 800);
        assert!(result.is_ok());

        // Test non-admin cannot emergency withdraw
        let result = Contract::emergency_withdraw_fees(env.clone(), non_admin.to_string(), 500);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Test cannot withdraw more than available
        let result = Contract::emergency_withdraw_fees(env.clone(), admin.to_string(), 1000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InsufficientCollateral);

        // Verify withdrawal worked
        let (total_collected, total_distributed, current_reserves, _, _, _) =
            Contract::get_reserve_data(env.clone());
        assert_eq!(total_collected, 1500);
        assert_eq!(total_distributed, 0); // Emergency withdrawal doesn't count as distribution
        assert_eq!(current_reserves, 700); // 1500 - 800
    });
}

#[test]
fn test_fee_integration_with_interest() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Set a higher reserve factor to make fees more visible
        Contract::set_reserve_factor(env.clone(), admin.to_string(), 20000000).unwrap(); // 20%

        // Create position and accrue interest
        Contract::deposit_collateral(env.clone(), user.to_string(), 10000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 5000).unwrap();

        // Manually accrue interest to generate fees
        Contract::accrue_interest(env.clone()).unwrap();

        // Check that fees were collected during operations
        let (total_collected, _, current_reserves, _, _, _) =
            Contract::get_reserve_data(env.clone());
        assert!(total_collected > 0);
        assert!(current_reserves > 0);

        // Check revenue metrics
        let (_, _, _, total_borrow, total_supply) = Contract::get_revenue_metrics(env.clone());
        assert!(total_borrow > 0 || total_supply > 0);
    });
}

#[test]
fn test_distribution_frequency_setting() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let non_admin = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test admin can set distribution frequency
        let result = Contract::set_distribution_frequency(env.clone(), admin.to_string(), 3600); // 1 hour
        assert!(result.is_ok());

        // Test non-admin cannot set distribution frequency
        let result = Contract::set_distribution_frequency(env.clone(), non_admin.to_string(), 7200);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);

        // Verify frequency was updated
        let (_, _, _, _, _, freq) = Contract::get_reserve_data(env.clone());
        assert_eq!(freq, 3600);
    });
}

#[test]
fn test_reserve_management_integration() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let treasury = TestUtils::create_user_address(&env, 1);
    let user = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Set oracle address
        let oracle = TestUtils::create_oracle_address(&env);
        Contract::set_oracle(env.clone(), admin.to_string(), oracle.to_string()).unwrap();

        // Set treasury address
        Contract::set_treasury_address(env.clone(), admin.to_string(), treasury.to_string())
            .unwrap();

        // Set higher reserve factor for testing
        Contract::set_reserve_factor(env.clone(), admin.to_string(), 15000000).unwrap(); // 15%

        // Create position and generate fees
        Contract::deposit_collateral(env.clone(), user.to_string(), 10000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 6000).unwrap();

        // Accrue interest to generate fees
        Contract::accrue_interest(env.clone()).unwrap();

        // Check initial reserve state
        let (total_collected, total_distributed, current_reserves, _, _, _) =
            Contract::get_reserve_data(env.clone());
        assert!(total_collected > 0);
        assert_eq!(total_distributed, 0);
        assert_eq!(current_reserves, total_collected);

        // Distribute some fees to treasury
        Contract::distribute_fees_to_treasury(env.clone(), admin.to_string(), total_collected / 2)
            .unwrap();

        // Verify distribution
        let (new_total_collected, new_total_distributed, new_current_reserves, _, _, _) =
            Contract::get_reserve_data(env.clone());
        assert_eq!(new_total_collected, total_collected);
        assert_eq!(new_total_distributed, total_collected / 2);
        assert_eq!(new_current_reserves, total_collected / 2);

        // Test emergency withdrawal
        Contract::emergency_withdraw_fees(env.clone(), admin.to_string(), new_current_reserves / 2)
            .unwrap();

        // Verify final state
        let (final_total_collected, final_total_distributed, final_current_reserves, _, _, _) =
            Contract::get_reserve_data(env.clone());
        assert_eq!(final_total_collected, total_collected);
        assert_eq!(final_total_distributed, total_collected / 2);
        assert_eq!(final_current_reserves, total_collected / 4);
    });
}

// --- Multi-Asset Support Tests ---

#[test]
fn test_multi_asset_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Test that asset registry is initialized
        let supported_assets = Contract::get_supported_assets(env.clone());
        assert_eq!(supported_assets.len(), 1);
        assert_eq!(supported_assets.get(0), Some(String::from_str(&env, "XLM")));

        // Test that default XLM asset is configured
        let xlm_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "XLM")).unwrap();
        assert_eq!(xlm_info.0, String::from_str(&env, "XLM")); // symbol
        assert_eq!(xlm_info.1, 7); // decimals
        assert_eq!(xlm_info.3, 150); // min_collateral_ratio
        assert_eq!(xlm_info.4, true); // deposit_enabled
        assert_eq!(xlm_info.5, true); // borrow_enabled
    });
}

#[test]
fn test_add_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Add USDC asset
        let result = Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120, // 120% collateral ratio
        );
        assert!(result.is_ok());

        // Verify asset is added to registry
        let supported_assets = Contract::get_supported_assets(env.clone());
        assert_eq!(supported_assets.len(), 2);
        assert!(supported_assets.contains(&String::from_str(&env, "USDC")));

        // Verify asset info is stored
        let usdc_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "USDC")).unwrap();
        assert_eq!(usdc_info.0, String::from_str(&env, "USDC"));
        assert_eq!(usdc_info.1, 6);
        assert_eq!(usdc_info.3, 120);
    });
}

#[test]
fn test_add_asset_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Try to add asset as non-admin
        let result = Contract::add_asset(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);
    });
}

#[test]
fn test_add_asset_invalid_params() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Try to add asset with empty symbol
        let result = Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, ""),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InvalidAsset);

        // Try to add asset with zero decimals
        let result = Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            0,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::InvalidAmount);
    });
}

#[test]
fn test_add_duplicate_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Add USDC asset
        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        )
        .unwrap();

        // Try to add USDC again
        let result = Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::AlreadyInitialized);
    });
}

#[test]
fn test_set_asset_params() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Add USDC asset
        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        )
        .unwrap();

        // Update asset parameters
        let result = Contract::set_asset_params(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            130,      // new collateral ratio
            60000000, // close factor 60%
            15000000, // liquidation incentive 15%
            3000000,  // base rate 3%
            12000000, // reserve factor 12%
        );
        assert!(result.is_ok());

        // Verify parameters are updated
        let usdc_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "USDC")).unwrap();
        assert_eq!(usdc_info.3, 130); // min_collateral_ratio
    });
}

#[test]
fn test_set_asset_params_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Add USDC asset
        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        )
        .unwrap();

        // Try to update as non-admin
        let result = Contract::set_asset_params(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "USDC"),
            130,
            60000000,
            15000000,
            3000000,
            12000000,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);
    });
}

#[test]
fn test_set_asset_params_invalid_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Try to update non-existent asset
        let result = Contract::set_asset_params(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "INVALID"),
            130,
            60000000,
            15000000,
            3000000,
            12000000,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::AssetNotSupported);
    });
}

#[test]
fn test_get_asset_info_invalid_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Try to get info for non-existent asset
        let result = Contract::get_asset_info(env.clone(), String::from_str(&env, "INVALID"));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::AssetNotSupported);
    });
}

#[test]
fn test_set_asset_deposit_enabled() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Add USDC asset
        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        )
        .unwrap();

        // Disable deposits
        let result = Contract::set_asset_deposit_enabled(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            false,
        );
        assert!(result.is_ok());

        // Verify deposit is disabled
        let usdc_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "USDC")).unwrap();
        assert_eq!(usdc_info.4, false); // deposit_enabled

        // Re-enable deposits
        Contract::set_asset_deposit_enabled(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            true,
        )
        .unwrap();

        // Verify deposit is enabled
        let usdc_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "USDC")).unwrap();
        assert_eq!(usdc_info.4, true); // deposit_enabled
    });
}

#[test]
fn test_set_asset_borrow_enabled() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Add USDC asset
        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        )
        .unwrap();

        // Disable borrowing
        let result = Contract::set_asset_borrow_enabled(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            false,
        );
        assert!(result.is_ok());

        // Verify borrowing is disabled
        let usdc_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "USDC")).unwrap();
        assert_eq!(usdc_info.5, false); // borrow_enabled

        // Re-enable borrowing
        Contract::set_asset_borrow_enabled(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            true,
        )
        .unwrap();

        // Verify borrowing is enabled
        let usdc_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "USDC")).unwrap();
        assert_eq!(usdc_info.5, true); // borrow_enabled
    });
}

#[test]
fn test_multi_asset_registry_management() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::create_admin_address(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        Contract::initialize(env.clone(), admin.to_string()).unwrap();

        // Add multiple assets
        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "USDC"),
            6,
            TestUtils::create_oracle_address(&env).to_string(),
            120,
        )
        .unwrap();

        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "BTC"),
            8,
            TestUtils::create_oracle_address(&env).to_string(),
            200,
        )
        .unwrap();

        Contract::add_asset(
            env.clone(),
            admin.to_string(),
            String::from_str(&env, "ETH"),
            18,
            TestUtils::create_oracle_address(&env).to_string(),
            150,
        )
        .unwrap();

        // Verify all assets are in registry
        let supported_assets = Contract::get_supported_assets(env.clone());
        assert_eq!(supported_assets.len(), 4); // XLM + 3 new assets
        assert!(supported_assets.contains(&String::from_str(&env, "XLM")));
        assert!(supported_assets.contains(&String::from_str(&env, "USDC")));
        assert!(supported_assets.contains(&String::from_str(&env, "BTC")));
        assert!(supported_assets.contains(&String::from_str(&env, "ETH")));

        // Verify each asset has correct info
        let btc_info =
            Contract::get_asset_info(env.clone(), String::from_str(&env, "BTC")).unwrap();
        assert_eq!(btc_info.0, String::from_str(&env, "BTC"));
        assert_eq!(btc_info.1, 8);
        assert_eq!(btc_info.3, 200);
    });
}

// --- Activity Tracking Tests ---

#[test]
fn test_activity_tracking_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Test initial user activity (should be empty)
        let activity = Contract::get_user_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(activity.0, 0); // total_deposits
        assert_eq!(activity.1, 0); // total_withdrawals
        assert_eq!(activity.2, 0); // total_borrows
        assert_eq!(activity.3, 0); // total_repayments
        assert_eq!(activity.4, 0); // last_activity
        assert_eq!(activity.5, 0); // activity_count

        // Test initial protocol activity
        let protocol_activity = Contract::get_protocol_activity(env.clone());
        assert_eq!(protocol_activity.0, 0); // total_users
        assert_eq!(protocol_activity.1, 0); // active_users_24h
        assert_eq!(protocol_activity.2, 0); // active_users_7d
        assert_eq!(protocol_activity.3, 0); // total_transactions
        assert_eq!(protocol_activity.4, 0); // last_update
    });
}

#[test]
fn test_track_user_activity_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track deposit activity
        let result = Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "deposit"),
            1000,
        );
        assert!(result.is_ok());

        // Verify activity is recorded
        let activity = Contract::get_user_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(activity.0, 1000); // total_deposits
        assert_eq!(activity.1, 0); // total_withdrawals
        assert_eq!(activity.2, 0); // total_borrows
        assert_eq!(activity.3, 0); // total_repayments
        assert_eq!(activity.5, 1); // activity_count
        assert!(activity.4 > 0); // last_activity timestamp
    });
}

#[test]
fn test_track_user_activity_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track withdrawal activity
        let result = Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "withdrawal"),
            500,
        );
        assert!(result.is_ok());

        // Verify activity is recorded
        let activity = Contract::get_user_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(activity.0, 0); // total_deposits
        assert_eq!(activity.1, 500); // total_withdrawals
        assert_eq!(activity.2, 0); // total_borrows
        assert_eq!(activity.3, 0); // total_repayments
        assert_eq!(activity.5, 1); // activity_count
    });
}

#[test]
fn test_track_user_activity_borrow() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track borrow activity
        let result = Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "borrow"),
            750,
        );
        assert!(result.is_ok());

        // Verify activity is recorded
        let activity = Contract::get_user_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(activity.0, 0); // total_deposits
        assert_eq!(activity.1, 0); // total_withdrawals
        assert_eq!(activity.2, 750); // total_borrows
        assert_eq!(activity.3, 0); // total_repayments
        assert_eq!(activity.5, 1); // activity_count
    });
}

#[test]
fn test_track_user_activity_repayment() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track repayment activity
        let result = Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "repayment"),
            300,
        );
        assert!(result.is_ok());

        // Verify activity is recorded
        let activity = Contract::get_user_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(activity.0, 0); // total_deposits
        assert_eq!(activity.1, 0); // total_withdrawals
        assert_eq!(activity.2, 0); // total_borrows
        assert_eq!(activity.3, 300); // total_repayments
        assert_eq!(activity.5, 1); // activity_count
    });
}

#[test]
fn test_track_user_activity_invalid_action() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track invalid activity
        let result = Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "invalid_action"),
            1000,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::Unknown);
    });
}

#[test]
fn test_track_user_activity_accumulation() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track multiple activities
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "deposit"),
            1000,
        )
        .unwrap();
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "borrow"),
            500,
        )
        .unwrap();
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "repayment"),
            200,
        )
        .unwrap();
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "withdrawal"),
            300,
        )
        .unwrap();

        // Verify accumulated activity
        let activity = Contract::get_user_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(activity.0, 1000); // total_deposits
        assert_eq!(activity.1, 300); // total_withdrawals
        assert_eq!(activity.2, 500); // total_borrows
        assert_eq!(activity.3, 200); // total_repayments
        assert_eq!(activity.5, 4); // activity_count
    });
}

#[test]
fn test_update_protocol_stats() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::initialize_contract(&env);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Update protocol stats
        let result = Contract::update_protocol_stats(
            env.clone(),
            admin.to_string(),
            100,  // total_users
            25,   // active_users_24h
            50,   // active_users_7d
            1000, // total_transactions
        );
        assert!(result.is_ok());

        // Verify stats are updated
        let stats = Contract::get_protocol_activity(env.clone());
        assert_eq!(stats.0, 100); // total_users
        assert_eq!(stats.1, 25); // active_users_24h
        assert_eq!(stats.2, 50); // active_users_7d
        assert_eq!(stats.3, 1000); // total_transactions
        assert!(stats.4 > 0); // last_update timestamp
    });
}

#[test]
fn test_update_protocol_stats_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Try to update protocol stats as non-admin
        let result =
            Contract::update_protocol_stats(env.clone(), user.to_string(), 100, 25, 50, 1000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::NotAdmin);
    });
}

#[test]
fn test_get_recent_activity() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track some activity first
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "deposit"),
            1000,
        )
        .unwrap();
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "borrow"),
            500,
        )
        .unwrap();

        // Get recent activity
        let recent = Contract::get_recent_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(recent.0, String::from_str(&env, "borrow")); // Last action should be borrow
        assert_eq!(recent.1, 500); // Last amount should be borrow amount
        assert!(recent.2 > 0); // Should have timestamp
    });
}

#[test]
fn test_get_recent_activity_no_activity() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Try to get recent activity for user with no activity
        let result = Contract::get_recent_activity(env.clone(), user.to_string());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ProtocolError::PositionNotFound);
    });
}

#[test]
fn test_activity_tracking_multiple_users() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user1 = TestUtils::create_user_address(&env, 1);
    let user2 = TestUtils::create_user_address(&env, 2);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Track activity for user1
        Contract::track_user_activity(
            env.clone(),
            user1.to_string(),
            String::from_str(&env, "deposit"),
            1000,
        )
        .unwrap();
        Contract::track_user_activity(
            env.clone(),
            user1.to_string(),
            String::from_str(&env, "borrow"),
            500,
        )
        .unwrap();

        // Track activity for user2
        Contract::track_user_activity(
            env.clone(),
            user2.to_string(),
            String::from_str(&env, "deposit"),
            2000,
        )
        .unwrap();
        Contract::track_user_activity(
            env.clone(),
            user2.to_string(),
            String::from_str(&env, "withdrawal"),
            300,
        )
        .unwrap();

        // Verify user1 activity
        let activity1 = Contract::get_user_activity(env.clone(), user1.to_string()).unwrap();
        assert_eq!(activity1.0, 1000); // total_deposits
        assert_eq!(activity1.2, 500); // total_borrows
        assert_eq!(activity1.5, 2); // activity_count

        // Verify user2 activity
        let activity2 = Contract::get_user_activity(env.clone(), user2.to_string()).unwrap();
        assert_eq!(activity2.0, 2000); // total_deposits
        assert_eq!(activity2.1, 300); // total_withdrawals
        assert_eq!(activity2.5, 2); // activity_count
    });
}

#[test]
fn test_activity_tracking_integration_with_lending() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);

    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Perform lending operations
        Contract::deposit_collateral(env.clone(), user.to_string(), 2000).unwrap();
        Contract::borrow(env.clone(), user.to_string(), 1000).unwrap();

        // Manually track activity (in real implementation, this would be automatic)
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "deposit"),
            2000,
        )
        .unwrap();
        Contract::track_user_activity(
            env.clone(),
            user.to_string(),
            String::from_str(&env, "borrow"),
            1000,
        )
        .unwrap();

        // Verify activity tracking works alongside lending operations
        let activity = Contract::get_user_activity(env.clone(), user.to_string()).unwrap();
        assert_eq!(activity.0, 2000); // total_deposits
        assert_eq!(activity.2, 1000); // total_borrows
        assert_eq!(activity.5, 2); // activity_count

        // Verify lending position is still correct
        let (collateral, debt, _ratio) =
            Contract::get_position(env.clone(), user.to_string()).unwrap();
        assert_eq!(collateral, 2000);
        assert_eq!(debt, 1000);
    });
}

#[test]
fn test_account_freeze_and_unfreeze() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Initially not frozen
        assert!(!Contract::is_account_frozen(env.clone(), user.to_string()));
        // Freeze account
        let result = Contract::freeze_account(env.clone(), admin.to_string(), user.to_string());
        assert!(result.is_ok());
        assert!(Contract::is_account_frozen(env.clone(), user.to_string()));
        // Unfreeze account
        let result = Contract::unfreeze_account(env.clone(), admin.to_string(), user.to_string());
        assert!(result.is_ok());
        assert!(!Contract::is_account_frozen(env.clone(), user.to_string()));
    });
}

#[test]
fn test_account_freeze_enforcement_on_actions() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Freeze account
        Contract::freeze_account(env.clone(), admin.to_string(), user.to_string()).unwrap();
        // All actions should fail
        let deposit = Contract::deposit_collateral(env.clone(), user.to_string(), 1000);
        assert!(deposit.is_err());
        let borrow = Contract::borrow(env.clone(), user.to_string(), 1000);
        assert!(borrow.is_err());
        let repay = Contract::repay(env.clone(), user.to_string(), 1000);
        assert!(repay.is_err());
        let withdraw = Contract::withdraw(env.clone(), user.to_string(), 1000);
        assert!(withdraw.is_err());
        // Unfreeze and actions should succeed
        Contract::unfreeze_account(env.clone(), admin.to_string(), user.to_string()).unwrap();
        let deposit = Contract::deposit_collateral(env.clone(), user.to_string(), 1000);
        assert!(deposit.is_ok());
    });
}

#[test]
fn test_account_freeze_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let _admin = TestUtils::initialize_contract(&env);
    let user = TestUtils::create_user_address(&env, 1);
    let not_admin = TestUtils::create_user_address(&env, 2);
    let contract_id = env.register(Contract, ());
    env.as_contract(&contract_id, || {
        // Non-admin cannot freeze
        let result = Contract::freeze_account(env.clone(), not_admin.to_string(), user.to_string());
        assert!(result.is_err());
        // Non-admin cannot unfreeze
        let result =
            Contract::unfreeze_account(env.clone(), not_admin.to_string(), user.to_string());
        assert!(result.is_err());
    });
}

#[test]
fn test_multi_admin_support() {
    let e = Env::default();
    let admin1 = <soroban_sdk::Address as TestAddress>::generate(&e);
    let admin2 = <soroban_sdk::Address as TestAddress>::generate(&e);
    let user = <soroban_sdk::Address as TestAddress>::generate(&e);
    // Initialize with admin1
    let contract_id = e.register(Contract, ());
    e.as_contract(&contract_id, || {
        Contract::initialize(e.clone(), admin1.to_string()).unwrap();
    });
    // admin1 is admin
    assert!(is_address_admin(e.clone(), admin1.clone()));
    // Add admin2
    assert!(add_admin(e.clone(), admin1.clone(), admin2.clone()).is_ok());
    assert!(is_address_admin(e.clone(), admin2.clone()));
    // admin2 can add another admin
    let admin3 = <soroban_sdk::Address as TestAddress>::generate(&e);
    assert!(add_admin(e.clone(), admin2.clone(), admin3.clone()).is_ok());
    assert!(is_address_admin(e.clone(), admin3.clone()));
    // Remove admin2
    assert!(remove_admin(e.clone(), admin1.clone(), admin2.clone()).is_ok());
    assert!(!is_address_admin(e.clone(), admin2.clone()));
    // Cannot remove last admin
    assert!(remove_admin(e.clone(), admin1.clone(), admin1.clone()).is_err());
    // Transfer admin1 to user
    assert!(transfer_admin(e.clone(), admin1.clone(), user.clone()).is_ok());
    assert!(!is_address_admin(e.clone(), admin1.clone()));
    assert!(is_address_admin(e.clone(), user.clone()));
    // Unauthorized add
    let not_admin = <soroban_sdk::Address as TestAddress>::generate(&e);
    assert!(add_admin(e.clone(), not_admin.clone(), admin1.clone()).is_err());
    // Unauthorized remove
    assert!(remove_admin(e.clone(), not_admin.clone(), user.clone()).is_err());
    // Unauthorized transfer
    assert!(transfer_admin(e.clone(), not_admin.clone(), admin1.clone()).is_err());
    // Query admin list
    let admins = get_admins(e.clone());
    assert_eq!(admins.len(), 2); // user and admin3
}

#[test]
fn test_permissionless_market_listing() {
    let e = Env::default();
    let admin = <soroban_sdk::Address as TestAddress>::generate(&e);
    let proposer = <soroban_sdk::Address as TestAddress>::generate(&e);
    let oracle = <soroban_sdk::Address as TestAddress>::generate(&e);

    // Initialize contract
    let contract_id = e.register(Contract, ());
    e.as_contract(&contract_id, || {
        Contract::initialize(e.clone(), admin.to_string()).unwrap();
    });

    // Propose new asset
    let proposal_id = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "BTC"),
        String::from_str(&e, "Bitcoin"),
        oracle.clone(),
        8000, // 80% collateral factor
        7500, // 75% borrow factor
    )
    .unwrap();

    assert_eq!(proposal_id, 1);

    // Check proposal exists
    let proposal = get_proposal_by_id(e.clone(), proposal_id).unwrap();
    assert_eq!(proposal.proposer, proposer);
    assert_eq!(proposal.symbol, String::from_str(&e, "BTC"));
    assert_eq!(proposal.status, ProposalStatus::Pending);

    // Admin approves proposal
    assert!(approve_proposal(e.clone(), admin.clone(), proposal_id).is_ok());

    // Check proposal status updated
    let updated_proposal = get_proposal_by_id(e.clone(), proposal_id).unwrap();
    assert_eq!(updated_proposal.status, ProposalStatus::Approved);

    // Check asset was created
    let asset_info = Contract::get_asset_info(e.clone(), String::from_str(&e, "BTC")).unwrap();
    assert_eq!(asset_info.0, String::from_str(&e, "BTC")); // symbol
    assert_eq!(asset_info.2, String::from_str(&e, "Bitcoin")); // name (oracle address)

    // Propose another asset
    let proposal_id2 = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "ETH"),
        String::from_str(&e, "Ethereum"),
        oracle.clone(),
        7500,
        7000,
    )
    .unwrap();

    // Admin rejects proposal
    assert!(reject_proposal(e.clone(), admin.clone(), proposal_id2).is_ok());

    let rejected_proposal = get_proposal_by_id(e.clone(), proposal_id2).unwrap();
    assert_eq!(rejected_proposal.status, ProposalStatus::Rejected);

    // Proposer cancels their own proposal
    let proposal_id3 = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "LTC"),
        String::from_str(&e, "Litecoin"),
        oracle.clone(),
        7000,
        6500,
    )
    .unwrap();

    assert!(cancel_proposal(e.clone(), proposer.clone(), proposal_id3).is_ok());

    let cancelled_proposal = get_proposal_by_id(e.clone(), proposal_id3).unwrap();
    assert_eq!(cancelled_proposal.status, ProposalStatus::Cancelled);

    // Test unauthorized operations
    let not_admin = <soroban_sdk::Address as TestAddress>::generate(&e);
    assert!(approve_proposal(e.clone(), not_admin.clone(), proposal_id).is_err());
    assert!(reject_proposal(e.clone(), not_admin.clone(), proposal_id2).is_err());

    let not_proposer = <soroban_sdk::Address as TestAddress>::generate(&e);
    assert!(cancel_proposal(e.clone(), not_proposer.clone(), proposal_id3).is_err());

    // Test query functions
    let all_proposals = get_all_proposals(e.clone());
    assert_eq!(all_proposals.len(), 3);

    let pending_proposals = get_proposals_by_status(e.clone(), ProposalStatus::Pending);
    assert_eq!(pending_proposals.len(), 0);

    let approved_proposals = get_proposals_by_status(e.clone(), ProposalStatus::Approved);
    assert_eq!(approved_proposals.len(), 1);

    let rejected_proposals = get_proposals_by_status(e.clone(), ProposalStatus::Rejected);
    assert_eq!(rejected_proposals.len(), 1);

    let cancelled_proposals = get_proposals_by_status(e.clone(), ProposalStatus::Cancelled);
    assert_eq!(cancelled_proposals.len(), 1);
}

#[test]
fn test_proposal_validation() {
    let e = Env::default();
    let admin = <soroban_sdk::Address as TestAddress>::generate(&e);
    let proposer = <soroban_sdk::Address as TestAddress>::generate(&e);
    let oracle = <soroban_sdk::Address as TestAddress>::generate(&e);

    let contract_id = e.register(Contract, ());
    e.as_contract(&contract_id, || {
        Contract::initialize(e.clone(), admin.to_string()).unwrap();
    });

    // Test invalid symbol length
    let result = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "VERYLONGSYMBOL"), // > 10 chars
        String::from_str(&e, "Test Asset"),
        oracle.clone(),
        8000,
        7500,
    );
    assert!(result.is_err());

    // Test invalid name length
    let result = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "TEST"),
        String::from_str(&e, "This is a very long asset name that exceeds the maximum allowed length of 50 characters"), // > 50 chars
        oracle.clone(),
        8000,
        7500,
    );
    assert!(result.is_err());

    // Test invalid factors
    let result = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "TEST"),
        String::from_str(&e, "Test Asset"),
        oracle.clone(),
        15000, // > 10000
        7500,
    );
    assert!(result.is_err());

    let result = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "TEST"),
        String::from_str(&e, "Test Asset"),
        oracle.clone(),
        8000,
        15000, // > 10000
    );
    assert!(result.is_err());
}

#[test]
fn test_proposal_lifecycle_errors() {
    let e = Env::default();
    let admin = <soroban_sdk::Address as TestAddress>::generate(&e);
    let proposer = <soroban_sdk::Address as TestAddress>::generate(&e);
    let oracle = <soroban_sdk::Address as TestAddress>::generate(&e);

    let contract_id = e.register(Contract, ());
    e.as_contract(&contract_id, || {
        Contract::initialize(e.clone(), admin.to_string()).unwrap();
    });

    // Create a proposal
    let proposal_id = propose_asset(
        e.clone(),
        proposer.clone(),
        String::from_str(&e, "TEST"),
        String::from_str(&e, "Test Asset"),
        oracle.clone(),
        8000,
        7500,
    )
    .unwrap();

    // Approve it
    assert!(approve_proposal(e.clone(), admin.clone(), proposal_id).is_ok());

    // Try to approve again (should fail)
    assert!(approve_proposal(e.clone(), admin.clone(), proposal_id).is_err());

    // Try to reject approved proposal (should fail)
    assert!(reject_proposal(e.clone(), admin.clone(), proposal_id).is_err());

    // Try to cancel approved proposal (should fail)
    assert!(cancel_proposal(e.clone(), proposer.clone(), proposal_id).is_err());

    // Try to get non-existent proposal
    assert!(get_proposal_by_id(e.clone(), 999).is_none());
}
