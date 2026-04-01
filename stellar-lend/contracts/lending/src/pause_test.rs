use super::*;
use crate::cross_asset::CrossAssetError;
use crate::deposit::DepositError;
use crate::flash_loan::FlashLoanError;
use crate::oracle::OracleError;
use crate::withdraw::WithdrawError;
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal, Vec,
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
        Err(Ok(DepositError::DepositPaused))
    );
    assert_eq!(
        client.try_repay(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_withdraw(&user, &asset, &10_000),
        Err(Ok(WithdrawError::WithdrawPaused))
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
        Err(Ok(DepositError::DepositPaused))
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
        Err(Ok(WithdrawError::WithdrawPaused))
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

// ═══════════════════════════════════════════════════════════════════════════
// get_pause_state query
// ═══════════════════════════════════════════════════════════════════════════

/// Default state: every operation flag starts as not-paused.
#[test]
fn test_get_pause_state_default_false() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    assert!(!client.get_pause_state(&PauseType::Deposit));
    assert!(!client.get_pause_state(&PauseType::Borrow));
    assert!(!client.get_pause_state(&PauseType::Repay));
    assert!(!client.get_pause_state(&PauseType::Withdraw));
    assert!(!client.get_pause_state(&PauseType::Liquidation));
    assert!(!client.get_pause_state(&PauseType::All));
}

/// get_pause_state reflects the state set by set_pause.
#[test]
fn test_get_pause_state_reflects_set_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_pause(&admin, &PauseType::Deposit, &true);
    assert!(client.get_pause_state(&PauseType::Deposit));
    assert!(!client.get_pause_state(&PauseType::Borrow));

    client.set_pause(&admin, &PauseType::Deposit, &false);
    assert!(!client.get_pause_state(&PauseType::Deposit));
}

/// Global All pause is reported as paused for every operation type.
#[test]
fn test_get_pause_state_global_all_returns_true_for_all_types() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_pause(&admin, &PauseType::All, &true);

    // All specific types appear paused when global is set.
    assert!(client.get_pause_state(&PauseType::Deposit));
    assert!(client.get_pause_state(&PauseType::Borrow));
    assert!(client.get_pause_state(&PauseType::Repay));
    assert!(client.get_pause_state(&PauseType::Withdraw));
    assert!(client.get_pause_state(&PauseType::Liquidation));
    assert!(client.get_pause_state(&PauseType::All));
}

// ═══════════════════════════════════════════════════════════════════════════
// Granular flag independence
// ═══════════════════════════════════════════════════════════════════════════

