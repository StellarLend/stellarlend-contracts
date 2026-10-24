#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn set_reserves(env: &Env, contract: &Address, reserves: i128) {
    env.as_contract(contract, || {
        env.storage()
            .persistent()
            .set(&DataKey::ProtocolReserves, &reserves);
    });
}

#[test]
fn withdraw_reserves_reduces_accounting_balance() {
    let (env, client, _admin) = setup();
    let to = Address::generate(&env);
    set_reserves(&env, &client.address, 1_000);

    client.withdraw_reserves(&400, &to);

    assert_eq!(client.get_protocol_reserves(), 600);
}

#[test]
fn withdraw_reserves_rejects_amount_above_balance() {
    let (env, client, _admin) = setup();
    let to = Address::generate(&env);
    set_reserves(&env, &client.address, 999);

    let result = client.try_withdraw_reserves(&1_000, &to);
    assert!(matches!(result, Err(Ok(LendingError::InvalidAmount))));
    assert_eq!(client.get_protocol_reserves(), 999);
}

#[test]
fn withdraw_reserves_rejects_non_positive_amount() {
    let (env, client, _admin) = setup();
    let to = Address::generate(&env);

    for amount in [0, -1] {
        let result = client.try_withdraw_reserves(&amount, &to);
        assert!(matches!(result, Err(Ok(LendingError::InvalidAmount))));
    }
}

#[test]
#[should_panic]
fn withdraw_reserves_requires_admin_authorization() {
    let env = Env::default();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let to = Address::generate(&env);

    env.mock_all_auths();
    client.initialize(&admin);
    set_reserves(&env, &client.address, 100);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "withdraw_reserves",
            args: (10i128, to.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.withdraw_reserves(&10, &to);
}
