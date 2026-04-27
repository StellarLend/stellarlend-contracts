use super::*;
use crate::withdraw::WithdrawError;
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
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);
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
    assert_eq!(result, Err(Ok(WithdrawError::InvalidAmount)));
}

#[test]
fn test_withdraw_invalid_amount_negative() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    let result = client.try_withdraw(&user, &asset, &-500);
    assert_eq!(result, Err(Ok(WithdrawError::InvalidAmount)));
}

#[test]
fn test_withdraw_below_minimum() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&5000);
    client.deposit(&user, &asset, &50_000);

    let result = client.try_withdraw(&user, &asset, &1000);
    assert_eq!(result, Err(Ok(WithdrawError::InvalidAmount)));
}

// --- Insufficient collateral ---

#[test]
fn test_withdraw_insufficient_collateral() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 10_000);

    let result = client.try_withdraw(&user, &asset, &50_000);
    assert_eq!(result, Err(Ok(WithdrawError::InsufficientCollateral)));
}

#[test]
fn test_withdraw_no_deposit() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    let result = client.try_withdraw(&user, &asset, &1000);
    assert_eq!(result, Err(Ok(WithdrawError::InsufficientCollateral)));
}

// --- Pause functionality ---

#[test]
fn test_withdraw_paused() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);
    client.set_withdraw_paused(&true);

    let result = client.try_withdraw(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(WithdrawError::WithdrawPaused)));
}

#[test]
fn test_withdraw_pause_unpause() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    client.set_withdraw_paused(&true);
    let result = client.try_withdraw(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(WithdrawError::WithdrawPaused)));

    client.set_withdraw_paused(&false);
    let remaining = client.withdraw(&user, &asset, &10_000);
    assert_eq!(remaining, 40_000);
}

// --- Collateral ratio validation ---

#[test]
fn test_withdraw_ratio_violation_with_debt() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Deposit 100,000 collateral
    setup_with_deposit(&env, &client, &user, &asset, 100_000);

    // Contract already initialized in setup_with_deposit
    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &15_000);

    // Try to withdraw 90,000 -> remaining 10,000 vs debt 10,000 * 1.5 = 15,000 -> fail
    let result = client.try_withdraw(&user, &asset, &90_000);
    assert_eq!(result, Err(Ok(WithdrawError::InsufficientCollateralRatio)));
}

#[test]
fn test_withdraw_ratio_valid_with_debt() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Deposit 100,000 collateral
    setup_with_deposit(&env, &client, &user, &asset, 100_000);

    // Borrow 10,000 against 15,000 collateral
    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &15_000);

    // Withdraw 80,000 -> remaining 20,000 vs debt 10,000 * 1.5 = 15,000 -> pass
    let remaining = client.withdraw(&user, &asset, &80_000);
    assert_eq!(remaining, 20_000);
}

#[test]
fn test_withdraw_ratio_boundary_exact_150_percent() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Deposit 100,000
    setup_with_deposit(&env, &client, &user, &asset, 100_000);

    // Borrow 10,000 (min collateral = 10,000 * 1.5 = 15,000)
    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &15_000);

    // Withdraw exactly to 15,000 remaining -> should succeed (exactly 150%)
    let remaining = client.withdraw(&user, &asset, &85_000);
    assert_eq!(remaining, 15_000);

    // Further withdrawal would violate ratio (tested in ratio_boundary_just_below)
}

#[test]
fn test_withdraw_ratio_boundary_just_below() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Deposit 30,000
    setup_with_deposit(&env, &client, &user, &asset, 30_000);

    // Borrow 10,000 (min collateral = 15,000)
    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &15_000);

    // Withdraw 15,100 -> remaining 14,900 < 15,000 -> fail
    let result = client.try_withdraw(&user, &asset, &15_100);
    assert_eq!(result, Err(Ok(WithdrawError::InsufficientCollateralRatio)));
}

