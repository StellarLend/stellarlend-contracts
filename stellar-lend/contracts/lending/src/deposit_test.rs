use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env,
};

fn create_token(env: &Env, admin: &Address) -> (Address, token::StellarAssetClient<'static>) {
    let token_address = env.register_stellar_asset_contract(admin.clone());
    let token_client = token::StellarAssetClient::new(env, &token_address);
    (token_address, token_client)
}

#[test]
fn test_deposit_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);

    // Setup: Mint tokens to user
    asset_client.mint(&user, &20_000);

    let balance = client.deposit(&user, &asset, &10_000);
    assert_eq!(balance, 10_000);

    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(position.amount, 10_000);

    // Verify physical transfer
    let token_client = token::Client::new(&env, &asset);
    assert_eq!(token_client.balance(&contract_id), 10_000);
    assert_eq!(token_client.balance(&user), 10_000);
}

#[test]
fn test_deposit_asset_mismatch() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset1, asset1_client) = create_token(&env, &admin);
    let (asset2, asset2_client) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);

    asset1_client.mint(&user, &10_000);
    asset2_client.mint(&user, &10_000);

    client.deposit(&user, &asset1, &5_000);
    
    // Attempting to deposit asset2 into asset1 position should fail
    let result = client.try_deposit(&user, &asset2, &5_000);
    assert_eq!(result, Err(Ok(DepositError::AssetNotSupported)));
}

#[test]
fn test_deposit_invalid_amount_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);

    let result = client.try_deposit(&user, &asset, &0);
    assert_eq!(result, Err(Ok(DepositError::InvalidAmount)));
}

#[test]
fn test_deposit_invalid_amount_negative() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);

    let result = client.try_deposit(&user, &asset, &-500);
    assert_eq!(result, Err(Ok(DepositError::InvalidAmount)));
}

#[test]
fn test_deposit_below_minimum() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &5000);

    let result = client.try_deposit(&user, &asset, &1000);
    assert_eq!(result, Err(Ok(DepositError::InvalidAmount)));
}

#[test]
fn test_deposit_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.set_deposit_paused(&true);

    let result = client.try_deposit(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(DepositError::DepositPaused)));
}

#[test]
fn test_deposit_exceeds_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, _) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&50_000, &100);

    let result = client.try_deposit(&user, &asset, &100_000);
    assert_eq!(result, Err(Ok(DepositError::ExceedsDepositCap)));
}

#[test]
fn test_deposit_multiple_times() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    asset_client.mint(&user, &20_000);

    let balance1 = client.deposit(&user, &asset, &10_000);
    assert_eq!(balance1, 10_000);

    let balance2 = client.deposit(&user, &asset, &5_000);
    assert_eq!(balance2, 15_000);

    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(position.amount, 15_000);
}

#[test]
fn test_deposit_pause_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    asset_client.mint(&user, &20_000);

    client.set_deposit_paused(&true);
    let result = client.try_deposit(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(DepositError::DepositPaused)));

    client.set_deposit_paused(&false);
    let balance = client.deposit(&user, &asset, &10_000);
    assert_eq!(balance, 10_000);
}

#[test]
fn test_deposit_overflow_protection() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);

    client.initialize(&admin, &i128::MAX, &1000);
    client.initialize_deposit_settings(&i128::MAX, &100);
    asset_client.mint(&user, &i128::MAX);

    client.deposit(&user, &asset, &1_000_000);

    let huge_amount = i128::MAX - 500_000;
    let result = client.try_deposit(&user, &asset, &huge_amount);
    assert_eq!(result, Err(Ok(DepositError::Overflow)));
}

#[test]
fn test_deposit_updates_timestamp() {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    asset_client.mint(&user, &15_000);
    client.deposit(&user, &asset, &10_000);

    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(position.last_deposit_time, 1000);

    env.ledger().with_mut(|li| {
        li.timestamp = 2000;
    });

    client.deposit(&user, &asset, &5_000);
    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(position.last_deposit_time, 2000);
    assert_eq!(position.amount, 15_000);
}

#[test]
fn test_deposit_separate_users() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);

    asset_client.mint(&user1, &10_000);
    asset_client.mint(&user2, &20_000);

    client.deposit(&user1, &asset, &10_000);
    client.deposit(&user2, &asset, &20_000);

    let pos1 = client.get_user_collateral_deposit(&user1, &asset);
    let pos2 = client.get_user_collateral_deposit(&user2, &asset);
    assert_eq!(pos1.amount, 10_000);
    assert_eq!(pos2.amount, 20_000);
}

#[test]
fn test_deposit_cap_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    let admin = Address::generate(&env);
    let (asset, asset_client) = create_token(&env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&50_000, &100);
    asset_client.mint(&user, &100_000);

    // Exact cap — should succeed
    let balance = client.deposit(&user, &asset, &50_000);
    assert_eq!(balance, 50_000);

    // Above cap — should fail
    let result = client.try_deposit(&user, &asset, &100);
    assert_eq!(result, Err(Ok(DepositError::ExceedsDepositCap)));
}
