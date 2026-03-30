//! # Intra-Ledger-Block Operation Ordering and Race Tests
//!
//! These tests simulate sequences of operations within a single ledger context.

use crate::*;
use crate::testutils::create_token;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

fn setup_race_test(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address, // admin
    Address, // user
    Address, // asset
    Address, // collateral_asset
    token::StellarAssetClient<'_>, // asset_client
    token::StellarAssetClient<'_>, // collateral_client
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let user = Address::generate(env);
    let (asset, asset_client) = create_token(env, &admin);
    let (collateral_asset, collateral_client) = create_token(env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    (client, admin, user, asset, collateral_asset, asset_client, collateral_client)
}

#[test]
fn test_intra_block_deposit_withdraw_same_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _collateral_asset, asset_client, _) = setup_race_test(&env);

    asset_client.mint(&user, &10_000);
    // Sequence: Deposit 10,000 then Withdraw 10,000 in same ledger
    client.deposit(&user, &asset, &10_000);
    client.withdraw(&user, &asset, &10_000);

    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(position.amount, 0);
}

#[test]
fn test_intra_block_borrow_repay() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, asset_client, collateral_client) = setup_race_test(&env);

    // Initial deposit for collateral
    collateral_client.mint(&user, &50_000);
    client.deposit(&user, &collateral_asset, &50_000);

    // Sequence: Borrow 10,000 then Repay 5,000
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    asset_client.mint(&user, &5_000);
    client.repay(&user, &asset, &5_000);

    let debt = client.get_user_debt(&user);
    assert_eq!(debt.borrowed_amount, 5_000);
}

#[test]
fn test_intra_block_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, asset_client, collateral_client) = setup_race_test(&env);

    collateral_client.mint(&user, &100_000);
    client.deposit(&user, &collateral_asset, &100_000);
    client.borrow(&user, &asset, &20_000, &collateral_asset, &40_000);
    asset_client.mint(&user, &20_000);
    client.repay(&user, &asset, &20_000);
    client.withdraw(&user, &collateral_asset, &50_000);

    let pos_dep = client.get_user_collateral_deposit(&user, &collateral_asset);
    assert_eq!(pos_dep.amount, 50_000);

    let debt = client.get_user_debt(&user);
    assert_eq!(debt.borrowed_amount, 0);
}

#[test]
fn test_intra_block_multi_user_interaction() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user1, asset, _collateral_asset, asset_client, _) = setup_race_test(&env);
    let user2 = Address::generate(&env);

    asset_client.mint(&user1, &10_000);
    client.deposit(&user1, &asset, &10_000);
    asset_client.mint(&user2, &20_000);
    client.deposit(&user2, &asset, &20_000);
    client.withdraw(&user1, &asset, &5_000);
    client.withdraw(&user2, &asset, &10_000);

    let pos1 = client.get_user_collateral_deposit(&user1, &asset);
    let pos2 = client.get_user_collateral_deposit(&user2, &asset);

    assert_eq!(pos1.amount, 5_000);
    assert_eq!(pos2.amount, 10_000);
}

#[test]
fn test_intra_block_invalid_ordering_withdraw_first() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _collateral_asset, asset_client, _) = setup_race_test(&env);

    asset_client.mint(&user, &10_000);
    client.deposit(&user, &asset, &10_000);

    let result = client.try_withdraw(&user, &asset, &15_000);
    assert!(result.is_err());

    asset_client.mint(&user, &10_000);
    client.deposit(&user, &asset, &10_000);

    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(pos.amount, 20_000);
}

#[test]
fn test_intra_block_excessive_borrow_repay_race() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, asset_client, collateral_client) = setup_race_test(&env);

    collateral_client.mint(&user, &1_000_000);
    client.deposit(&user, &collateral_asset, &1_000_000);

    for i in 1..=5 {
        client.borrow(&user, &asset, &(i * 1000), &collateral_asset, &(i * 2000));
        asset_client.mint(&user, &(i * 500));
        client.repay(&user, &asset, &(i * 500));
    }

    let debt = client.get_user_debt(&user);
    assert_eq!(debt.borrowed_amount, 7_500);
}
