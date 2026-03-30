use crate::deposit::DepositError;
use crate::flash_loan::FlashLoanError;
use crate::testutils::create_token;
use crate::withdraw::WithdrawError;
use crate::*;
use soroban_sdk::{
    testutils::{Address as _, Events},
    token, Address, Env, Symbol, TryFromVal,
};

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
    let (client, admin, asset, collateral, asset_client, collateral_client) =
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
    let (client, admin, _, _, _, _) = setup_pause_test(&env);

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
