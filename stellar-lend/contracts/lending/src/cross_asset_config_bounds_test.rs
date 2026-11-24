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
fn cross_asset_config_bounds_reject_ltv_above_threshold() {
    let (env, client, admin) = setup();
    let asset = Address::generate(&env);
    let result = client.try_set_asset_params(&admin, &asset, &8001, &8000, &1_000_000, &0, &0);
    assert!(matches!(
        result,
        Err(Ok(LendingError::InvalidLiquidationParams))
    ));
}

#[test]
fn cross_asset_config_bounds_reject_ltv_above_100_percent() {
    let (env, client, admin) = setup();
    let asset = Address::generate(&env);
    let result = client.try_set_asset_params(&admin, &asset, &10001, &10000, &1_000_000, &0, &0);
    assert!(matches!(result, Err(Ok(LendingError::InvalidAmount))));
}

#[test]
fn cross_asset_config_bounds_accept_equal_ltv_and_threshold() {
    let (env, client, admin) = setup();
    let asset = Address::generate(&env);
    client.set_asset_params(&admin, &asset, &8000, &8000, &1_000_000, &0, &0);
    let params = client.get_asset_params(&asset).unwrap();
    assert_eq!(params.ltv_bps, 8000);
    assert_eq!(params.liquidation_threshold_bps, 8000);
}
