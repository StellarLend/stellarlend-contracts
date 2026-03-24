use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, FromVal, Symbol,
};

/// Helper: register contract and return client
fn setup_env() -> (Env, LendingContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    (env, client)
}

/// Helper: initialize deposit + withdraw settings and deposit collateral
fn setup_with_deposit(
    _env: &Env,
    client: &LendingContractClient,
    user: &Address,
    asset: &Address,
    deposit_amount: i128,
) {
    let admin = Address::generate(_env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.deposit(user, asset, &deposit_amount);
}

// --- Successful withdrawal ---

#[test]
fn test_withdraw_success() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    let remaining = client.withdraw(&user, &asset, &20_000);
    assert_eq!(remaining, 30_000);

    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(position.amount, 30_000);
}

#[test]
fn test_withdraw_full_balance() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    let remaining = client.withdraw(&user, &asset, &50_000);
    assert_eq!(remaining, 0);

    let position = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(position.amount, 0);
}

#[test]
fn test_withdraw_multiple_times() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 100_000);

    let r1 = client.withdraw(&user, &asset, &30_000);
    assert_eq!(r1, 70_000);

    let r2 = client.withdraw(&user, &asset, &20_000);
    assert_eq!(r2, 50_000);

    let r3 = client.withdraw(&user, &asset, &50_000);
    assert_eq!(r3, 0);
}

// --- Invalid amount ---

#[test]
fn test_withdraw_invalid_amount_zero() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    let result = client.try_withdraw(&user, &asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_withdraw_invalid_amount_negative() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    let result = client.try_withdraw(&user, &asset, &-500);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_withdraw_below_minimum() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.deposit(&user, &asset, &50_000);

    let result = client.try_withdraw(&user, &asset, &100); // Default min is 100
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// --- Insufficient collateral ---

#[test]
fn test_withdraw_insufficient_collateral() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 10_000);

    let result = client.try_withdraw(&user, &asset, &50_000);
    assert_eq!(result, Err(Ok(BorrowError::InsufficientCollateral)));
}

#[test]
fn test_withdraw_no_deposit() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    let result = client.try_withdraw(&user, &asset, &1000);
    assert_eq!(result, Err(Ok(BorrowError::InsufficientCollateral)));
}

// --- Pause functionality ---


#[test]
fn test_withdraw_pause_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.deposit(&user, &asset, &50_000);

    client.set_pause(&admin, &PauseType::Withdraw, &true);
    let result = client.try_withdraw(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));

    client.set_pause(&admin, &PauseType::Withdraw, &false);
    let remaining = client.withdraw(&user, &asset, &10_000);
    assert_eq!(remaining, 40_000);
}

// --- Collateral ratio validation ---

#[test]
fn test_withdraw_ratio_violation_with_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.deposit(&user, &collateral_asset, &100_000);
    client.borrow(&user, &asset, &50_000, &collateral_asset, &100_000);

    // Borrow 50k against 100k collateral. Min collateral = 50k * 1.5 = 75k.
    // Try to withdraw 50k -> remaining 50k < 75k -> fail.
    let result = client.try_withdraw(&user, &collateral_asset, &50_000);
    assert_eq!(result, Err(Ok(BorrowError::InsufficientCollateral)));
}

// --- Total deposits tracking ---

#[test]
fn test_withdraw_updates_total_deposits() {
    let (env, client) = setup_env();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let asset = Address::generate(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.deposit(&user1, &asset, &60_000);
    client.deposit(&user2, &asset, &40_000);

    client.withdraw(&user1, &asset, &20_000);

    let pos1 = client.get_user_collateral_deposit(&user1, &asset);
    assert_eq!(pos1.amount, 40_000);

    let pos2 = client.get_user_collateral_deposit(&user2, &asset);
    assert_eq!(pos2.amount, 40_000);
}

// --- Separate users ---

#[test]
fn test_withdraw_separate_users() {
    let (env, client) = setup_env();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let asset = Address::generate(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.deposit(&user1, &asset, &50_000);
    client.deposit(&user2, &asset, &30_000);

    client.withdraw(&user1, &asset, &10_000);

    let pos1 = client.get_user_collateral_deposit(&user1, &asset);
    let pos2 = client.get_user_collateral_deposit(&user2, &asset);
    assert_eq!(pos1.amount, 40_000);
    assert_eq!(pos2.amount, 30_000);
}

// --- Timestamp preservation ---

#[test]
fn test_withdraw_preserves_deposit_timestamp() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.deposit(&user, &asset, &50_000);

    let pos_before = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(pos_before.last_deposit_time, 1000);

    env.ledger().with_mut(|li| {
        li.timestamp = 2000;
    });

    client.withdraw(&user, &asset, &10_000);

    // Withdraw should preserve the last deposit time, not update it
    let pos_after = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(pos_after.last_deposit_time, 1000);
    assert_eq!(pos_after.amount, 40_000);
}

// --- Event emission ---

#[test]
fn test_withdraw_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.deposit(&user, &asset, &50_000);

    client.withdraw(&user, &asset, &20_000);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    let topic: Symbol = Symbol::from_val(&env, &last_event.1.get(0).unwrap());
    assert_eq!(topic, Symbol::new(&env, "withdraw_event"));
}
