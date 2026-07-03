use crate::{DataKey, LendingContract, LendingContractClient, LendingError};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

/// Builds an initialized lending contract with mocked auth for direct client calls.
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

/// Stores a test-specific deposit cap without exercising unrelated admin flows.
fn set_deposit_cap(env: &Env, contract_id: &Address, cap: i128) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
    });
}

/// Reads the protocol-wide TotalDeposits counter from persistent storage.
fn read_total_deposits(env: &Env, contract_id: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TotalDeposits)
            .unwrap_or(0)
    })
}

/// Asserts that the ledger has not advanced between simulated same-ledger operations.
fn assert_same_ledger(env: &Env, expected_sequence: u32) {
    assert_eq!(env.ledger().sequence(), expected_sequence);
}

#[test]
fn same_ledger_second_deposit_rejects_when_running_total_would_cross_cap() {
    let (env, client, _admin, user1) = setup();
    let user2 = Address::generate(&env);
    let ledger_sequence = 1048;
    env.ledger().set_sequence_number(ledger_sequence);
    set_deposit_cap(&env, &client.address, 1_000);

    assert_eq!(client.deposit(&user1, &600), 600);
    assert_same_ledger(&env, ledger_sequence);
    assert_eq!(read_total_deposits(&env, &client.address), 600);

    let res = client.try_deposit(&user2, &500);
    assert!(
        matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
        "expected DepositCapExceeded, got {:?}",
        res
    );
    assert_same_ledger(&env, ledger_sequence);
    assert_eq!(read_total_deposits(&env, &client.address), 600);
}

#[test]
fn same_ledger_deposits_can_fill_exact_cap_then_reject_one_over() {
    let (env, client, _admin, user1) = setup();
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);
    let ledger_sequence = 1049;
    env.ledger().set_sequence_number(ledger_sequence);
    set_deposit_cap(&env, &client.address, 1_000);

    assert_eq!(client.deposit(&user1, &250), 250);
    assert_same_ledger(&env, ledger_sequence);

    assert_eq!(client.deposit(&user2, &750), 750);
    assert_same_ledger(&env, ledger_sequence);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);

    let res = client.try_deposit(&user3, &1);
    assert!(
        matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
        "expected DepositCapExceeded, got {:?}",
        res
    );
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);
}

#[test]
fn withdraw_in_same_ledger_frees_headroom_for_later_deposit() {
    let (env, client, _admin, user1) = setup();
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);
    let ledger_sequence = 1050;
    env.ledger().set_sequence_number(ledger_sequence);
    set_deposit_cap(&env, &client.address, 1_000);

    client.deposit(&user1, &800);
    client.deposit(&user2, &200);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);

    assert_eq!(client.withdraw(&user1, &125), 675);
    assert_same_ledger(&env, ledger_sequence);
    assert_eq!(read_total_deposits(&env, &client.address), 875);

    assert_eq!(client.deposit(&user3, &125), 125);
    assert_same_ledger(&env, ledger_sequence);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);
}

#[test]
fn cap_reduced_below_current_total_rejects_until_withdraw_creates_room() {
    let (env, client, _admin, user1) = setup();
    let user2 = Address::generate(&env);
    let ledger_sequence = 1051;
    env.ledger().set_sequence_number(ledger_sequence);
    set_deposit_cap(&env, &client.address, 1_000);

    client.deposit(&user1, &1_000);
    set_deposit_cap(&env, &client.address, 900);

    let res = client.try_deposit(&user2, &1);
    assert!(
        matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
        "expected DepositCapExceeded, got {:?}",
        res
    );
    assert_same_ledger(&env, ledger_sequence);
    assert_eq!(read_total_deposits(&env, &client.address), 1_000);

    assert_eq!(client.withdraw(&user1, &200), 800);
    assert_eq!(client.deposit(&user2, &100), 100);
    assert_eq!(read_total_deposits(&env, &client.address), 900);

    let res = client.try_deposit(&user2, &1);
    assert!(
        matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
        "expected DepositCapExceeded, got {:?}",
        res
    );
    assert_eq!(read_total_deposits(&env, &client.address), 900);
}
