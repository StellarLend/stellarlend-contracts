#![cfg(test)]

use crate::cross_asset::{AssetParams, CrossAssetError};
use crate::oracle::OracleConfig;
use crate::{LendingContract, LendingContractClient};
use soroban_sdk::{
    testutils::Address as _,
    token, Address, Env,
};

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    token::StellarAssetClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_admin(&admin);

    let oracle = Address::generate(&env);
    client.set_oracle(&admin, &oracle);
    client.configure_oracle(&admin, &OracleConfig { max_staleness_seconds: 3600 });

    let token_admin = Address::generate(&env);
    let token_asset = env.register_stellar_asset_contract(token_admin.clone());
    let token_client = token::StellarAssetClient::new(&env, &token_asset);
    
    // We also mint some initially so that mock transfer doesn't crash if balances are fully checked in the token contract
    let initial_user = Address::generate(&env);
    token_client.mint(&initial_user, &100_000_000);
    token_client.mint(&contract_id, &100_000_000); // give the contract money for withdraws/borrows that actually try to transfer!

    (env, client, admin, token_asset, token_client)
}

fn generate_params(oracle: Address) -> AssetParams {
    AssetParams {
        ltv: 7500, // 75%
        liquidation_threshold: 8000, // 80%
        price_feed: oracle,
        debt_ceiling: 1_000_000_0000000,
        is_active: true,
    }
}

#[test]
fn test_cross_asset_deposit_borrow() {
    let (env, client, admin, asset, token) = setup();

    let user = Address::generate(&env);
    token.mint(&user, &100_000);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000); // 1.00 USD

    client.set_asset_params(&admin, &asset, &generate_params(oracle.clone()));

    // Deposit tests
    client.deposit_collateral_asset(&user, &asset, &10_000);

    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_collateral_usd, 10_000);
    assert_eq!(summary.total_debt_usd, 0);

    // Borrow tests
    client.borrow_asset(&user, &asset, &5_000);
    let summary2 = client.get_cross_position_summary(&user);
    assert_eq!(summary2.total_collateral_usd, 10_000);
    assert_eq!(summary2.total_debt_usd, 5_000);
    
    // Check health factor: 10_000 * 0.75 = 7500 borrow power
    // debt = 5000 -> 7500 / 5000 * 10000 = 15000
    assert_eq!(summary2.health_factor, 15000);
}

#[test]
fn test_cross_asset_repay_withdraw() {
    let (env, client, admin, asset, token) = setup();

    let user = Address::generate(&env);
    token.mint(&user, &100_000);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000); // 1.00 USD

    client.set_asset_params(&admin, &asset, &generate_params(oracle.clone()));

    client.deposit_collateral_asset(&user, &asset, &20_000);
    client.borrow_asset(&user, &asset, &10_000);

    // Repay checks
    client.repay_asset(&user, &asset, &2_000);
    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_debt_usd, 8_000);

    // Withdraw checks
    client.withdraw_asset(&user, &asset, &5_000);
    let summary2 = client.get_cross_position_summary(&user);
    assert_eq!(summary2.total_collateral_usd, 15_000);
    
    // Remaining debt 8000. Need 8000 / 0.75 = 10666 collateral min.
    // We withdrew 5000 leaving 15000. So we are good.
}

#[test]
fn test_borrow_fails_if_insufficient_collateral() {
    let (env, client, admin, asset, token) = setup();

    let user = Address::generate(&env);
    token.mint(&user, &100_000);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000);

    client.set_asset_params(&admin, &asset, &generate_params(oracle.clone()));
    client.deposit_collateral_asset(&user, &asset, &10_000);
    
    // Max borrow is 7500. So 8000 should fail!
    let res = client.try_borrow_asset(&user, &asset, &8_000);
    assert_eq!(res, Err(Ok(CrossAssetError::InsufficientCollateral)));
}

#[test]
fn test_withdraw_fails_if_insufficient_health() {
    let (env, client, admin, asset, token) = setup();

    let user = Address::generate(&env);
    token.mint(&user, &100_000);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000);

    client.set_asset_params(&admin, &asset, &generate_params(oracle.clone()));
    client.deposit_collateral_asset(&user, &asset, &20_000);
    client.borrow_asset(&user, &asset, &15_000); // 15000 is perfectly 75% of 20000

    // Any withdraw should fail!
    let res = client.try_withdraw_asset(&user, &asset, &1);
    assert_eq!(res, Err(Ok(CrossAssetError::InsufficientCollateral)));
}

#[test]
fn test_borrow_fails_if_exceeds_debt_ceiling() {
    let (env, client, admin, asset, token) = setup();

    let user = Address::generate(&env);
    token.mint(&user, &100_000);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000);

    let mut params = generate_params(oracle.clone());
    params.debt_ceiling = 4_000;
    client.set_asset_params(&admin, &asset, &params);
    
    client.deposit_collateral_asset(&user, &asset, &10_000);
    
    // Valid LTV (7500 max), but debt ceiling is 4000.
    let res = client.try_borrow_asset(&user, &asset, &5_000);
    assert_eq!(res, Err(Ok(CrossAssetError::DebtCeilingReached)));
}

#[test]
fn test_deposit_zero_fails() {
    let (env, client, admin, asset, _token) = setup();
    let user = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000);
    client.set_asset_params(&admin, &asset, &generate_params(oracle.clone()));

    let res = client.try_deposit_collateral_asset(&user, &asset, &0);
    assert_eq!(res, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_deposit_paused() {
    let (env, client, admin, asset, _) = setup();
    let user = Address::generate(&env);
    let oracle = Address::generate(&env);
    client.set_primary_oracle(&admin, &asset, &oracle);
    client.update_price_feed(&oracle, &asset, &100_000_000);
    client.set_asset_params(&admin, &asset, &generate_params(oracle.clone()));

    client.set_deposit_paused(&true);
    let res = client.try_deposit_collateral_asset(&user, &asset, &10_000);
    assert_eq!(res, Err(Ok(CrossAssetError::ProtocolPaused)));
}