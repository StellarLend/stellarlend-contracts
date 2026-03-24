#![cfg(test)]

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, Map, Symbol};
use crate::deposit::{AssetParams, DepositDataKey, Position};

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn balance(_env: Env, _user: Address) -> i128 {
        10_000_000 // Always return enough balance for testing
    }
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
    pub fn transfer_from(_env: Env, _spender: Address, _from: Address, _to: Address, _amount: i128) {}
}

fn setup_test(env: &Env) -> (HelloContractClient, Address, Address, Address, Address) {
    let admin = Address::generate(env);
    let user = Address::generate(env);
    
    // Register mock token as native asset
    let native_asset = env.register(MockToken, ());
    
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    
    client.initialize(&admin);
    client.set_native_asset_address(&admin, &native_asset);
    
    // Enable the native asset for deposits
    let params = AssetParams {
        deposit_enabled: true,
        collateral_factor: 8000, // 80%
        max_deposit: 1_000_000,
        borrow_fee_bps: 100, // 1%
    };
    
    // Set asset params directly via storage
    env.as_contract(&contract_id, || {
        let key = DepositDataKey::AssetParams(native_asset.clone());
        env.storage().persistent().set(&key, &params);
    });
    
    (client, admin, user, native_asset, contract_id)
}

fn get_user_position(env: &Env, contract_id: &Address, user: &Address) -> Position {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap()
    })
}

#[test]
fn test_native_xlm_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _native_asset, contract_id) = setup_test(&env);
    
    let amount = 1000;
    let balance = client.deposit_collateral(&user, &None, &amount);
    
    assert_eq!(balance, amount);
    
    let position = get_user_position(&env, &contract_id, &user);
    assert_eq!(position.collateral, amount);
}

#[test]
fn test_native_xlm_borrow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _native_asset, contract_id) = setup_test(&env);
    
    // Deposit collateral first
    client.deposit_collateral(&user, &None, &1000);
    
    // Borrow XLM
    let borrow_amount = 500;
    let new_debt = client.borrow_asset(&user, &None, &borrow_amount);
    
    assert_eq!(new_debt, borrow_amount);
    
    let position = get_user_position(&env, &contract_id, &user);
    assert_eq!(position.debt, borrow_amount);
}

#[test]
fn test_native_xlm_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _native_asset, contract_id) = setup_test(&env);
    
    client.deposit_collateral(&user, &None, &1000);
    
    let withdraw_amount = 400;
    let remaining = client.withdraw_collateral(&user, &None, &withdraw_amount);
    
    assert_eq!(remaining, 600);
    
    let position = get_user_position(&env, &contract_id, &user);
    assert_eq!(position.collateral, 600);
}

#[test]
fn test_native_xlm_liquidation() {
    use crate::risk_params::{RiskParams, RiskParamsDataKey};

    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, _native_asset, contract_id) = setup_test(&env);
    let liquidator = Address::generate(&env);
    
    // Setup: Borrower deposits 1000 XLM, borrows 500 XLM
    // Ratio = 1000/500 * 10000 = 20000 bps (200%)
    client.deposit_collateral(&borrower, &None, &1000);
    client.borrow_asset(&borrower, &None, &500);
    
    // Override global RiskParams to set liquidation_threshold very high (500%)
    // so that the 200% ratio falls below it, making the position liquidatable.
    // The liquidate() function uses risk_params::can_be_liquidated() which
    // checks this global threshold, NOT the per-asset collateral_factor.
    env.as_contract(&contract_id, || {
        let config_key = RiskParamsDataKey::RiskParamsConfig;
        let params = RiskParams {
            min_collateral_ratio: 50_000, // 500%
            liquidation_threshold: 50_000, // 500% — position at 200% is now underwater
            close_factor: 5_000,           // 50%
            liquidation_incentive: 1_000,  // 10%
            last_update: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&config_key, &params);
    });
    
    // Liquidate 200 of the 500 debt (within 50% close factor = 250 max)
    let liquidated = client.liquidate(&liquidator, &borrower, &None, &None, &200);
    
    assert!(liquidated > 0);
    
    let position = get_user_position(&env, &contract_id, &borrower);
    assert!(position.debt < 500);
    assert!(position.collateral < 1000);
}
