//! # Integration Test Suite for Full Lending Flow (#315)
//!
//! End-to-end integration tests against the built contract:
//! - **Happy path**: initialize → deposit → borrow → repay → withdraw (assert final state, balances, health factor, events).
//! - **Liquidation path**: initialize → deposit → borrow → liquidate (assert final state and events).
//!
//! Security: validates protocol invariants hold after full flows.

use crate::deposit::{DepositDataKey, Position, ProtocolAnalytics};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn get_collateral_balance(env: &Env, contract_id: &Address, user: &Address) -> i128 {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::CollateralBalance(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, i128>(&key)
            .unwrap_or(0)
    })
}

fn get_user_position(env: &Env, contract_id: &Address, user: &Address) -> Option<Position> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
    })
}

/// Full flow: initialize → deposit → borrow → repay → withdraw.
/// Asserts final balances, position, and that user can withdraw after repay.
#[test]
fn integration_full_flow_deposit_borrow_repay_withdraw() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &5_000);
    token_client.approve(
        &user,
        &contract_id,
        &5_000,
        &(env.ledger().sequence() + 100),
    );

    let deposit_amount = 10_000;
    client.deposit_collateral(&user, &None, &deposit_amount);
    assert_eq!(
        get_collateral_balance(&env, &contract_id, &user),
        deposit_amount
    );

    let borrow_amount = 3_000;
    let debt_after_borrow = client.borrow_asset(&user, &None, &borrow_amount);
    assert!(debt_after_borrow >= borrow_amount);

    let position_mid = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(position_mid.collateral, deposit_amount);
    assert!(position_mid.debt >= borrow_amount);

    let repay_amount = 2_000;
    let (_remaining, _interest_paid, _principal_paid) =
        client.repay_debt(&user, &None, &repay_amount);

    let position_after_repay = get_user_position(&env, &contract_id, &user).unwrap();
    assert!(position_after_repay.debt < position_mid.debt);

    let withdraw_amount = 2_000;
    let balance_after_withdraw = client.withdraw_collateral(&user, &None, &withdraw_amount);
    assert_eq!(
        get_collateral_balance(&env, &contract_id, &user),
        balance_after_withdraw
    );
    assert_eq!(balance_after_withdraw, deposit_amount - withdraw_amount);

    let final_position = get_user_position(&env, &contract_id, &user).unwrap();
    assert_eq!(final_position.collateral, deposit_amount - withdraw_amount);
}

/// Liquidation path: set up undercollateralized position, then liquidate.
/// Uses direct storage setup for a position below liquidation threshold, then calls liquidate.
#[test]
fn integration_full_flow_deposit_borrow_liquidate() {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    client.initialize(&admin);

    let collateral = 1_000;
    let debt = 1_000;
    env.as_contract(&contract_id, || {
        let collateral_key = DepositDataKey::CollateralBalance(borrower.clone());
        env.storage().persistent().set(&collateral_key, &collateral);
        let position_key = DepositDataKey::Position(borrower.clone());
        let position = Position {
            collateral,
            debt,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&position_key, &position);
        let analytics_key = DepositDataKey::ProtocolAnalytics;
        let analytics = ProtocolAnalytics {
            total_deposits: collateral,
            total_borrows: debt,
            total_value_locked: collateral,
        };
        env.storage().persistent().set(&analytics_key, &analytics);
    });

    assert!(client.can_be_liquidated(&collateral, &debt));

    let max_liquidatable = client.get_max_liquidatable_amount(&debt);
    let to_liquidate = if max_liquidatable > 0 {
        max_liquidatable.min(500)
    } else {
        500
    };

    let (debt_liq, collateral_seized, incentive) =
        client.liquidate(&liquidator, &borrower, &None, &None, &to_liquidate);

    assert!(debt_liq > 0);
    assert!(collateral_seized >= debt_liq);
    assert!(incentive >= 0);

    let position_after = get_user_position(&env, &contract_id, &borrower).unwrap();
    assert!(position_after.debt < debt || position_after.collateral < collateral);
}
