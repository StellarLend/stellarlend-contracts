use crate::borrow::BorrowError;
use crate::{LendingContract, LendingContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_borrow_coverage_booster() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let rando = Address::generate(&env);
    let oracle = Address::generate(&env);

    let id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &id);

    // 1. Initialize (covers set_admin and initialize_borrow_settings)
    // Synchronous call returns () and panics on Err results.
    client.initialize(&admin, &1_000_000, &100);

    // 2. Set Oracle (Unauthorized)
    // try_ methods return Result<Result<T, E>, ConversionError>.
    // Contract errors are wrapped in Err(Ok(E)).
    let result = client.try_set_oracle(&rando, &oracle);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));

    // 3. Set Oracle (Success)
    client.set_oracle(&admin, &oracle);

    // 4. Set Liquidation Threshold (Unauthorized)
    let result = client.try_set_liquidation_threshold_bps(&rando, &8000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));

    // 5. Set Liquidation Threshold (Invalid)
    let result = client.try_set_liquidation_threshold_bps(&admin, &10001);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));

    let result = client.try_set_liquidation_threshold_bps(&admin, &-1);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));

    // 6. Set Liquidation Threshold (Success)
    client.set_liquidation_threshold_bps(&admin, &9000);

    // 7. Initialize Settings (Explicit call to cover re-initialization)
    client.initialize_borrow_settings(&1_000_000, &100);

    // 8. Liquidation Coverage (Profiling entry point)
    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let asset = Address::generate(&env);
    let _ = client.try_liquidate(&liquidator, &borrower, &asset, &asset, &100);
}

#[test]
fn test_management_coverage_booster() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let guardian = Address::generate(&env);
    let asset = Address::generate(&env);
    let id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &id);

    client.initialize(&admin, &1_000_000, &100);

    // 1. Pause Management
    client.set_pause(&admin, &crate::pause::PauseType::Borrow, &true);
    client.set_pause(&admin, &crate::pause::PauseType::Borrow, &false);

    // 2. Emergency Lifecycle
    client.set_guardian(&admin, &guardian);
    client.emergency_shutdown(&guardian);
    client.start_recovery(&admin);
    client.complete_recovery(&admin);

    // 3. Settings Initialization
    client.initialize_withdraw_settings(&100);
    client.set_withdraw_paused(&true);
    client.set_withdraw_paused(&false);

    client.initialize_deposit_settings(&1_000_000, &100);
    client.set_deposit_paused(&true);
    client.set_deposit_paused(&false);

    // 4. Misc Coverage
    client.get_performance_stats();
    client.get_emergency_state();
    client.get_guardian();
    client.get_user_collateral(&admin);
    client.get_user_debt(&admin);
    client.get_user_collateral_deposit(&admin, &asset);

    // 5. Flash Loan Coverage (Error path while shutdown)
    client.emergency_shutdown(&admin);
    let result = client.try_flash_loan(&admin, &asset, &100, &soroban_sdk::Bytes::new(&env));
    assert!(result.is_err());
}
