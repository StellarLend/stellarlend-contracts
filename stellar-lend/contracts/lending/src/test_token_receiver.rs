use crate::{DataKey, LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, Symbol, Val, Vec,
};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address, TokenClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let asset = env.register_stellar_asset_contract(token_admin);
    client.set_collateral_asset(&asset);

    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &asset).mint(&user, &1_000);
    let token = TokenClient::new(&env, &asset);
    token.approve(&user, &contract_id, &1_000, &1_000);

    (env, client, asset, user, token)
}

fn payload(env: &Env, action: Symbol) -> Vec<Val> {
    let mut payload = Vec::new(env);
    payload.push_back(1u32.into_val(env));
    payload.push_back(action.into_val(env));
    payload
}

fn total_deposits(env: &Env, contract_id: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0)
    })
}

fn set_deposit_cap(env: &Env, contract_id: &Address, cap: i128) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });
}

#[test]
fn receive_deposit_transfers_tokens_and_updates_accounting() {
    let (env, client, asset, user, token) = setup();
    let action = payload(&env, symbol_short!("deposit"));

    client.receive(&asset, &user, &250, &action);

    assert_eq!(token.balance(&user), 750);
    assert_eq!(token.balance(&client.address), 250);
    assert_eq!(total_deposits(&env, &client.address), 250);
}

#[test]
fn unsupported_action_is_rejected_before_token_pull() {
    let (env, client, asset, user, token) = setup();
    let action = payload(&env, symbol_short!("withdraw"));

    assert!(matches!(
        client.try_receive(&asset, &user, &250, &action),
        Err(Ok(LendingError::AssetNotSupported))
    ));
    assert_eq!(token.balance(&user), 1_000);
    assert_eq!(token.balance(&client.address), 0);
    assert_eq!(token.allowance(&user, &client.address), 1_000);
    assert_eq!(total_deposits(&env, &client.address), 0);
}

#[test]
fn malformed_and_unsupported_version_payloads_do_not_pull_tokens() {
    let (env, client, asset, user, token) = setup();
    let empty_payload = Vec::new(&env);
    assert!(matches!(
        client.try_receive(&asset, &user, &250, &empty_payload),
        Err(Ok(LendingError::MalformedPayload))
    ));

    let mut wrong_version = Vec::new(&env);
    wrong_version.push_back(2u32.into_val(&env));
    wrong_version.push_back(symbol_short!("deposit").into_val(&env));
    assert!(matches!(
        client.try_receive(&asset, &user, &250, &wrong_version),
        Err(Ok(LendingError::InvalidPayloadVersion))
    ));

    assert_eq!(token.balance(&user), 1_000);
    assert_eq!(token.balance(&client.address), 0);
    assert_eq!(total_deposits(&env, &client.address), 0);
}

#[test]
fn sender_and_asset_guards_reject_before_token_pull() {
    let (env, client, asset, user, token) = setup();
    let action = payload(&env, symbol_short!("deposit"));
    let other_asset = Address::generate(&env);

    assert!(matches!(
        client.try_receive(&other_asset, &user, &250, &action),
        Err(Ok(LendingError::AssetNotSupported))
    ));
    assert!(matches!(
        client.try_receive(&asset, &asset, &250, &action),
        Err(Ok(LendingError::UnauthorizedSender))
    ));
    assert!(matches!(
        client.try_receive(&asset, &client.address, &250, &action),
        Err(Ok(LendingError::UnauthorizedSender))
    ));

    assert_eq!(token.balance(&user), 1_000);
    assert_eq!(token.balance(&client.address), 0);
    assert_eq!(total_deposits(&env, &client.address), 0);
}

#[test]
fn failed_deposit_rolls_back_token_pull_and_can_be_retried() {
    let (env, client, asset, user, token) = setup();
    let action = payload(&env, symbol_short!("deposit"));
    set_deposit_cap(&env, &client.address, 100);

    assert!(matches!(
        client.try_receive(&asset, &user, &250, &action),
        Err(Ok(LendingError::DepositCapExceeded))
    ));
    assert_eq!(token.balance(&user), 1_000);
    assert_eq!(token.balance(&client.address), 0);
    assert_eq!(token.allowance(&user, &client.address), 1_000);
    assert_eq!(total_deposits(&env, &client.address), 0);

    set_deposit_cap(&env, &client.address, 250);
    client.receive(&asset, &user, &250, &action);
    assert_eq!(token.balance(&user), 750);
    assert_eq!(token.balance(&client.address), 250);
    assert_eq!(token.allowance(&user, &client.address), 750);
    assert_eq!(total_deposits(&env, &client.address), 250);
}

#[test]
fn zero_amount_is_rejected_without_changing_balances_or_allowance() {
    let (env, client, asset, user, token) = setup();
    let action = payload(&env, symbol_short!("deposit"));

    assert!(matches!(
        client.try_receive(&asset, &user, &0, &action),
        Err(Ok(LendingError::InvalidAmount))
    ));
    assert_eq!(token.balance(&user), 1_000);
    assert_eq!(token.balance(&client.address), 0);
    assert_eq!(token.allowance(&user, &client.address), 1_000);
    assert_eq!(total_deposits(&env, &client.address), 0);
}