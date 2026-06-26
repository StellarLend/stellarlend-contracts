#![cfg(test)]

use crate::{LendingContract, LendingContractClient};
use soroban_sdk::{testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation}, symbol_short, token, Address, Env};

#[test]
fn test_withdraw_reserve_success() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let asset = env.register_stellar_asset_contract(admin.clone());
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    let token_admin_client = token::StellarAssetClient::new(&env, &asset);
    token_admin_client.mint(&contract_id, &10_000);

    // Mock reserve state directly to test withdrawal logic bounds
    let reserve_key = symbol_short!("reserve");
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&reserve_key, &2_000_i128);
    });

    // Partial drain
    client.withdraw_reserve(&admin, &asset, &treasury, &500);
    let token_client = token::Client::new(&env, &asset);
    assert_eq!(token_client.balance(&treasury), 500);

    // Full drain ensures depositor principal (8000) is isolated
    client.withdraw_reserve(&admin, &asset, &treasury, &1_500);
    assert_eq!(token_client.balance(&treasury), 2_000);
    assert_eq!(token_client.balance(&contract_id), 8_000); 
}

#[test]
#[should_panic(expected = "Insufficient accrued reserve to withdraw")]
fn test_withdraw_reserve_over_withdraw() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let asset = env.register_stellar_asset_contract(admin.clone());
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let token_admin_client = token::StellarAssetClient::new(&env, &asset);
    token_admin_client.mint(&contract_id, &10_000);

    let reserve_key = symbol_short!("reserve");
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&reserve_key, &1_000_i128);
    });

    client.withdraw_reserve(&admin, &asset, &treasury, &1_001);
}

#[test]
#[should_panic(expected = "Amount must be greater than zero")]
fn test_withdraw_reserve_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let asset = env.register_stellar_asset_contract(admin.clone());
    
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    client.withdraw_reserve(&admin, &asset, &treasury, &0);
}