/// Pausing Borrow does not block Repay.
#[test]
fn test_borrow_pause_does_not_block_repay() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    // Give the user a debt to repay.
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);

    client.set_pause(&admin, &PauseType::Borrow, &true);

    // Repay must still succeed.
    client.repay(&user, &asset, &5_000);

    // Borrow is blocked.
    assert_eq!(
        client.try_borrow(&user, &asset, &1_000, &collateral, &2_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
}

/// Pausing Repay does not block Borrow.
#[test]
fn test_repay_pause_does_not_block_borrow() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_pause(&admin, &PauseType::Repay, &true);

    // Borrow must succeed.
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);

    // Repay is blocked.
    assert_eq!(
        client.try_repay(&user, &asset, &1_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
}

/// Pausing Liquidation does not affect Deposit, Borrow, Repay, or Withdraw.
#[test]
fn test_liquidation_pause_is_independent() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    client.set_pause(&admin, &PauseType::Liquidation, &true);

    // Other operations remain open.
    client.deposit(&user, &asset, &10_000);
    client.borrow(&user, &asset, &5_000, &collateral, &10_000);
    client.repay(&user, &asset, &1_000);
    client.withdraw(&user, &asset, &1_000);

    // Liquidation is blocked.
    assert_eq!(
        client.try_liquidate(&admin, &user, &asset, &collateral, &1_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
}

/// Pausing Deposit blocks deposit_collateral as well as deposit.
#[test]
fn test_deposit_pause_blocks_deposit_collateral() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_pause(&admin, &PauseType::Deposit, &true);

    assert_eq!(
        client.try_deposit(&user, &asset, &10_000),
        Err(Ok(DepositError::DepositPaused))
    );
    assert_eq!(
        client.try_deposit_collateral(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Multiple simultaneous pauses
// ═══════════════════════════════════════════════════════════════════════════

/// Multiple operations can be paused simultaneously and independently toggled.
#[test]
fn test_multiple_simultaneous_pauses() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_pause(&admin, &PauseType::Deposit, &true);
    client.set_pause(&admin, &PauseType::Borrow, &true);
    client.set_pause(&admin, &PauseType::Liquidation, &true);

    assert!(client.get_pause_state(&PauseType::Deposit));
    assert!(client.get_pause_state(&PauseType::Borrow));
    assert!(client.get_pause_state(&PauseType::Liquidation));
    assert!(!client.get_pause_state(&PauseType::Repay));
    assert!(!client.get_pause_state(&PauseType::Withdraw));

    // Unpausing one does not affect others.
    client.set_pause(&admin, &PauseType::Borrow, &false);
    assert!(!client.get_pause_state(&PauseType::Borrow));
    assert!(client.get_pause_state(&PauseType::Deposit));
    assert!(client.get_pause_state(&PauseType::Liquidation));

    // Borrow now works; deposit is still blocked.
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);
    assert_eq!(
        client.try_deposit(&user, &asset, &5_000),
        Err(Ok(DepositError::DepositPaused))
    );
}

/// Global All pause overrides individual operations that were explicitly unpaused.
#[test]
fn test_global_pause_overrides_individual_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    // Explicitly unpause individual flags (no-op since they start false, but
    // this tests that All overrides whatever the individual flag says).
    client.set_pause(&admin, &PauseType::Deposit, &false);
    client.set_pause(&admin, &PauseType::Borrow, &false);

    // Engage global pause.
    client.set_pause(&admin, &PauseType::All, &true);

    assert_eq!(
        client.try_deposit(&user, &asset, &10_000),
        Err(Ok(DepositError::DepositPaused))
    );
    assert_eq!(
        client.try_borrow(&user, &asset, &10_000, &collateral, &20_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_repay(&user, &asset, &10_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_withdraw(&user, &asset, &10_000),
        Err(Ok(WithdrawError::WithdrawPaused))
    );

    // Disengage global pause; individual flags are still false → all allowed.
    client.set_pause(&admin, &PauseType::All, &false);
    client.deposit(&user, &asset, &10_000);
    client.borrow(&user, &asset, &5_000, &collateral, &10_000);
}

/// Toggling a pause flag multiple times converges to the last value.
#[test]
fn test_pause_toggle_multiple_times() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    for _ in 0..5 {
        client.set_pause(&admin, &PauseType::Borrow, &true);
        client.set_pause(&admin, &PauseType::Borrow, &false);
    }

    // Final state is unpaused → borrow succeeds.
    assert!(!client.get_pause_state(&PauseType::Borrow));
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// Convenience wrappers (set_deposit_paused / set_withdraw_paused)
// ═══════════════════════════════════════════════════════════════════════════

/// set_deposit_paused emits a pause_event (uses unified system).
#[test]
fn test_set_deposit_paused_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_deposit_paused(&true);

    let events = env.events().all();
    let last = events.last().unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
    assert_eq!(topic, Symbol::new(&env, "pause_event"));

    // get_pause_state must reflect the change.
    assert!(client.get_pause_state(&PauseType::Deposit));
}

/// set_withdraw_paused emits a pause_event (uses unified system).
#[test]
fn test_set_withdraw_paused_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_withdraw_paused(&true);

    let events = env.events().all();
    let last = events.last().unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
    assert_eq!(topic, Symbol::new(&env, "pause_event"));

    assert!(client.get_pause_state(&PauseType::Withdraw));
}

/// set_deposit_paused blocks deposit when true.
#[test]
fn test_set_deposit_paused_blocks_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_deposit_paused(&true);

    assert_eq!(
        client.try_deposit(&user, &asset, &10_000),
        Err(Ok(DepositError::DepositPaused))
    );

    client.set_deposit_paused(&false);
    client.deposit(&user, &asset, &10_000);
}

