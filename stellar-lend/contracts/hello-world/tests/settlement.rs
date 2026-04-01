//! Integration tests for settlement finalization (avoids compiling legacy lib test suite).

use hello_world::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, vec, Address, Env, Vec};

fn setup() -> (Env, HelloContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn finalize_settlement_links_tx_ids() {
    let (_env, client, admin) = setup();
    let sid = 100_u64;
    let tx_ids = vec![&_env, 1_u64, 2_u64, 3_u64];

    client.finalize_settlement(&admin, &sid, &tx_ids);

    assert_eq!(client.get_tx_settlement_id(&1_u64), Some(sid));
    assert_eq!(client.get_tx_settlement_id(&2_u64), Some(sid));
    assert_eq!(client.get_tx_settlement_id(&3_u64), Some(sid));
    assert_eq!(client.get_tx_settlement_id(&99_u64), None);
}

#[test]
fn finalize_settlement_empty_tx_list_ok() {
    let (env, client, admin) = setup();
    let tx_ids: Vec<u64> = Vec::new(&env);
    client.finalize_settlement(&admin, &1_u64, &tx_ids);
}

#[test]
#[should_panic(expected = "transaction already settled")]
fn finalize_settlement_rejects_already_linked_tx() {
    let (_env, client, admin) = setup();
    let tx_ids = vec![&_env, 42_u64];
    client.finalize_settlement(&admin, &1_u64, &tx_ids);
    client.finalize_settlement(&admin, &2_u64, &tx_ids);
}

#[test]
#[should_panic(expected = "transaction already settled")]
fn finalize_settlement_rejects_if_any_tx_already_settled_in_batch() {
    let (_env, client, admin) = setup();
    client.finalize_settlement(&admin, &1_u64, &vec![&_env, 10_u64]);
    let batch = vec![&_env, 10_u64, 20_u64];
    client.finalize_settlement(&admin, &2_u64, &batch);
}
