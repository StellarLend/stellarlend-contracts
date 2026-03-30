use crate::{borrow::BorrowError, LendingContract, LendingContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env, IntoVal, Symbol, Val, Vec};

fn create_token(env: &Env, admin: &Address) -> (Address, token::StellarAssetClient<'static>) {
    let token_address = env.register_stellar_asset_contract(admin.clone());
    let token_client = token::StellarAssetClient::new(env, &token_address);
    (token_address, token_client)
}

#[test]
fn test_receive_deposit_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let from = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);

    let payload = (Symbol::new(&env, "deposit"),).into_val(&env);

    // Simulate token contract calling receive
    client.receive(&asset, &from, &50_000, &payload);

    let collateral = client.get_user_collateral(&from);
    assert_eq!(collateral.amount, 50_000);
    assert_eq!(collateral.asset, asset);
}

#[test]
fn test_receive_paused_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let from = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_deposit_paused(&true);

    let payload = (Symbol::new(&env, "deposit"),).into_val(&env);

    let result = client.try_receive(&asset, &from, &50_000, &payload);
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));
}

#[test]
fn test_receive_repay_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let from = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);
    let (collateral_asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);

    // Initial borrow to create debt
    client.borrow(&from, &asset, &10_000, &collateral_asset, &20_000);

    let payload = (Symbol::new(&env, "repay"),).into_val(&env);

    // Simulate token contract calling receive for repayment
    client.receive(&asset, &from, &5_000, &payload);

    let debt = client.get_user_debt(&from);
    assert_eq!(debt.borrowed_amount, 5_000);
}

#[test]
fn test_receive_invalid_action() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let from = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);

    let payload = (Symbol::new(&env, "withdraw"),).into_val(&env);

    let result = client.try_receive(&asset, &from, &50_000, &payload);
    assert_eq!(result, Err(Ok(BorrowError::AssetNotSupported)));
}

#[test]
fn test_receive_empty_payload() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let from = Address::generate(&env);
    let asset = Address::generate(&env);
    let payload = Vec::new(&env);
    let result = client.try_receive(&asset, &from, &50_000, &payload);
    assert!(result.is_err());
}

#[test]
fn test_direct_deposit_repay() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);
    let (borrow_asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    asset_client.mint(&user, &20_000);

    // Test direct deposit
    client.deposit_collateral(&user, &asset, &10_000);
    assert_eq!(client.get_user_collateral(&user).amount, 10_000);

    // Initial borrow
    client.borrow(&user, &borrow_asset, &5_000, &asset, &10_000);

    // Test direct repay
    client.repay(&user, &borrow_asset, &2_000);
    assert_eq!(client.get_user_debt(&user).borrowed_amount, 3_000);
}