/// set_withdraw_paused blocks withdraw when true.
#[test]
fn test_set_withdraw_paused_blocks_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);
    client.deposit(&user, &asset, &10_000);

    client.set_withdraw_paused(&true);

    assert_eq!(
        client.try_withdraw(&user, &asset, &1_000),
        Err(Ok(WithdrawError::WithdrawPaused))
    );

    client.set_withdraw_paused(&false);
    client.withdraw(&user, &asset, &1_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// Flash loan pause behaviour
// ═══════════════════════════════════════════════════════════════════════════

/// Flash loan is blocked by the global All pause.
#[test]
fn test_flash_loan_blocked_by_all_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_pause(&admin, &PauseType::All, &true);

    assert_eq!(
        client.try_flash_loan(&user, &asset, &1_000, &soroban_sdk::Bytes::new(&env)),
        Err(Ok(FlashLoanError::ProtocolPaused))
    );
}

/// Flash loan is NOT blocked by individual operation pauses (only by All).
#[test]
fn test_flash_loan_not_blocked_by_specific_pauses() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    // These individual pauses must NOT block flash loans.
    client.set_pause(&admin, &PauseType::Deposit, &true);
    client.set_pause(&admin, &PauseType::Borrow, &true);
    client.set_pause(&admin, &PauseType::Repay, &true);
    client.set_pause(&admin, &PauseType::Withdraw, &true);
    client.set_pause(&admin, &PauseType::Liquidation, &true);

    // Flash loan will fail for business reasons (invalid amount path / callback),
    // but the pause check itself must not trigger ProtocolPaused.
    let result = client.try_flash_loan(&user, &asset, &0, &soroban_sdk::Bytes::new(&env));
    assert_ne!(result, Err(Ok(FlashLoanError::ProtocolPaused)));
}

// ═══════════════════════════════════════════════════════════════════════════
// Guardian management
// ═══════════════════════════════════════════════════════════════════════════

/// get_guardian returns None before any guardian is configured.
#[test]
fn test_get_guardian_initially_none() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    assert_eq!(client.get_guardian(), None);
}

/// set_guardian stores the address and get_guardian returns it.
#[test]
fn test_set_guardian_and_get_guardian() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_guardian(&admin, &guardian);
    assert_eq!(client.get_guardian(), Some(guardian.clone()));

    // Rotating the guardian replaces the previous one.
    let new_guardian = Address::generate(&env);
    client.set_guardian(&admin, &new_guardian);
    assert_eq!(client.get_guardian(), Some(new_guardian));
}

/// set_guardian emits a guardian_set_event.
#[test]
fn test_set_guardian_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_guardian(&admin, &guardian);

    let events = env.events().all();
    let last = events.last().unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
    assert_eq!(topic, Symbol::new(&env, "guardian_set_event"));
}

/// A non-admin address cannot configure the guardian.
#[test]
#[should_panic(expected = "HostError: Error(Contract, #6)")]
fn test_non_admin_cannot_set_guardian() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.set_guardian(&user, &Address::generate(&env));
}

// ═══════════════════════════════════════════════════════════════════════════
// Emergency shutdown – authorization
// ═══════════════════════════════════════════════════════════════════════════

/// Admin (without a guardian configured) can trigger shutdown.
#[test]
fn test_admin_can_trigger_shutdown_without_guardian() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.emergency_shutdown(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);
}

/// Non-admin, non-guardian address cannot trigger shutdown.
#[test]
fn test_random_address_cannot_trigger_shutdown() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    // No guardian configured → only admin is allowed.
    assert_eq!(
        client.try_emergency_shutdown(&attacker),
        Err(Ok(BorrowError::Unauthorized))
    );
    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
}

/// Guardian cannot call set_pause (only admin can).
#[test]
fn test_guardian_cannot_set_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);

    // Guardian is not the admin → set_pause must fail.
    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::Borrow, &true),
        Err(Ok(BorrowError::Unauthorized))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Emergency state lifecycle
// ═══════════════════════════════════════════════════════════════════════════

/// start_recovery fails when the protocol is still in Normal state.
#[test]
fn test_start_recovery_fails_when_not_in_shutdown() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    assert_eq!(
        client.try_start_recovery(&admin),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
}

/// complete_recovery can be called from any state to return to Normal.
#[test]
fn test_complete_recovery_from_shutdown_state() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.emergency_shutdown(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);

    // Skip Recovery; go straight back to Normal.
    client.complete_recovery(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
}

/// emergency_shutdown emits an emergency_state_event.
#[test]
fn test_emergency_shutdown_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    client.emergency_shutdown(&admin);

    let events = env.events().all();
    let last = events.last().unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
    assert_eq!(topic, Symbol::new(&env, "emergency_state_event"));
}

