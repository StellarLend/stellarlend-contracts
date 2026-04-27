#![cfg(test)]

use soroban_sdk::{Env, Address};
use super::*;

// -----------------------------------
fn setup_test(env: &Env) -> (
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let admin =
        <Address as soroban_sdk::testutils::Address>::generate(&env);
    let user1 =
        <Address as soroban_sdk::testutils::Address>::generate(&env);
    let user2 =
        <Address as soroban_sdk::testutils::Address>::generate(&env);
    let asset_usdc =
        <Address as soroban_sdk::testutils::Address>::generate(&env);
    let asset_eth =
        <Address as soroban_sdk::testutils::Address>::generate(&env);

    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(env, &contract_id);

    client.initialize(&admin, &1_000_000_i128, &1_i128);

    (client, admin, user1, user2, asset_usdc, asset_eth)
}

// -----------------------------------
#[test]
fn test_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, user1, _user2, asset_usdc, asset_eth) =
        setup_test(&env);

    // -------------------------
    // Deposit
    // -------------------------
    let deposit_amount = 500_i128;

    client.deposit(&user1, &asset_usdc, &deposit_amount);

    let deposited =
        client.get_user_collateral_deposit(&user1, &asset_usdc);

    assert_eq!(deposited.amount, deposit_amount);

    // -------------------------
    // Borrow
    // -------------------------
    let borrow_amount = 200_i128;

    client.borrow(
        &user1,
        &asset_eth,
        &borrow_amount,
        &asset_usdc,
        &deposit_amount,
    );

    let debt = client.get_user_debt(&user1);

    assert_eq!(debt.borrowed_amount, borrow_amount);

    // -------------------------
    // FINAL CHECK (no liquidation)
    // -------------------------
    let final_debt = client.get_user_debt(&user1);

    assert_eq!(final_debt.borrowed_amount, borrow_amount);
}