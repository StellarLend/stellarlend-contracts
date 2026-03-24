use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[contract]
pub struct MockOracleLiquidation;

#[contractimpl]
impl MockOracleLiquidation {
    pub fn price(_env: Env, _asset: Address) -> i128 {
        100_000_000
    }
}

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
    pub fn balance_of(_env: Env, _user: Address) -> i128 {
        0
    }
    pub fn mint(_env: Env, _user: Address, _amount: i128) {}
}

fn setup_liquidation_test(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let liquidator = Address::generate(env);
    let borrower = Address::generate(env);

    // Register mock tokens at specific addresses
    let debt_asset = env.register(MockToken, ());
    let collateral_asset = env.register(MockToken, ());

    client.initialize(&admin, &i128::MAX, &0);

    let oracle_id = env.register(MockOracleLiquidation, ());
    client.set_oracle(&admin, &oracle_id);

    // Set 50% close factor, 10% incentive
    client.set_close_factor_bps(&admin, &5000);
    client.set_liquidation_incentive_bps(&admin, &1000);

    (
        client,
        admin,
        liquidator,
        borrower,
        debt_asset,
        collateral_asset,
    )
}

#[test]
fn test_liquidation_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) =
        setup_liquidation_test(&env);

    client.set_liquidation_threshold_bps(&admin, &8000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);

    assert_eq!(client.get_health_factor(&borrower), 12_000);

    client.set_liquidation_threshold_bps(&admin, &4000);
    assert_eq!(client.get_health_factor(&borrower), 6000);

    let (repaid, seized) = client.liquidate(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        &5_000,
    );

    assert_eq!(repaid, 5_000);
    assert_eq!(seized, 5_500);

    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 5_000);

    let collat = client.get_user_collateral(&borrower);
    assert_eq!(collat.amount, 15_000 - 5_500);
}

#[test]
fn test_liquidation_unauthorized_self() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _liquidator, borrower, debt_asset, collateral_asset) =
        setup_liquidation_test(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);

    let result = client.try_liquidate(&borrower, &borrower, &debt_asset, &collateral_asset, &5_000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_liquidation_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) =
        setup_liquidation_test(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);

    client.set_pause(&admin, &PauseType::Liquidation, &true);
    let result = client.try_liquidate(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        &5_000,
    );
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));
}

#[test]
fn test_liquidation_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) =
        setup_liquidation_test(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);

    let result = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_liquidation_asset_mismatch() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) =
        setup_liquidation_test(&env);
    let wrong_asset = env.register(MockToken, ());

    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);

    let result = client.try_liquidate(
        &liquidator,
        &borrower,
        &wrong_asset,
        &collateral_asset,
        &1_000,
    );
    assert_eq!(result, Err(Ok(BorrowError::AssetNotSupported)));

    let result = client.try_liquidate(&liquidator, &borrower, &debt_asset, &wrong_asset, &1_000);
    assert_eq!(result, Err(Ok(BorrowError::NotLiquidatable)));
}

#[test]
fn test_liquidation_exceeds_close_factor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) =
        setup_liquidation_test(&env);

    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);

    let result = client.try_liquidate(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        &6_000,
    );
    assert_eq!(result, Err(Ok(BorrowError::ExceedsCloseFactor)));
}

#[test]
fn test_liquidation_not_liquidatable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) =
        setup_liquidation_test(&env);

    client.set_liquidation_threshold_bps(&admin, &8000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &20_000);

    let result = client.try_liquidate(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        &5_000,
    );
    assert_eq!(result, Err(Ok(BorrowError::NotLiquidatable)));
}
