#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env,
};
use stellarlend_lending::{LendingContract, LendingContractClient};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    (env, client, contract_id)
}

fn assert_last_event_contains(env: &Env, contract_id: &Address, expected_fragments: &[&str]) {
    let all = env.events().all();
    let filtered = all.filter_by_contract(contract_id);
    let event = filtered.events().last().expect("expected event");
    let debug = format!("{event:?}");
    for fragment in expected_fragments {
        assert!(
            debug.contains(fragment),
            "event debug output missing {fragment:?}: {debug}"
        );
    }
}

#[test]
fn deposit_emits_versioned_event_with_resulting_balance() {
    let (env, client, contract_id) = setup();
    let user = Address::generate(&env);

    let balance = client.deposit(&user, &125);

    assert_eq!(balance, 125);
    assert_last_event_contains(
        &env,
        &contract_id,
        &[
            "deposit_event",
            "schema_version",
            "U32(1)",
            "amount",
            "125",
            "resulting_balance",
            "user",
        ],
    );
}

#[test]
fn withdraw_emits_versioned_event_with_resulting_balance() {
    let (env, client, contract_id) = setup();
    let user = Address::generate(&env);

    client.deposit(&user, &500);
    let balance = client.withdraw(&user, &175);

    assert_eq!(balance, 325);
    assert_last_event_contains(
        &env,
        &contract_id,
        &[
            "withdraw_event",
            "schema_version",
            "U32(1)",
            "amount",
            "175",
            "resulting_balance",
            "325",
            "user",
        ],
    );
}

#[test]
fn borrow_emits_versioned_event_with_resulting_debt() {
    let (env, client, contract_id) = setup();
    let borrower = Address::generate(&env);

    let debt = client.borrow(&borrower, &240);

    assert_eq!(debt, 240);
    assert_last_event_contains(
        &env,
        &contract_id,
        &[
            "borrow_event",
            "schema_version",
            "U32(1)",
            "amount",
            "240",
            "resulting_debt",
            "borrower",
        ],
    );
}

#[test]
fn full_repay_emits_versioned_event_with_zero_resulting_debt() {
    let (env, client, contract_id) = setup();
    let borrower = Address::generate(&env);

    client.borrow(&borrower, &300);
    let debt = client.repay(&borrower, &300);

    assert_eq!(debt, 0);
    assert_last_event_contains(
        &env,
        &contract_id,
        &[
            "repay_event",
            "schema_version",
            "U32(1)",
            "amount",
            "300",
            "resulting_debt",
            "0",
            "borrower",
        ],
    );
}

#[test]
fn liquidate_emits_versioned_event_with_repaid_and_seized_amounts() {
    let (env, client, contract_id) = setup();
    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);

    client.deposit(&borrower, &100);
    client.borrow(&borrower, &200);
    let repaid = client.liquidate(&liquidator, &borrower, &40);

    assert_eq!(repaid, 40);
    assert_last_event_contains(
        &env,
        &contract_id,
        &[
            "liquidate_event",
            "schema_version",
            "U32(1)",
            "liquidator",
            "borrower",
            "repaid_debt",
            "40",
            "seized_collateral",
            "44",
            "resulting_debt",
            "160",
            "resulting_collateral",
            "56",
        ],
    );
}
