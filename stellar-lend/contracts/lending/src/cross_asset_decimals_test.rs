use super::*;
use soroban_sdk::testutils::Address as _;

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
fn cross_asset_decimals_normalizes_same_scale() {
    let price = 1_000_000_000_000_000_000i128;
    assert_eq!(cross_asset::value_in_base(1, price, 18), Ok(1));
}

#[test]
fn cross_asset_decimals_normalizes_7_decimals() {
    assert_eq!(cross_asset::value_in_base(1, 10_000_000, 7), Ok(1));
}

#[test]
fn cross_asset_decimals_rejects_invalid_v2_decimals() {
    let (_env, client, admin) = setup();
    let asset = Address::generate(&_env);

    let result =
        client.try_set_asset_params_v2(&admin, &asset, &7500, &8000, &1_000_000, &0, &0, &19);
    assert!(matches!(result, Err(Ok(LendingError::InvalidAmount))));

    let result =
        client.try_set_asset_params_v2(&admin, &asset, &7500, &8000, &1_000_000, &0, &0, &0);
    assert!(matches!(result, Err(Ok(LendingError::InvalidAmount))));
}

#[test]
fn cross_asset_decimals_accepts_supported_v2_decimals() {
    let (_env, client, admin) = setup();
    let asset = Address::generate(&_env);

    client.set_asset_params_v2(&admin, &asset, &7500, &8000, &1_000_000, &0, &0, &6);

    let params = client.get_asset_params(&asset).unwrap();
    assert_eq!(params.ltv_bps, 7500);
}
