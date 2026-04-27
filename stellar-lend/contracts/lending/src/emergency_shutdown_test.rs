use super::*;
use crate::deposit::DepositError;
use crate::flash_loan::FlashLoanError;
use crate::withdraw::WithdrawError;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_emergency_shutdown_authorization_and_state_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);

    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
    assert_eq!(client.get_guardian(), Some(guardian.clone()));

    assert_eq!(
        client.try_emergency_shutdown(&user),
        Err(Ok(BorrowError::Unauthorized))
    );

    client.emergency_shutdown(&guardian);
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);

    client.start_recovery(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Recovery);

    client.complete_recovery(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
}

#[test]
fn test_shutdown_blocks_high_risk_ops_and_recovery_allows_unwind_only() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    client.deposit(&user, &asset, &50_000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    client.emergency_shutdown(&guardian);

    assert_eq!(
        client.try_deposit(&user, &asset, &1000),
        Err(Ok(DepositError::DepositPaused))
    );
    assert_eq!(
        client.try_borrow(&user, &asset, &1000, &collateral_asset, &2000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_repay(&user, &asset, &1000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_withdraw(&user, &asset, &1000),
        Err(Ok(WithdrawError::WithdrawPaused))
    );
    assert_eq!(
        client.try_flash_loan(&user, &asset, &1000, &soroban_sdk::Bytes::new(&env)),
        Err(Ok(FlashLoanError::ProtocolPaused))
    );

    client.start_recovery(&admin);

    assert_eq!(
        client.try_borrow(&user, &asset, &1000, &collateral_asset, &2000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_deposit(&user, &asset, &1000),
        Err(Ok(DepositError::DepositPaused))
    );
    assert_eq!(
        client.try_flash_loan(&user, &asset, &1000, &soroban_sdk::Bytes::new(&env)),
        Err(Ok(FlashLoanError::ProtocolPaused))
    );

    // Controlled unwind path: repay and withdraw are allowed in recovery mode.
    client.repay(&user, &asset, &1000);
    client.withdraw(&user, &asset, &1000);
}

#[test]
fn test_recovery_transition_edge_cases_and_partial_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    // Cannot start recovery before a shutdown.
    assert_eq!(
        client.try_start_recovery(&admin),
        Err(Ok(BorrowError::ProtocolPaused))
    );

    client.deposit(&user, &asset, &50_000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    client.emergency_shutdown(&admin);
    client.start_recovery(&admin);

    // Partial controls still apply in recovery.
    client.set_pause(&admin, &PauseType::Repay, &true);
    assert_eq!(
        client.try_repay(&user, &asset, &1000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    client.set_pause(&admin, &PauseType::Repay, &false);

    client.set_pause(&admin, &PauseType::Withdraw, &true);
    assert_eq!(
        client.try_withdraw(&user, &asset, &1000),
        Err(Ok(WithdrawError::WithdrawPaused))
    );
    client.set_pause(&admin, &PauseType::Withdraw, &false);

    client.repay(&user, &asset, &1000);
    client.withdraw(&user, &asset, &1000);
}
