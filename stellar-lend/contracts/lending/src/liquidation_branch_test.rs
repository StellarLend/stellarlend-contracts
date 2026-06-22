use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::debt::{load_debt, save_debt, DebtPosition};
use crate::{DataKey, LendingContract, LendingContractClient, LendingError};

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    (env, client, contract_id, borrower, liquidator)
}

fn set_position(
    env: &Env,
    contract_id: &Address,
    borrower: &Address,
    collateral: i128,
    debt: i128,
) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &collateral);
        save_debt(
            env,
            borrower,
            &DebtPosition {
                principal: debt,
                last_update: env.ledger().timestamp(),
            },
        );
    });
}

fn collateral(env: &Env, contract_id: &Address, borrower: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::Collateral(borrower.clone()))
            .unwrap_or(0)
    })
}

fn principal(env: &Env, contract_id: &Address, borrower: &Address) -> i128 {
    env.as_contract(contract_id, || load_debt(env, borrower).principal)
}

fn assert_contract_error<T: core::fmt::Debug>(
    result: Result<T, Result<LendingError, soroban_sdk::InvokeError>>,
    expected: LendingError,
) {
    match result {
        Err(Ok(actual)) => assert_eq!(actual, expected),
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn liquidate_caps_repay_at_close_factor_without_seizure_clamp() {
    let (env, client, contract_id, borrower, liquidator) = setup();
    set_position(&env, &contract_id, &borrower, 600, 1_000);

    let repaid = client.liquidate(&liquidator, &borrower, &900);

    assert_eq!(repaid, 500);
    assert_eq!(principal(&env, &contract_id, &borrower), 500);
    assert_eq!(collateral(&env, &contract_id, &borrower), 50);
}

#[test]
fn liquidate_clamps_seizure_to_available_collateral() {
    let (env, client, contract_id, borrower, liquidator) = setup();
    set_position(&env, &contract_id, &borrower, 100, 1_000);

    let repaid = client.liquidate(&liquidator, &borrower, &100);

    assert_eq!(repaid, 100);
    assert_eq!(principal(&env, &contract_id, &borrower), 900);
    assert_eq!(collateral(&env, &contract_id, &borrower), 0);
}

#[test]
fn liquidate_allows_repeated_partial_liquidations() {
    let (env, client, contract_id, borrower, liquidator) = setup();
    set_position(&env, &contract_id, &borrower, 700, 1_000);

    let first_repay = client.liquidate(&liquidator, &borrower, &200);
    assert_eq!(first_repay, 200);
    assert_eq!(principal(&env, &contract_id, &borrower), 800);
    assert_eq!(collateral(&env, &contract_id, &borrower), 480);

    let second_repay = client.liquidate(&liquidator, &borrower, &300);
    assert_eq!(second_repay, 300);
    assert_eq!(principal(&env, &contract_id, &borrower), 500);
    assert_eq!(collateral(&env, &contract_id, &borrower), 150);
}

#[test]
fn liquidate_rejects_zero_debt_position() {
    let (env, client, contract_id, borrower, liquidator) = setup();
    set_position(&env, &contract_id, &borrower, 500, 0);

    let result = client.try_liquidate(&liquidator, &borrower, &100);

    assert_contract_error(result, LendingError::PositionHealthy);
}

#[test]
fn liquidate_rejects_healthy_position() {
    let (env, client, contract_id, borrower, liquidator) = setup();
    set_position(&env, &contract_id, &borrower, 2_000, 1_000);

    let result = client.try_liquidate(&liquidator, &borrower, &100);

    assert_contract_error(result, LendingError::PositionHealthy);
}
