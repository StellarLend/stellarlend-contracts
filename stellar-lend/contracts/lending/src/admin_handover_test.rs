#![cfg(test)]

use super::{DataKey, LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

fn setup() -> (
    Env,
    Address,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let first_pending_admin = Address::generate(&env);
    let second_pending_admin = Address::generate(&env);

    client.initialize(&admin);

    (
        env,
        contract_id,
        client,
        admin,
        first_pending_admin,
        second_pending_admin,
    )
}

fn mock_propose_admin_auth(
    env: &Env,
    contract_id: &Address,
    caller: &Address,
    new_admin: &Address,
) {
    env.mock_auths(&[MockAuth {
        address: caller,
        invoke: &MockAuthInvoke {
            contract: contract_id,
            fn_name: "propose_admin",
            args: (new_admin.clone(),).into_val(env),
            sub_invokes: &[],
        },
    }]);
}

fn mock_accept_admin_auth(env: &Env, contract_id: &Address, caller: &Address) {
    env.mock_auths(&[MockAuth {
        address: caller,
        invoke: &MockAuthInvoke {
            contract: contract_id,
            fn_name: "accept_admin",
            args: ().into_val(env),
            sub_invokes: &[],
        },
    }]);
}

#[test]
fn propose_and_accept_admin_updates_admin_and_clears_pending() {
    let (env, contract_id, client, admin, pending_admin, _second_pending_admin) = setup();

    mock_propose_admin_auth(&env, &contract_id, &admin, &pending_admin);
    client.propose_admin(&pending_admin);

    let stored_pending: Address = env
        .storage()
        .instance()
        .get(&DataKey::PendingAdmin)
        .expect("pending admin should be stored");
    assert_eq!(stored_pending, pending_admin);

    mock_accept_admin_auth(&env, &contract_id, &pending_admin);
    let res = client.try_accept_admin();
    assert_eq!(res, Ok(Ok(())));
    assert_eq!(client.get_admin(), pending_admin);
    assert!(!env.storage().instance().has(&DataKey::PendingAdmin));
}

#[test]
fn accept_admin_without_pending_returns_error() {
    let (env, _contract_id, client, admin, _pending_admin, _second_pending_admin) = setup();

    let res = client.try_accept_admin();
    assert_eq!(res, Err(Ok(LendingError::PendingAdminNotSet)));
    assert_eq!(client.get_admin(), admin);
    assert!(!env.storage().instance().has(&DataKey::PendingAdmin));
}

#[test]
#[should_panic]
fn accept_admin_rejects_non_pending_signer() {
    let (env, contract_id, client, admin, pending_admin, _second_pending_admin) = setup();
    let wrong_acceptor = Address::generate(&env);

    mock_propose_admin_auth(&env, &contract_id, &admin, &pending_admin);
    client.propose_admin(&pending_admin);

    mock_accept_admin_auth(&env, &contract_id, &wrong_acceptor);
    client.accept_admin();
}

#[test]
fn double_accept_after_success_returns_missing_pending_error() {
    let (env, contract_id, client, admin, pending_admin, _second_pending_admin) = setup();

    mock_propose_admin_auth(&env, &contract_id, &admin, &pending_admin);
    client.propose_admin(&pending_admin);

    mock_accept_admin_auth(&env, &contract_id, &pending_admin);
    client.accept_admin();
    assert_eq!(client.get_admin(), pending_admin);
    assert!(!env.storage().instance().has(&DataKey::PendingAdmin));

    let res = client.try_accept_admin();
    assert_eq!(res, Err(Ok(LendingError::PendingAdminNotSet)));
}

#[test]
fn re_propose_overwrites_previous_pending_admin() {
    let (env, contract_id, client, admin, first_pending_admin, second_pending_admin) = setup();

    mock_propose_admin_auth(&env, &contract_id, &admin, &first_pending_admin);
    client.propose_admin(&first_pending_admin);
    let stored_pending: Address = env
        .storage()
        .instance()
        .get(&DataKey::PendingAdmin)
        .expect("first pending admin should be stored");
    assert_eq!(stored_pending, first_pending_admin);

    mock_propose_admin_auth(&env, &contract_id, &admin, &second_pending_admin);
    client.propose_admin(&second_pending_admin);
    let overwritten_pending: Address = env
        .storage()
        .instance()
        .get(&DataKey::PendingAdmin)
        .expect("second pending admin should overwrite the first");
    assert_eq!(overwritten_pending, second_pending_admin);

    mock_accept_admin_auth(&env, &contract_id, &second_pending_admin);
    assert_eq!(client.try_accept_admin(), Ok(Ok(())));
    assert_eq!(client.get_admin(), second_pending_admin);
    assert!(!env.storage().instance().has(&DataKey::PendingAdmin));
}
