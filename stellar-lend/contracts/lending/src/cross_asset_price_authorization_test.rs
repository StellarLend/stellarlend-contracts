use super::*;
use soroban_sdk::{testutils::Address as _, BytesN};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn cross_asset_price_authorization_rejects_non_admin_before_validation() {
    let (env, client, _admin) = setup();
    let caller = Address::generate(&env);
    let asset = Address::generate(&env);
    let signature = BytesN::<64>::from_array(&env, &[0; 64]);

    let result = client.try_set_price(&caller, &asset, &0, &env.ledger().timestamp(), &signature);
    assert!(matches!(result, Err(Ok(LendingError::Unauthorized))));
}

#[test]
fn cross_asset_price_authorization_rejects_zero_and_negative_prices() {
    let (env, client, admin) = setup();
    let asset = Address::generate(&env);
    let signature = BytesN::<64>::from_array(&env, &[0; 64]);

    let zero = client.try_set_price(&admin, &asset, &0, &env.ledger().timestamp(), &signature);
    assert!(matches!(zero, Err(Ok(LendingError::InvalidAmount))));

    let negative = client.try_set_price(&admin, &asset, &-1, &env.ledger().timestamp(), &signature);
    assert!(matches!(negative, Err(Ok(LendingError::InvalidAmount))));
}
