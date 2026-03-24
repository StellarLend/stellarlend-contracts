use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal,
};

#[test]
fn test_pause_borrow_granular() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    // Initial state: not paused
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    // Pause borrow
    client.set_pause(&admin, &PauseType::Borrow, &true);

    // Try borrow - should fail
    let result = client.try_borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));

    // Try other operations (if not paused) - should succeed
    client.deposit(&user, &asset, &10_000);

    // Unpause borrow
    client.set_pause(&admin, &PauseType::Borrow, &false);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
}

#[test]
fn test_global_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    // Pause all
    client.set_pause(&admin, &PauseType::All, &true);

    // All operations should fail
    assert_eq!(
        client.try_borrow(&user, &asset, &10_000, &collateral_asset, &20_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_deposit(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_repay(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_withdraw(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_liquidate(&admin, &user, &asset, &collateral_asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );

    // Unpause all
    client.set_pause(&admin, &PauseType::All, &false);

    // Operations should succeed
    client.deposit(&user, &asset, &10_000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #6)")]
fn test_set_pause_unauthorized_address() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    // Try to set pause with non-admin address
    client.set_pause(&user, &PauseType::Borrow, &true);
}

#[test]
fn test_all_granular_pauses() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    // Pause Deposit
    client.set_pause(&admin, &PauseType::Deposit, &true);
    assert_eq!(
        client.try_deposit(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    client.set_pause(&admin, &PauseType::Deposit, &false);

    // Pause Repay
    client.set_pause(&admin, &PauseType::Repay, &true);
    assert_eq!(
        client.try_repay(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    client.set_pause(&admin, &PauseType::Repay, &false);

    // Pause Withdraw
    client.set_pause(&admin, &PauseType::Withdraw, &true);
    assert_eq!(
        client.try_withdraw(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    client.set_pause(&admin, &PauseType::Withdraw, &false);

    // Pause Liquidation
    client.set_pause(&admin, &PauseType::Liquidation, &true);
    assert_eq!(
        client.try_liquidate(&admin, &user, &asset, &collateral_asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    client.set_pause(&admin, &PauseType::Liquidation, &false);
}

#[test]
fn test_pause_events() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_pause(&admin, &PauseType::Borrow, &true);

    let events = env.events().all();
    let last_event = events.last().unwrap();

    assert_eq!(last_event.0, contract_id);
    let topic: Symbol = Symbol::try_from_val(&env, &last_event.1.get(0).unwrap()).unwrap();
    assert_eq!(topic, Symbol::new(&env, "pause_event"));
}
