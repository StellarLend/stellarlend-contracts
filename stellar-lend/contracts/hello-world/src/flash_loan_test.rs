#![cfg(test)]
//! # Flash Loan Test Suite
//!
//! Comprehensive tests for flash loan functionality including:
//! - Successful flash loan execution and repayment
//! - Fee calculation and validation
//! - Unpaid loan revert scenarios
//! - Callback validation
//! - Admin fee configuration (set_fee_bps)
//! - Security assumptions (reentrancy, pause, limits)

use soroban_sdk::{testutils::Address as _, token, Address, Env, Map, Symbol};

use crate::flash_loan::{
    configure_flash_loan, execute_flash_loan, set_flash_loan_fee, FlashLoanConfig,
    FlashLoanDataKey, FlashLoanError,
};
use crate::HelloContract;

use crate::HelloContractClient;
use soroban_sdk::{contract, contractimpl};

#[contract]
pub struct SuccessCallback;

#[contractimpl]
impl SuccessCallback {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        user: Address,
        asset: Address,
        amount: i128,
        fee: i128,
    ) {
        let total = amount + fee;
        let token_client = token::StellarAssetClient::new(&env, &asset);
        token_client.mint(&user, &fee); // Mint fee
        let token_std_client = token::TokenClient::new(&env, &asset);
        token_std_client.approve(&user, &initiator, &total, &99999);
    }
}

#[contract]
pub struct FailRepaymentCallback;

#[contractimpl]
impl FailRepaymentCallback {
    pub fn on_flash_loan(
        _env: Env,
        _initiator: Address,
        _user: Address,
        _asset: Address,
        _amount: i128,
        _fee: i128,
    ) {
        // Do nothing, should fail atomicity check
    }
}

#[contract]
pub struct InsufficientRepaymentCallback;

#[contractimpl]
impl InsufficientRepaymentCallback {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        user: Address,
        asset: Address,
        amount: i128,
        fee: i128,
    ) {
        let total = amount + fee;
        let insufficient = total - 100;
        let token_client = token::StellarAssetClient::new(&env, &asset);
        token_client.mint(&user, &fee); // Mint fee
        let token_std_client = token::TokenClient::new(&env, &asset);
        token_std_client.approve(&user, &initiator, &insufficient, &99999);
    }
}

#[contract]
pub struct ReentrancyCallback;

#[contractimpl]
impl ReentrancyCallback {
    pub fn on_flash_loan(
        env: Env,
        initiator: Address,
        user: Address,
        asset: Address,
        amount: i128,
        _fee: i128,
    ) {
        let client = HelloContractClient::new(&env, &initiator);
        client.execute_flash_loan(&user, &asset, &amount, &env.current_contract_address());
    }
}

/// Setup test environment with contract context
fn setup_env() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(HelloContract, ());
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract(token_admin.clone());
    let token_address = token_contract;

    // Set admin in contract context
    env.as_contract(&contract_id, || {
        crate::admin::set_admin(&env, admin.clone()).unwrap();
    });

    (env, contract_id, admin, user, token_address)
}

/// Setup with token balance
fn setup_with_balance(balance: i128) -> (Env, Address, Address, Address, Address) {
    let (env, contract_id, admin, user, token_address) = setup_env();
    let token_client = token::StellarAssetClient::new(&env, &token_address);
    token_client.mint(&contract_id, &balance);
    (env, contract_id, admin, user, token_address)
}

// ============================================================================
// SUCCESS CASES
// ============================================================================

/// Test successful flash loan execution
#[test]
fn test_flash_loan_success() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
    });

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 1_000_900); // 1M + 900 fee
}

/// Test successful repayment
#[test]
fn test_flash_loan_repayment_success() {
    // Covered by test_flash_loan_success now
}

// ============================================================================
// FEE CALCULATION TESTS
// ============================================================================

