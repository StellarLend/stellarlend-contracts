//! Lightweight regression test for `LendingContract::liquidate`.
//!
//! The older version of this test depended on Soroban budget APIs that are no
//! longer available in SDK 25.3.1. Keep a smoke test here so the liquidation
//! path is still exercised in a focused way.

use crate::{debt::DebtPosition, LendingContract, LendingContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn liquidate_smoke_test() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.initialize(&admin);

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&crate::DataKey::Collateral(borrower.clone()), &100_i128);
        crate::debt::save_debt(
            &env,
            &borrower,
            &DebtPosition {
                principal: 200,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    let repaid = client.liquidate(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        &1_000,
    );
    assert_eq!(repaid, 100);
}
