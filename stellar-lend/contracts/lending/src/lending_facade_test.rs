//! Covers `LendingContract` entrypoints in `lib.rs` that forward to other modules
//! but are not exercised by `data_store_test.rs` (which uses `DataStore` directly).

use soroban_sdk::{testutils::Address as _, Address, Bytes, Env, String as SorobanString};

use crate::borrow::BorrowError;
use crate::{LendingContract, LendingContractClient};

#[test]
fn test_get_performance_stats_returns_placeholder_vector() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    let stats = client.get_performance_stats();
    assert_eq!(stats.len(), 2);
    assert_eq!(stats.get(0).unwrap(), 0u64);
    assert_eq!(stats.get(1).unwrap(), 0u64);
}

#[test]
fn test_data_store_init_idempotent_through_lending_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.data_store_init(&admin);
    client.data_store_init(&admin);
    assert_eq!(client.data_schema_version(), 0u32);
}

#[test]
fn test_data_key_exists_and_data_revoke_writer_facade() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let writer = Address::generate(&env);
    let key = SorobanString::from_str(&env, "facade_key");
    client.data_store_init(&admin);
    assert!(!client.data_key_exists(&key));
    client.data_grant_writer(&admin, &writer);
    let val = Bytes::from_slice(&env, b"v");
    client.data_save(&writer, &key, &val);
    assert!(client.data_key_exists(&key));
    client.data_revoke_writer(&admin, &writer);
    let denied = client.try_data_save(&writer, &key, &val);
    assert!(denied.is_err());
}

#[test]
fn test_initialize_rejects_double_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    let again = client.try_initialize(&admin, &1_000_000_000, &1000);
    assert_eq!(again, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_get_admin_none_before_initialize_facade() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    assert_eq!(client.get_admin(), None);
}

#[test]
fn test_get_user_debt_and_collateral_facade_without_borrow_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let _debt = client.get_user_debt(&user);
    let _collateral = client.get_user_collateral(&user);
}