/// Full lifecycle: Normal → Shutdown → Recovery → Normal.
/// Each transition emits an emergency_state_event; verified immediately after the call
/// before any subsequent invocation clears the event buffer.
#[test]
fn test_full_emergency_lifecycle_events() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);

    // Step 1: Shutdown
    // Read events immediately – no other client call may intervene.
    client.emergency_shutdown(&admin);
    {
        let events = env.events().all();
        let last = events.last().unwrap();
        let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
        assert_eq!(topic, Symbol::new(&env, "emergency_state_event"));
    }

    // Step 2: Recovery
    client.start_recovery(&admin);
    {
        let events = env.events().all();
        let last = events.last().unwrap();
        let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
        assert_eq!(topic, Symbol::new(&env, "emergency_state_event"));
    }

    // Step 3: Normal
    client.complete_recovery(&admin);
    {
        let events = env.events().all();
        let last = events.last().unwrap();
        let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
        assert_eq!(topic, Symbol::new(&env, "emergency_state_event"));
    }

    // Final state verification (separate read call is fine here).
    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
}

/// During Recovery, only unwind operations (repay / withdraw) are allowed;
/// new-risk operations (borrow / deposit) remain blocked.
#[test]
fn test_recovery_allows_unwind_blocks_new_risk() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    client.deposit(&user, &asset, &50_000);
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);

    client.emergency_shutdown(&guardian);
    client.start_recovery(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Recovery);

    // New-risk operations must fail.
    assert_eq!(
        client.try_borrow(&user, &asset, &1_000, &collateral, &2_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_deposit(&user, &asset, &1_000),
        Err(Ok(DepositError::DepositPaused))
    );

    // Unwind operations must succeed.
    client.repay(&user, &asset, &1_000);
    client.withdraw(&user, &asset, &1_000);
}

