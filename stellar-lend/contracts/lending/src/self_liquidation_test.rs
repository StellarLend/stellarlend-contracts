#![cfg(test)]

use crate::{debt::DebtPosition, DataKey, LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = borrower.clone();
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.initialize(&admin);
    (
        env,
        client,
        borrower,
        liquidator,
        admin,
        debt_asset,
        collateral_asset,
    )
}

#[test]
fn self_liquidation_is_rejected_before_any_state_change() {
    let (env, client, borrower, liquidator, _admin, debt_asset, collateral_asset) = setup();

    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &100i128);
        env.storage().persistent().set(
            &DataKey::Debt(borrower.clone()),
            &DebtPosition {
                principal: 200,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    let before_collateral = client.get_position(&borrower).collateral;
    let before_debt = client.get_position(&borrower).debt;

    let res = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);

    assert!(matches!(res, Err(Ok(LendingError::SelfLiquidation))));
    assert_eq!(client.get_position(&borrower).collateral, before_collateral);
    assert_eq!(client.get_position(&borrower).debt, before_debt);

    let other_liquidator = Address::generate(&env);
    let success = client.try_liquidate(
        &other_liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        &100,
    );
    assert!(
        success.is_ok(),
        "distinct-address liquidation should still succeed"
    );
}

#[test]
fn unhealthy_self_position_is_rejected_even_when_position_is_underwater() {
    let (env, client, borrower, liquidator, _admin, debt_asset, collateral_asset) = setup();

    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &100i128);
        env.storage().persistent().set(
            &DataKey::Debt(borrower.clone()),
            &DebtPosition {
                principal: 200,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    let res = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);

    assert!(matches!(res, Err(Ok(LendingError::SelfLiquidation))));
}