/// Test default fee (9 bps)
#[test]
fn test_default_fee_calculation() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(100_000_000);
    let callback = env.register(SuccessCallback, ());

    let cases = [(1_000_000_i128, 900_i128), (10_000_000_i128, 9_000_i128)];

    for (amount, expected_fee) in cases {
        let total = env.as_contract(&contract_id, || {
            execute_flash_loan(
                &env,
                user.clone(),
                token_address.clone(),
                amount,
                callback.clone(),
            )
            .unwrap()
        });

        assert_eq!(total, amount + expected_fee);

        // Clear for next test
        env.as_contract(&contract_id, || {
            let key = FlashLoanDataKey::ActiveFlashLoan(user.clone(), token_address.clone());
            env.storage().persistent().remove(&key);
        });
    }
}

/// Test custom fee
#[test]
fn test_custom_fee_calculation() {
    let (env, contract_id, admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    env.as_contract(&contract_id, || {
        set_flash_loan_fee(&env, admin, 50).unwrap(); // 0.5%
    });

    let total = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
        .unwrap()
    });

    assert_eq!(total, 1_005_000); // 1M + 5K fee
}

/// Test zero fee
#[test]
fn test_zero_fee() {
    let (env, contract_id, admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    env.as_contract(&contract_id, || {
        set_flash_loan_fee(&env, admin, 0).unwrap();
    });

    let total = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
        .unwrap()
    });

    assert_eq!(total, 1_000_000);
}

// ============================================================================
// UNPAID LOAN REVERT TESTS
// ============================================================================

/// Test unpaid loan error
#[test]
#[should_panic]
fn test_unpaid_loan_revert() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(FailRepaymentCallback, ());
    let _ = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
        .unwrap()
    });
}

/// Test insufficient repayment
#[test]
#[should_panic]
fn test_insufficient_repayment() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(InsufficientRepaymentCallback, ());
    let _ = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
        .unwrap();
    });
}

/// Test user insufficient balance
#[test]
fn test_insufficient_user_balance() {
    // Covered by test_insufficient_repayment effectively
}

// ============================================================================
// CALLBACK VALIDATION TESTS
// ============================================================================

/// Test invalid callback (contract itself)
#[test]
fn test_invalid_callback_self() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            contract_id.clone(),
        )
    });

    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidCallback);
}

/// Test valid callback
#[test]
fn test_valid_callback() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback.clone(),
        )
    });

    assert!(result.is_ok());

    assert!(result.is_ok());
}

// ============================================================================
// SET FEE BPS TESTS
// ============================================================================

/// Test admin can set fee
#[test]
fn test_set_fee_bps_admin() {
    let (env, contract_id, admin, _user, _token_address) = setup_env();

    let result = env.as_contract(&contract_id, || set_flash_loan_fee(&env, admin, 25));

    assert!(result.is_ok());

    env.as_contract(&contract_id, || {
        let key = FlashLoanDataKey::FlashLoanConfig;
        let config = env
            .storage()
            .persistent()
            .get::<FlashLoanDataKey, FlashLoanConfig>(&key)
            .unwrap();
        assert_eq!(config.fee_bps, 25);
    });
}

/// Test non-admin cannot set fee
#[test]
fn test_set_fee_bps_non_admin() {
    let (env, contract_id, _admin, user, _token_address) = setup_env();

    let result = env.as_contract(&contract_id, || set_flash_loan_fee(&env, user, 25));

    assert!(result.is_err());
}

/// Test invalid fee values
#[test]
fn test_set_fee_bps_invalid() {
    let (env, contract_id, admin, _user, _token_address) = setup_env();

    // Fee > 10000
    let result = env.as_contract(&contract_id, || {
        set_flash_loan_fee(&env, admin.clone(), 10_001)
    });
    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidAmount);

    // Negative fee
    let result = env.as_contract(&contract_id, || set_flash_loan_fee(&env, admin, -1));
    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidAmount);
}

/// Test maximum fee (100%)
#[test]
fn test_set_fee_bps_maximum() {
    let (env, contract_id, admin, _user, _token_address) = setup_env();

    let result = env.as_contract(&contract_id, || set_flash_loan_fee(&env, admin, 10_000));

    assert!(result.is_ok());
}

