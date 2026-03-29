#![cfg(test)]

use crate::cross_asset::{AssetConfig, AssetKey};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Map};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_protocol<'a>(env: &'a Env, admin: &'a Address) -> HelloContractClient<'a> {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    client.initialize_ca(admin);
    client
}

fn create_asset_config(
    env: &Env,
    asset: Option<Address>,
    price: i128,
    max_borrow: i128,
) -> AssetConfig {
    AssetConfig {
        asset: asset.clone(),
        collateral_factor: 8000,
        liquidation_threshold: 8000,
        reserve_factor: 1000,
        max_supply: 10_000_000_000,
        max_borrow,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_updated_at: env.ledger().timestamp(),
    }
}

#[test]
fn test_borrow_cap_enforcement() {
    let env = create_test_env();
    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let usdc = Address::generate(&env);

    let client = setup_protocol(&env, &admin);

    // Initialize USDC with a 1000 unit borrow cap
    let cap = 1000;
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000, cap);
    client.initialize_asset(&Some(usdc.clone()), &config);

    // User 1 deposits collateral (Native XLM)
    client.ca_deposit_collateral(&user1, &None, &5000);

    // User 1 borrows 600 USDC (Within cap)
    client.cross_asset_borrow(&user1, &Some(usdc.clone()), &600);

    // User 2 deposits collateral
    client.ca_deposit_collateral(&user2, &None, &5000);

    // User 2 tries to borrow 500 USDC (Would exceed cap: 600 + 500 = 1100 > 1000)
    let result = client.try_cross_asset_borrow(&user2, &Some(usdc.clone()), &500);

    assert!(result.is_err());
    // Error(Contract, #109) corresponds to CrossAssetError::BorrowCapExceeded
    // depending on the enum index, which I confirmed in cross_asset.rs
}

#[test]
fn test_borrow_cap_update_via_admin() {
    let env = create_test_env();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let usdc = Address::generate(&env);

    let client = setup_protocol(&env, &admin);

    // Initialize with small cap
    let config = create_asset_config(&env, Some(usdc.clone()), 1_0000000, 500);
    client.initialize_asset(&Some(usdc.clone()), &config);

    client.ca_deposit_collateral(&user, &None, &5000);

    // Borrow fails at 600
    assert!(client
        .try_cross_asset_borrow(&user, &Some(usdc.clone()), &600)
        .is_err());

    // Update cap to 1000
    client.update_asset_config(
        &Some(usdc.clone()),
        &None,       // cf
        &None,       // lt
        &None,       // max_supply
        &Some(1000), // max_borrow
        &None,       // can_collateralize
        &None,       // can_borrow
    );

    // Now borrow 600 works
    let result = client.try_cross_asset_borrow(&user, &Some(usdc.clone()), &600);
    assert!(result.is_ok());
}
