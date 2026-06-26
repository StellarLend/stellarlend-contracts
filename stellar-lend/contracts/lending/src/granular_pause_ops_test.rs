use crate::{LendingContract, LendingContractClient, PauseType};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user)
}

fn pause(
    env: &Env,
    client: &LendingContractClient<'static>,
    admin: &Address,
    operation: PauseType,
) {
    let expires_at = env.ledger().sequence().saturating_add(5);
    client.set_pause(admin, &operation, &true, &expires_at);
}

fn advance_past_pause_expiry(env: &Env) {
    let mut ledger = env.ledger().get();
    ledger.sequence_number = ledger.sequence_number.saturating_add(10);
    env.ledger().set(ledger);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn deposit_specific_pause_blocks_deposit() {
    let (env, client, admin, user) = setup();
    pause(&env, &client, &admin, PauseType::Deposit);

    client.deposit(&user, &100);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn deposit_all_pause_blocks_deposit() {
    let (env, client, admin, user) = setup();
    pause(&env, &client, &admin, PauseType::All);

    client.deposit(&user, &100);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn withdraw_specific_pause_blocks_withdraw() {
    let (env, client, admin, user) = setup();
    client.deposit(&user, &100);
    pause(&env, &client, &admin, PauseType::Withdraw);

    client.withdraw(&user, &25);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn withdraw_all_pause_blocks_withdraw() {
    let (env, client, admin, user) = setup();
    client.deposit(&user, &100);
    pause(&env, &client, &admin, PauseType::All);

    client.withdraw(&user, &25);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn borrow_specific_pause_blocks_borrow() {
    let (env, client, admin, user) = setup();
    pause(&env, &client, &admin, PauseType::Borrow);

    client.borrow(&user, &50);
}

#[test]
#[should_panic(expected = "OperationPaused")]
fn borrow_all_pause_blocks_borrow() {
    let (env, client, admin, user) = setup();
    pause(&env, &client, &admin, PauseType::All);

    client.borrow(&user, &50);
}

#[test]
fn expired_pause_allows_operation_again() {
    let (env, client, admin, user) = setup();
    pause(&env, &client, &admin, PauseType::Deposit);
    advance_past_pause_expiry(&env);

    assert_eq!(client.deposit(&user, &100), 100);
    assert!(!client.get_pause_state(&PauseType::Deposit));
}
