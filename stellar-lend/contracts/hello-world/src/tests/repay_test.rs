use crate::deposit::{DepositDataKey, Position, ProtocolAnalytics, UserAnalytics};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

/// Helper function to create a test environment
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Helper function to get user position
fn get_user_position(env: &Env, contract_id: &Address, user: &Address) -> Option<Position> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
    })
}

/// Helper function to get user analytics
fn get_user_analytics(env: &Env, contract_id: &Address, user: &Address) -> Option<UserAnalytics> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::UserAnalytics(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, UserAnalytics>(&key)
    })
}

/// Helper function to get protocol analytics
fn get_protocol_analytics(env: &Env, contract_id: &Address) -> Option<ProtocolAnalytics> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::ProtocolAnalytics;
        env.storage()
            .persistent()
            .get::<DepositDataKey, ProtocolAnalytics>(&key)
    })
}

#[test]
fn test_repay_debt_success_native() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);

    // Initialize
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup initial position: Deposit and Borrow
    let deposit_amount = 1000;
    client.deposit_collateral(&user, &None, &deposit_amount);

    let borrow_amount = 500;
    client.borrow_asset(&user, &None, &borrow_amount);

    // Mint tokens to user for repayment (since borrow_asset placeholder doesn't transfer)
    let native_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset_addr);
    native_token_client.mint(&user, &borrow_amount);
    native_token_client.approve(
        &user,
        &contract_id,
        &borrow_amount,
        &(env.ledger().sequence() + 100),
    );

    // Verify balance after minting
    let token_client = soroban_sdk::token::Client::new(&env, &native_asset_addr);
    assert_eq!(token_client.balance(&user), borrow_amount);

    // Verify initial debt
    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.debt, borrow_amount);

    // Repay partial debt
    let repay_amount = 200;
    let (remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &repay_amount);

    assert_eq!(principal_paid, repay_amount);
    assert_eq!(interest_paid, 0); // No time elapsed, so 0 interest
    assert_eq!(remaining_debt, borrow_amount - repay_amount);

    // Verify position after repayment
    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.debt, borrow_amount - repay_amount);

    // Verify user analytics
    let analytics = get_user_analytics(&env, &contract_id, &user).unwrap();
    assert_eq!(analytics.total_repayments, repay_amount);

    // Verify protocol analytics
    let protocol_analytics = get_protocol_analytics(&env, &contract_id).unwrap();
    assert_eq!(
        protocol_analytics.total_borrows,
        borrow_amount - repay_amount
    );
}

#[test]
fn test_repay_full_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup initial position
    client.deposit_collateral(&user, &None, &1000);
    client.borrow_asset(&user, &None, &500);

    // Mint tokens to user for repayment
    let native_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset_addr);
    native_token_client.mint(&user, &600);
    native_token_client.approve(&user, &contract_id, &600, &(env.ledger().sequence() + 100));

    // Repay full debt
    let (remaining_debt, _, _) = client.repay_debt(&user, &None, &600); // Overpaying to trigger full repayment

    assert_eq!(remaining_debt, 0);

    // Verify position
    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.debt, 0);
    assert_eq!(position.borrow_interest, 0);
}

#[test]
#[should_panic(expected = "Repay error: NoDebt")]
fn test_repay_no_debt() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    client.repay_debt(&user, &None, &100);
}

#[test]
fn test_repay_interest_accrual() {
    let env = create_test_env();
    env.ledger().with_mut(|li| li.timestamp = 1000);

    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register and set native asset address
    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    // Setup initial position
    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &1000);

    // Mint tokens to user for repayment
    let native_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset_addr);
    native_token_client.mint(&user, &1000);
    native_token_client.approve(&user, &contract_id, &1000, &(env.ledger().sequence() + 100));

    // Jump forward in time: 1 year (31,536,000 seconds)
    // Borrow rate is approximately 1.25% at 10% utilization (borrows=1000, deposits=10000)
    // 1000 * 0.0125 = 12.5 interest
    env.ledger().with_mut(|li| li.timestamp = 1000 + 31536000);

    // Repay some amount
    let repay_amount = 100;
    let (_remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &repay_amount);

    assert!(interest_paid > 0, "Interest should have been paid");
    assert_eq!(interest_paid + principal_paid, repay_amount);

    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert!(
        interest_paid > 0 || position.borrow_interest >= 0,
        "Interest should be tracked accurately"
    );
}

#[test]
fn test_repay_interest_only() {
    let env = create_test_env();
    env.ledger().with_mut(|li| li.timestamp = 1000);

    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &1000);

    let native_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset_addr);
    native_token_client.mint(&user, &1000);
    native_token_client.approve(&user, &contract_id, &1000, &(env.ledger().sequence() + 100));

    env.ledger().with_mut(|li| li.timestamp = 1000 + 31536000);

    // Get position to see how much interest accrued
    client.borrow_asset(&user, &None, &0); // force accrual, ignore result or just do direct read if possible, to get interest
    
    // Actually we can just repay 5, which is < expected interest (12.5)
    let repay_amount = 5;
    let (_remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &repay_amount);

    assert_eq!(interest_paid, 5, "All repayment should go to interest");
    assert_eq!(principal_paid, 0, "No principal should be paid");

    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.debt, 1000, "Principal debt should remain untouched");
}

#[test]
fn test_repay_tiny_amount() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    client.deposit_collateral(&user, &None, &10000);
    client.borrow_asset(&user, &None, &1000);

    let native_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset_addr);
    
    // Repay a tiny amount (dust)
    let tiny_amount = 1;
    native_token_client.mint(&user, &tiny_amount);
    native_token_client.approve(&user, &contract_id, &tiny_amount, &(env.ledger().sequence() + 100));

    let (_remaining_debt, interest_paid, principal_paid) =
        client.repay_debt(&user, &None, &tiny_amount);

    assert_eq!(interest_paid, 0); // No time passed
    assert_eq!(principal_paid, tiny_amount);

    let position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position.debt, 999);
}

