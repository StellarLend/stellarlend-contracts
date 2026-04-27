#![cfg(test)]

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, Symbol, Val, Vec};

fn setup_test() -> (Env, HelloContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    // Initialize risk parameters to set admin
    client.initialize(&admin);

    (env, client, admin)
}

#[test]
fn test_config_set_and_get() {
    let (env, client, admin) = setup_test();
    let key = Symbol::new(&env, "fee_rate");
    let val: Val = 100_u32.into_val(&env);

    // Admin sets config
    client.config_set(&admin, &key, &val);

    // Anyone can get config
    let retrieved_val = client.config_get(&key).unwrap();
    assert_eq!(retrieved_val.get_payload(), val.get_payload());
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_config_set_unauthorized() {
    let (env, client, _admin) = setup_test();
    let malicious = Address::generate(&env);

    let key = Symbol::new(&env, "fee_rate");
    let val: Val = 100_u32.into_val(&env);

    // Non-admin tries to set config (should panic/return err)
    client.config_set(&malicious, &key, &val);
}

#[test]
fn test_config_backup_and_restore() {
    let (env, client, admin) = setup_test();

    let key1 = Symbol::new(&env, "param1");
    let val1: Val = 10_u32.into_val(&env);

    let key2 = Symbol::new(&env, "param2");
    let val2: Val = 20_u32.into_val(&env);

    // Set config values
    client.config_set(&admin, &key1, &val1);
    client.config_set(&admin, &key2, &val2);

    // Backup keys
    let mut backup_keys = Vec::new(&env);
    backup_keys.push_back(key1.clone());
    backup_keys.push_back(key2.clone());

    let backup_data = client.config_backup(&admin, &backup_keys);
    assert_eq!(backup_data.len(), 2);

    // Create new environment to simulate restore
    let env2 = Env::default();
    env2.mock_all_auths();
    let admin2 = Address::generate(&env2);
    let contract_id2 = env2.register(HelloContract, ());
    let client2 = HelloContractClient::new(&env2, &contract_id2);
    client2.initialize(&admin2);

    // Translate backup data to new env
    let mut backup_data2 = Vec::new(&env2);
    let key1_env2 = Symbol::new(&env2, "param1");
    let val1_env2: Val = 10_u32.into_val(&env2);
    let key2_env2 = Symbol::new(&env2, "param2");
    let val2_env2: Val = 20_u32.into_val(&env2);

    backup_data2.push_back((key1_env2.clone(), val1_env2));
    backup_data2.push_back((key2_env2.clone(), val2_env2));

    // Restore
    client2.config_restore(&admin2, &backup_data2);

    // Verify restore
    let restored_val1 = client2.config_get(&key1_env2).unwrap();
    let expected_val1: Val = 10_u32.into_val(&env2);
    assert_eq!(restored_val1.get_payload(), expected_val1.get_payload());

    let restored_val2 = client2.config_get(&key2_env2).unwrap();
    let expected_val2: Val = 20_u32.into_val(&env2);
    assert_eq!(restored_val2.get_payload(), expected_val2.get_payload());
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_config_backup_unauthorized() {
    let (env, client, _) = setup_test();
    let malicious = Address::generate(&env);
    let mut keys = Vec::new(&env);
    keys.push_back(Symbol::new(&env, "param1"));

    let _ = client.config_backup(&malicious, &keys);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_config_restore_unauthorized() {
    let (env, client, _) = setup_test();
    let malicious = Address::generate(&env);
    let backup = Vec::new(&env);

    client.config_restore(&malicious, &backup);
}
