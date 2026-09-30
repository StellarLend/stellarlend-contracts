use crate::LendingContract;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, Address, Bytes, Env, IntoVal, Symbol};

fn setup() -> (Env, crate::LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = crate::LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

fn mk_bytes(env: &Env, val: &[u8]) -> Bytes {
    Bytes::from_slice(env, val)
}

fn assert_entry(
    env: &Env,
    client: &crate::LendingContractClient<'static>,
    key: &Symbol,
    expected: &[u8],
) {
    let got = client.config_get(key);
    assert!(got.is_some(), "key {:?} should exist", key);
    assert_eq!(
        got.unwrap(),
        mk_bytes(env, expected),
        "value mismatch for key {:?}",
        key
    );
}

fn assert_absent(client: &crate::LendingContractClient<'static>, key: &Symbol) {
    assert!(
        client.config_get(key).is_none(),
        "key {:?} should be absent",
        key
    );
}

// -----------------------------------------------------------------------
// Round-trip fidelity
// -----------------------------------------------------------------------

#[test]
fn roundtrip_empty_store() {
    let (_env, client, _admin, _user) = setup();
    let name = symbol_short!("e");
    client.config_backup(&name);
    client.config_restore(&name);
}

#[test]
fn roundtrip_single_key_preserves_value() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("s");
    let k = symbol_short!("a");
    let v = mk_bytes(&env, b"hello");

    client.config_set(&k, &v);
    client.config_backup(&name);
    client.config_set(&k, &mk_bytes(&env, b"world"));
    client.config_restore(&name);
    assert_entry(&env, &client, &k, b"hello");
}

#[test]
fn roundtrip_multiple_keys_all_preserved() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("m");
    let ka = symbol_short!("a");
    let kb = symbol_short!("b");
    let kc = symbol_short!("c");

    client.config_set(&ka, &mk_bytes(&env, b"1"));
    client.config_set(&kb, &mk_bytes(&env, b"2"));
    client.config_set(&kc, &mk_bytes(&env, b"3"));
    client.config_backup(&name);

    client.config_set(&ka, &mk_bytes(&env, b"x"));
    client.config_set(&kb, &mk_bytes(&env, b"y"));
    client.config_set(&kc, &mk_bytes(&env, b"z"));
    client.config_restore(&name);

    assert_entry(&env, &client, &ka, b"1");
    assert_entry(&env, &client, &kb, b"2");
    assert_entry(&env, &client, &kc, b"3");
}

#[test]
fn restore_removes_keys_added_after_backup() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("r");
    let keep = symbol_short!("keep");
    client.config_set(&keep, &mk_bytes(&env, b"stays"));
    client.config_backup(&name);

    let added = symbol_short!("new");
    client.config_set(&added, &mk_bytes(&env, b"post"));
    client.config_restore(&name);

    assert_entry(&env, &client, &keep, b"stays");
    assert_absent(&client, &added);
}

#[test]
fn backup_overwrites_previous_snapshot() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("o");
    let k = symbol_short!("x");

    client.config_set(&k, &mk_bytes(&env, b"first"));
    client.config_backup(&name);
    client.config_set(&k, &mk_bytes(&env, b"second"));
    client.config_backup(&name);
    client.config_set(&k, &mk_bytes(&env, b"third"));
    client.config_restore(&name);
    assert_entry(&env, &client, &k, b"second");
}

#[test]
fn restore_is_idempotent() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("i");
    let k = symbol_short!("y");
    client.config_set(&k, &mk_bytes(&env, b"data"));
    client.config_backup(&name);
    client.config_restore(&name);
    client.config_restore(&name);
    assert_entry(&env, &client, &k, b"data");
}

#[test]
fn multiple_backups_are_independent() {
    let (env, client, _admin, _user) = setup();
    let s1 = symbol_short!("s1");
    let s2 = symbol_short!("s2");
    let ka = symbol_short!("a");
    let kb = symbol_short!("b");

    client.config_set(&ka, &mk_bytes(&env, b"a_val"));
    client.config_backup(&s1);
    client.config_set(&kb, &mk_bytes(&env, b"b_val"));
    client.config_backup(&s2);

    client.config_restore(&s1);
    assert_entry(&env, &client, &ka, b"a_val");
    assert_absent(&client, &kb);

    client.config_restore(&s2);
    assert_entry(&env, &client, &ka, b"a_val");
    assert_entry(&env, &client, &kb, b"b_val");
}

// -----------------------------------------------------------------------
// Authorization gating
// -----------------------------------------------------------------------

#[test]
#[should_panic]
fn stranger_cannot_backup() {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let client = crate::LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let stranger = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &stranger,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "config_backup",
            args: (Symbol::new(&env, "x"),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.config_backup(&Symbol::new(&env, "x"));
}

#[test]
#[should_panic]
fn stranger_cannot_restore() {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let client = crate::LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let stranger = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    client.config_backup(&Symbol::new(&env, "x"));
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &stranger,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "config_restore",
            args: (Symbol::new(&env, "x"),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.config_restore(&Symbol::new(&env, "x"));
}

// -----------------------------------------------------------------------
// Edge cases
// -----------------------------------------------------------------------

#[test]
fn restore_missing_backup_returns_error() {
    let (env, client, _admin, _user) = setup();
    let name = Symbol::new(&env, "missing");
    let res = client.try_config_restore(&name);
    assert!(
        matches!(res, Err(Ok(crate::LendingError::BackupNotFound))),
        "expected BackupNotFound, got {:?}",
        res
    );
}

#[test]
fn config_set_empty_value_allowed() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("ev");
    let k = symbol_short!("e");
    let empty = Bytes::new(&env);

    client.config_set(&k, &empty);
    client.config_backup(&name);
    client.config_set(&k, &mk_bytes(&env, b"not_empty"));
    client.config_restore(&name);
    assert_entry(&env, &client, &k, b"");
}

#[test]
fn config_get_nonexistent_key() {
    let (_env, client, _admin, _user) = setup();
    let missing = symbol_short!("none");
    assert_absent(&client, &missing);
}

#[test]
fn large_value_roundtrip() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("lg");
    let k = symbol_short!("big");

    let data: [u8; 200] = core::array::from_fn(|i| (i % 256) as u8);
    let val = Bytes::from_slice(&env, &data);
    client.config_set(&k, &val);
    client.config_backup(&name);
    client.config_set(&k, &mk_bytes(&env, b"small"));
    client.config_restore(&name);
    assert_entry(&env, &client, &k, &data);
}

#[test]
fn overwrite_then_restore() {
    let (env, client, _admin, _user) = setup();
    let name = symbol_short!("ow");
    let k = symbol_short!("k");
    client.config_set(&k, &mk_bytes(&env, b"original"));
    client.config_backup(&name);
    client.config_set(&k, &mk_bytes(&env, b"over1"));
    client.config_set(&k, &mk_bytes(&env, b"over2"));
    client.config_restore(&name);
    assert_entry(&env, &client, &k, b"original");
}