#[test]
fn test_withdraw_no_debt_allows_full_withdrawal() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    // No borrow debt — full withdrawal allowed
    let remaining = client.withdraw(&user, &asset, &50_000);
    assert_eq!(remaining, 0);
}

// --- Max withdrawal ---

#[test]
fn test_withdraw_max_with_debt() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let borrow_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Deposit 100,000
    setup_with_deposit(&env, &client, &user, &asset, 100_000);

    // Borrow 10,000 (min collateral = 15,000)
    client.borrow(&user, &borrow_asset, &10_000, &collateral_asset, &15_000);

    // Max safe withdrawal = 100,000 - 15,000 = 85,000
    let remaining = client.withdraw(&user, &asset, &85_000);
    assert_eq!(remaining, 15_000);

    // Any further withdrawal beyond min_withdraw should fail on ratio
    let result = client.try_withdraw(&user, &asset, &100);
    assert_eq!(result, Err(Ok(WithdrawError::InsufficientCollateralRatio)));
}

// --- Total deposits tracking ---

#[test]
fn test_withdraw_updates_total_deposits() {
    let (env, client) = setup_env();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user1, &asset, 60_000);
    client.deposit(&user2, &asset, &40_000);

    // Total deposits = 100,000
    client.withdraw(&user1, &asset, &20_000);

    // Verify user1 position
    let pos1 = client.get_user_collateral_deposit(&user1, &asset);
    assert_eq!(pos1.amount, 40_000);

    // Verify user2 unaffected
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

    setup_with_deposit(&env, &client, &user1, &asset, 50_000);
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
    let (env, client) = setup_env();

    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });

    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

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
    let (env, client) = setup_env();

    env.ledger().with_mut(|li| {
        li.timestamp = 5000;
    });

    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    client.withdraw(&user, &asset, &20_000);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    let topic: Symbol = Symbol::from_val(&env, &last_event.1.get(0).unwrap());
    assert_eq!(topic, Symbol::new(&env, "withdraw_event"));
}

// --- Edge cases ---

#[test]
fn test_withdraw_minimum_amount_boundary() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&500);
    client.deposit(&user, &asset, &50_000);

    // Below minimum — should fail
    let result = client.try_withdraw(&user, &asset, &499);
    assert_eq!(result, Err(Ok(WithdrawError::InvalidAmount)));

    // Exactly minimum — should succeed
    let remaining = client.withdraw(&user, &asset, &500);
    assert_eq!(remaining, 49_500);
}

#[test]
fn test_withdraw_after_multiple_deposits() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 20_000);
    client.deposit(&user, &asset, &30_000);

    // Total deposited = 50,000
    let remaining = client.withdraw(&user, &asset, &40_000);
    assert_eq!(remaining, 10_000);
}

#[test]
fn test_withdraw_deposit_withdraw_cycle() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    client.withdraw(&user, &asset, &20_000);
    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(pos.amount, 30_000);

    client.deposit(&user, &asset, &10_000);
    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(pos.amount, 40_000);

    client.withdraw(&user, &asset, &40_000);
    let pos = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(pos.amount, 0);
}

#[test]
fn test_withdraw_global_pause() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    // Set global pause instead of withdraw-specific pause
    let admin = client.get_admin().unwrap();
    client.set_pause(&admin, &crate::pause::PauseType::All, &true);

    let result = client.try_withdraw(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(WithdrawError::WithdrawPaused)));
}

#[test]
fn test_withdraw_invalid_min_amount_setup() {
    let (env, client) = setup_env();
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    setup_with_deposit(&env, &client, &user, &asset, 50_000);

    // Set min withdraw to 500
    client.initialize_withdraw_settings(&500);

    // Withdrawal for 499 should fail
    let result = client.try_withdraw(&user, &asset, &499);
    assert_eq!(result, Err(Ok(WithdrawError::InvalidAmount)));
}