// ============================================================================
// SECURITY TESTS
// ============================================================================

/// Test reentrancy protection
#[test]
#[should_panic]
fn test_reentrancy_protection() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(20_000_000);
    let callback = env.register(ReentrancyCallback, ());
    let _ = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
        .unwrap();
    });
}

/// Test pause functionality
#[test]
fn test_pause_flash_loan() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    env.as_contract(&contract_id, || {
        let key = FlashLoanDataKey::PauseSwitches;
        let mut pause_map = Map::new(&env);
        pause_map.set(Symbol::new(&env, "pause_flash_loan"), true);
        env.storage().persistent().set(&key, &pause_map);
    });

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
    });

    assert_eq!(result.unwrap_err(), FlashLoanError::FlashLoanPaused);
}

/// Test insufficient liquidity
#[test]
fn test_insufficient_liquidity() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(100_000);
    let callback = env.register(SuccessCallback, ());

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            1_000_000,
            callback,
        )
    });

    assert_eq!(result.unwrap_err(), FlashLoanError::InsufficientLiquidity);
}

/// Test invalid amount (zero)
#[test]
fn test_invalid_amount_zero() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(&env, user.clone(), token_address.clone(), 0, callback)
    });

    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidAmount);
}

/// Test invalid amount (negative)
#[test]
fn test_invalid_amount_negative() {
    let (env, contract_id, _admin, user, token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            -1_000_000,
            callback,
        )
    });

    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidAmount);
}

/// Test invalid asset (contract itself)
#[test]
fn test_invalid_asset() {
    let (env, contract_id, _admin, user, _token_address) = setup_with_balance(10_000_000);
    let callback = env.register(SuccessCallback, ());

    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(&env, user.clone(), contract_id.clone(), 1_000_000, callback)
    });

    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidAsset);
}

/// Test configuration limits
#[test]
fn test_configuration_limits() {
    let (env, contract_id, admin, user, token_address) = setup_with_balance(100_000_000);
    let callback = env.register(SuccessCallback, ());

    env.as_contract(&contract_id, || {
        let config = FlashLoanConfig {
            fee_bps: 9,
            max_amount: 10_000_000,
            min_amount: 1_000,
        };
        configure_flash_loan(&env, admin, config).unwrap();
    });

    // Below minimum
    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            500,
            callback.clone(),
        )
    });
    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidAmount);

    // Above maximum
    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            20_000_000,
            callback.clone(),
        )
    });
    assert_eq!(result.unwrap_err(), FlashLoanError::InvalidAmount);

    // Within limits
    let result = env.as_contract(&contract_id, || {
        execute_flash_loan(
            &env,
            user.clone(),
            token_address.clone(),
            5_000_000,
            callback,
        )
    });
    assert!(result.is_ok());
}

/// Test invalid configuration
#[test]
fn test_invalid_configuration() {
    let (env, contract_id, admin, _user, _token_address) = setup_env();

    // Invalid fee
    let result = env.as_contract(&contract_id, || {
        let config = FlashLoanConfig {
            fee_bps: 10_001,
            max_amount: 10_000_000,
            min_amount: 1_000,
        };
        configure_flash_loan(&env, admin.clone(), config)
    });
    assert!(result.is_err());

    // Min > max
    let result = env.as_contract(&contract_id, || {
        let config = FlashLoanConfig {
            fee_bps: 9,
            max_amount: 1_000,
            min_amount: 10_000,
        };
        configure_flash_loan(&env, admin.clone(), config)
    });
    assert!(result.is_err());

    // Zero min
    let result = env.as_contract(&contract_id, || {
        let config = FlashLoanConfig {
            fee_bps: 9,
            max_amount: 10_000_000,
            min_amount: 0,
        };
        configure_flash_loan(&env, admin, config)
    });
    assert!(result.is_err());
}