/// Granular pause on Repay still blocks repay even inside Recovery.
#[test]
fn test_granular_repay_pause_respected_in_recovery() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);
    client.deposit(&user, &asset, &50_000);
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);

    client.emergency_shutdown(&admin);
    client.start_recovery(&admin);

    // Repay granular pause takes precedence even in Recovery.
    client.set_pause(&admin, &PauseType::Repay, &true);
    assert_eq!(
        client.try_repay(&user, &asset, &1_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );

    client.set_pause(&admin, &PauseType::Repay, &false);
    client.repay(&user, &asset, &1_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-Asset Operations Pause Testing
// ═══════════════════════════════════════════════════════════════════════════

/// Cross-asset deposit is blocked by Deposit pause and global All pause.
#[test]
fn test_cross_asset_deposit_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_admin(&admin);

    // Test Deposit pause blocks cross-asset deposit
    client.set_pause(&admin, &PauseType::Deposit, &true);
    assert_eq!(
        client.try_deposit_collateral_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test All pause blocks cross-asset deposit
    client.set_pause(&admin, &PauseType::Deposit, &false);
    client.set_pause(&admin, &PauseType::All, &true);
    assert_eq!(
        client.try_deposit_collateral_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test unpause allows cross-asset deposit
    client.set_pause(&admin, &PauseType::All, &false);
    client.deposit_collateral_asset(&user, &asset, &10_000);
}

/// Cross-asset borrow is blocked by Borrow pause and global All pause.
#[test]
fn test_cross_asset_borrow_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_admin(&admin);

    // Test Borrow pause blocks cross-asset borrow
    client.set_pause(&admin, &PauseType::Borrow, &true);
    assert_eq!(
        client.try_borrow_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test All pause blocks cross-asset borrow
    client.set_pause(&admin, &PauseType::Borrow, &false);
    client.set_pause(&admin, &PauseType::All, &true);
    assert_eq!(
        client.try_borrow_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test unpause allows cross-asset borrow
    client.set_pause(&admin, &PauseType::All, &false);
    client.borrow_asset(&user, &asset, &10_000);
}

/// Cross-asset repay is blocked by Repay pause and global All pause (except in Recovery).
#[test]
fn test_cross_asset_repay_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_admin(&admin);

    // Test Repay pause blocks cross-asset repay
    client.set_pause(&admin, &PauseType::Repay, &true);
    assert_eq!(
        client.try_repay_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test All pause blocks cross-asset repay
    client.set_pause(&admin, &PauseType::Repay, &false);
    client.set_pause(&admin, &PauseType::All, &true);
    assert_eq!(
        client.try_repay_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test unpause allows cross-asset repay
    client.set_pause(&admin, &PauseType::All, &false);
    client.repay_asset(&user, &asset, &10_000);
}

/// Cross-asset withdraw is blocked by Withdraw pause and global All pause (except in Recovery).
#[test]
fn test_cross_asset_withdraw_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_admin(&admin);

    // Test Withdraw pause blocks cross-asset withdraw
    client.set_pause(&admin, &PauseType::Withdraw, &true);
    assert_eq!(
        client.try_withdraw_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test All pause blocks cross-asset withdraw
    client.set_pause(&admin, &PauseType::Withdraw, &false);
    client.set_pause(&admin, &PauseType::All, &true);
    assert_eq!(
        client.try_withdraw_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    // Test unpause allows cross-asset withdraw
    client.set_pause(&admin, &PauseType::All, &false);
    client.withdraw_asset(&user, &asset, &10_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// Oracle Operations Pause Testing
// ═══════════════════════════════════════════════════════════════════════════

/// Oracle price updates are blocked by dedicated oracle pause flag.
#[test]
fn test_oracle_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_oracle(&admin, &oracle);

    // Test oracle pause blocks price updates
    client.set_oracle_paused(&oracle, &true);
    assert_eq!(
        client.try_update_price_feed(&oracle, &asset, &100_000),
        Err(Ok(OracleError::OraclePaused))
    );

    // Test unpaused oracle allows price updates
    client.set_oracle_paused(&oracle, &false);
    client.update_price_feed(&oracle, &asset, &100_000);
}

/// Oracle pause is independent of other pause flags.
#[test]
fn test_oracle_pause_independence() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let asset = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_oracle(&admin, &oracle);

    // Pause all core operations but not oracle
    client.set_pause(&admin, &PauseType::All, &true);

    // Oracle should still work if not paused
    client.update_price_feed(&oracle, &asset, &100_000);

    // Now pause oracle specifically
    client.set_oracle_paused(&oracle, &true);
    assert_eq!(
        client.try_update_price_feed(&oracle, &asset, &200_000),
        Err(Ok(OracleError::OraclePaused))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Cases and Matrix Testing
// ═══════════════════════════════════════════════════════════════════════════

/// Zero amount operations should still be blocked by pause flags.
#[test]
fn test_zero_amount_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);

    // Pause operations
    client.set_pause(&admin, &PauseType::Deposit, &true);
    client.set_pause(&admin, &PauseType::Borrow, &true);
    client.set_pause(&admin, &PauseType::Repay, &true);
    client.set_pause(&admin, &PauseType::Withdraw, &true);

    // Zero amount operations should still fail with pause errors
    assert_eq!(
        client.try_deposit(&user, &asset, &0),
        Err(Ok(DepositError::DepositPaused))
    );
    assert_eq!(
        client.try_borrow(&user, &asset, &0, &collateral, &0),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_repay(&user, &asset, &0),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_withdraw(&user, &asset, &0),
        Err(Ok(WithdrawError::WithdrawPaused))
    );
}

/// Unauthorized callers cannot bypass pause by calling admin functions.
#[test]
fn test_unauthorized_pause_bypass_attempts() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let guardian = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);

    // Pause operations
    client.set_pause(&admin, &PauseType::Borrow, &true);
    client.set_pause(&admin, &PauseType::Deposit, &true);

    // Attacker cannot unpause operations
    assert_eq!(
        client.try_set_pause(&attacker, &PauseType::Borrow, &false),
        Err(Ok(BorrowError::Unauthorized))
    );
    assert_eq!(
        client.try_set_deposit_paused(&false),
        Err(Ok(DepositError::Unauthorized))
    );
    assert_eq!(
        client.try_set_withdraw_paused(&false),
        Err(Ok(WithdrawError::Unauthorized))
    );

    // Attacker cannot trigger emergency shutdown unless they are guardian
    assert_eq!(
        client.try_emergency_shutdown(&attacker),
        Err(Ok(BorrowError::Unauthorized))
    );

    // Guardian can trigger shutdown but cannot unpause
    client.emergency_shutdown(&guardian);
    assert_eq!(
        client.try_set_pause(&guardian, &PauseType::Borrow, &false),
        Err(Ok(BorrowError::Unauthorized))
    );
}

/// Comprehensive pause state matrix test.
#[test]
fn test_comprehensive_pause_state_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    // Matrix: Test each pause flag individually
    let pause_types = vec![
        (PauseType::Deposit, "deposit"),
        (PauseType::Borrow, "borrow"),
        (PauseType::Repay, "repay"),
        (PauseType::Withdraw, "withdraw"),
        (PauseType::Liquidation, "liquidation"),
    ];

    for (pause_type, operation) in pause_types {
        // Pause the specific operation
        client.set_pause(&admin, &pause_type, &true);

        // Verify get_pause_state reflects the change
        assert!(client.get_pause_state(&pause_type));

        // Test that other operations are not affected (except by All)
        match pause_type {
            PauseType::Deposit => {
                assert_eq!(
                    client.try_deposit(&user, &asset, &10_000),
                    Err(Ok(DepositError::DepositPaused))
                );
                // Other operations should work
                client.borrow(&user, &asset, &10_000, &collateral, &20_000);
                client.repay(&user, &asset, &1_000);
                client.withdraw(&user, &asset, &1_000);
            }
            PauseType::Borrow => {
                assert_eq!(
                    client.try_borrow(&user, &asset, &10_000, &collateral, &20_000),
                    Err(Ok(BorrowError::ProtocolPaused))
                );
                // Other operations should work
                client.deposit(&user, &asset, &10_000);
                client.repay(&user, &asset, &1_000);
                client.withdraw(&user, &asset, &1_000);
            }
            PauseType::Repay => {
                assert_eq!(
                    client.try_repay(&user, &asset, &10_000),
                    Err(Ok(BorrowError::ProtocolPaused))
                );
                // Other operations should work
                client.deposit(&user, &asset, &10_000);
                client.borrow(&user, &asset, &10_000, &collateral, &20_000);
                client.withdraw(&user, &asset, &1_000);
            }
            PauseType::Withdraw => {
                assert_eq!(
                    client.try_withdraw(&user, &asset, &10_000),
                    Err(Ok(WithdrawError::WithdrawPaused))
                );
                // Other operations should work
                client.deposit(&user, &asset, &10_000);
                client.borrow(&user, &asset, &10_000, &collateral, &20_000);
                client.repay(&user, &asset, &1_000);
            }
            PauseType::Liquidation => {
                assert_eq!(
                    client.try_liquidate(&admin, &user, &asset, &collateral, &10_000),
                    Err(Ok(BorrowError::ProtocolPaused))
                );
                // Other operations should work
                client.deposit(&user, &asset, &10_000);
                client.borrow(&user, &asset, &10_000, &collateral, &20_000);
                client.repay(&user, &asset, &1_000);
                client.withdraw(&user, &asset, &1_000);
            }
            _ => {}
        }

        // Unpause the operation
        client.set_pause(&admin, &pause_type, &false);
        assert!(!client.get_pause_state(&pause_type));
    }
}

/// Pause behavior during different emergency states.
#[test]
fn test_pause_during_emergency_states() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral = Address::generate(&env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_guardian(&admin, &guardian);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    // Setup user position
    client.deposit(&user, &asset, &50_000);
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);

    // Test pause behavior during Shutdown
    client.emergency_shutdown(&guardian);
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);

    // All operations should be blocked regardless of pause flags
    client.set_pause(&admin, &PauseType::Repay, &false); // Try to unpause
    assert_eq!(
        client.try_repay(&user, &asset, &1_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_withdraw(&user, &asset, &1_000),
        Err(Ok(WithdrawError::WithdrawPaused))
    );

    // Move to Recovery
    client.start_recovery(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Recovery);

    // In Recovery, repay and withdraw should work unless specifically paused
    client.repay(&user, &asset, &1_000);
    client.withdraw(&user, &asset, &1_000);

    // But new risk operations should still be blocked
    assert_eq!(
        client.try_borrow(&user, &asset, &1_000, &collateral, &2_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );
    assert_eq!(
        client.try_deposit(&user, &asset, &1_000),
        Err(Ok(DepositError::DepositPaused))
    );

    // Granular pause on repay/withdraw should still be respected in Recovery
    client.set_pause(&admin, &PauseType::Repay, &true);
    assert_eq!(
        client.try_repay(&user, &asset, &1_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );

    client.set_pause(&admin, &PauseType::Withdraw, &true);
    assert_eq!(
        client.try_withdraw(&user, &asset, &1_000),
        Err(Ok(WithdrawError::WithdrawPaused))
    );
}
