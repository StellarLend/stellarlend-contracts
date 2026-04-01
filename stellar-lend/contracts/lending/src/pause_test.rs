use crate::deposit::DepositError;
use crate::flash_loan::FlashLoanError;
use crate::testutils::create_token;
use crate::withdraw::WithdrawError;
use crate::pause::PauseType;
use crate::borrow::BorrowError;
use crate::emergency::EmergencyState;
use crate::oracle::OracleError;
use crate::cross_asset::CrossAssetError;
use crate::*;
use soroban_sdk::{testutils::{Address as _, Events}, token, Address, Env, Symbol, TryFromVal, Vec};

fn setup_pause_test(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address,
    Address,
    Address,
    token::StellarAssetClient<'_>,
    token::StellarAssetClient<'_>,
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let (asset, asset_client) = create_token(env, &admin);
    let (collateral, collateral_client) = create_token(env, &admin);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    (
        client,
        admin,
        asset,
        collateral,
        asset_client,
        collateral_client,
    )
}

#[test]
fn test_pause_borrow_granular() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, collateral, _, collateral_client) = setup_pause_test(&env);
    let user = Address::generate(&env);

    collateral_client.mint(&user, &20_000);
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);

    client.set_pause(&admin, &PauseType::Borrow, &true);
    let result = client.try_borrow(&user, &asset, &10_000, &collateral, &20_000);
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));
}

#[test]
fn test_global_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, collateral, _asset_client, _collateral_client) =
        setup_pause_test(&env);
    let user = Address::generate(&env);

    client.set_pause(&admin, &PauseType::All, &true);

    assert_eq!(
        client.try_borrow(&user, &asset, &10_000, &collateral, &20_000),
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
}

#[test]
fn test_all_granular_pauses() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, collateral, _, collateral_client) = setup_pause_test(&env);
    let user = Address::generate(&env);

    client.set_pause(&admin, &PauseType::Deposit, &true);
    assert_eq!(
        client.try_deposit(&user, &asset, &10_000),
        Err(Ok(DepositError::DepositPaused))
    );

    collateral_client.mint(&user, &20_000);
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);
}

#[test]
fn test_get_pause_state_default_false() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _, _, _, _) = setup_pause_test(&env);

    assert!(!client.get_pause_state(&PauseType::Deposit));
    assert!(!client.get_pause_state(&PauseType::All));
}

#[test]
fn test_set_deposit_paused_blocks_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset, _, asset_client, _) = setup_pause_test(&env);
    let user = Address::generate(&env);

    client.set_deposit_paused(&true);
    assert_eq!(
        client.try_deposit(&user, &asset, &10_000),
        Err(Ok(DepositError::DepositPaused))
    );

    client.set_deposit_paused(&false);
    asset_client.mint(&user, &10_000);
    client.deposit(&user, &asset, &10_000);
}

#[test]
fn test_set_withdraw_paused_blocks_withdraw() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, asset, _, asset_client, _) = setup_pause_test(&env);
    let user = Address::generate(&env);

    asset_client.mint(&user, &10_000);
    client.deposit(&user, &asset, &10_000);

    client.set_withdraw_paused(&true);
    assert_eq!(
        client.try_withdraw(&user, &asset, &1_000),
        Err(Ok(WithdrawError::WithdrawPaused))
    );

    client.set_withdraw_paused(&false);
    client.withdraw(&user, &asset, &1_000);
}

#[test]
fn test_flash_loan_blocked_by_all_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _, _, _) = setup_pause_test(&env);
    let user = Address::generate(&env);

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
    let (client, admin, asset, _, _, _) = setup_pause_test(&env);
    let user = Address::generate(&env);

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
    let (client, _admin, _, _, _, _) = setup_pause_test(&env);

    assert_eq!(client.get_guardian(), None);
}

/// set_guardian stores the address and get_guardian returns it.
#[test]
fn test_set_guardian_and_get_guardian() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _, _, _, _) = setup_pause_test(&env);
    let guardian = Address::generate(&env);

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
    let (client, admin, _, _, _, _) = setup_pause_test(&env);
    let guardian = Address::generate(&env);

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
    let (client, _admin, _, _, _, _) = setup_pause_test(&env);
    let user = Address::generate(&env);

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
    let (client, admin, _, _, _, _) = setup_pause_test(&env);

    client.emergency_shutdown(&admin);
    assert_eq!(client.get_emergency_state(), EmergencyState::Shutdown);
}

/// Non-admin, non-guardian address cannot trigger shutdown.
#[test]
fn test_random_address_cannot_trigger_shutdown() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _, _, _, _) = setup_pause_test(&env);
    let attacker = Address::generate(&env);

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
    let (client, admin, _, _, _, _) = setup_pause_test(&env);
    let guardian = Address::generate(&env);
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
    let (client, admin, _, _, _, _) = setup_pause_test(&env);

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
    let (client, admin, _, _, _, _) = setup_pause_test(&env);

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
    let (client, admin, _, _, _, _) = setup_pause_test(&env);

    client.emergency_shutdown(&admin);

    let events = env.events().all();
    let last = events.last().unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &last.1.get(0).unwrap()).unwrap();
    assert_eq!(topic, Symbol::new(&env, "emergency_state_event"));
}

/// Full lifecycle: Normal → Shutdown → Recovery → Normal.
#[test]
fn test_full_emergency_lifecycle_events() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _, _, _, _) = setup_pause_test(&env);

    // Step 1: Shutdown
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

    assert_eq!(client.get_emergency_state(), EmergencyState::Normal);
}

/// During Recovery, only unwind operations (repay / withdraw) are allowed;
/// new-risk operations (borrow / deposit) remain blocked.
#[test]
fn test_recovery_allows_unwind_blocks_new_risk() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, collateral, _, _) = setup_pause_test(&env);
    let guardian = Address::generate(&env);
    let user = Address::generate(&env);

    client.set_guardian(&admin, &guardian);
    
    // Setup initial positions
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
    let (client, admin, asset, collateral, _, _) = setup_pause_test(&env);
    let user = Address::generate(&env);

    client.deposit(&user, &asset, &50_000);
    client.borrow(&user, &asset, &10_000, &collateral, &20_000);

    client.emergency_shutdown(&admin);
    client.start_recovery(&admin);

    client.set_pause(&admin, &PauseType::Repay, &true);
    assert_eq!(
        client.try_repay(&user, &asset, &1_000),
        Err(Ok(BorrowError::ProtocolPaused))
    );

    client.set_pause(&admin, &PauseType::Repay, &false);
    client.repay(&user, &asset, &1_000);
}

#[test]
fn test_cross_asset_deposit_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _, _, _) = setup_pause_test(&env);
    let user = Address::generate(&env);

    client.set_pause(&admin, &PauseType::Deposit, &true);
    assert_eq!(
        client.try_deposit_collateral_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );

    client.set_pause(&admin, &PauseType::Deposit, &false);
    client.set_pause(&admin, &PauseType::All, &true);
    assert_eq!(
        client.try_deposit_collateral_asset(&user, &asset, &10_000),
        Err(Ok(CrossAssetError::ProtocolPaused))
    );
}

#[test]
fn test_oracle_pause_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, _, _, _) = setup_pause_test(&env);
    let oracle = Address::generate(&env);

    client.set_oracle(&admin, &oracle);

    client.set_oracle_paused(&oracle, &true);
    assert_eq!(
        client.try_update_price_feed(&oracle, &asset, &100_000),
        Err(Ok(OracleError::OraclePaused))
    );

    client.set_oracle_paused(&oracle, &false);
    client.update_price_feed(&oracle, &asset, &100_000);
}
