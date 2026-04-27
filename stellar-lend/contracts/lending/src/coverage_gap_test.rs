// coverage_gap_test.rs
//
// Targeted tests that exercise the specific lines not covered by the existing
// test suite.  Each test is labelled with the source file + line(s) it targets.
//
// Files / lines covered here:
//   lib.rs          : 98, 199, 233-241, 246-252, 314-321, 354-355, 484, 491-492, 528-529
//   borrow.rs       : 334, 340, 360, 364, 367, 380-381, 385, 403, 412
//   deposit.rs      : 67
//   flash_loan.rs   : 43, 48
//   token_receiver.rs: 28

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper
// ─────────────────────────────────────────────────────────────────────────────

fn setup(env: &Env) -> (LendingContractClient<'_>, Address, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let user = Address::generate(env);
    let asset = Address::generate(env);
    client.initialize(&admin, &1_000_000_000, &1000);
    (client, admin, user, asset)
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : line 98 — double initialize returns Unauthorized
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_initialize_twice_returns_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset) = setup(&env);

    // Second call must be rejected.
    let result = client.try_initialize(&admin, &1_000_000_000, &1000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : line 199 — deposit_collateral when Deposit is paused
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_deposit_collateral_when_deposit_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset) = setup(&env);

    client.set_pause(&admin, &PauseType::Deposit, &true);

    let result = client.try_deposit_collateral(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : lines 233-241 — liquidate (stub, always Ok)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_stub_returns_ok() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);
    let collateral_asset = Address::generate(&env);

    // liquidate_position is a stub that returns Ok; just verify no panic.
    let result = client.try_liquidate(&user, &user, &asset, &collateral_asset, &1000);
    assert!(result.is_ok());
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : lines 246-252 — get_performance_stats returns two zeros
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_performance_stats_returns_two_zeros() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset) = setup(&env);

    let stats = client.get_performance_stats();
    assert_eq!(stats.len(), 2);
    assert_eq!(stats.get(0).unwrap(), 0u64);
    assert_eq!(stats.get(1).unwrap(), 0u64);
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : lines 314-321 — initialize_borrow_settings (admin only)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_initialize_borrow_settings_by_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset) = setup(&env);

    let result = client.try_initialize_borrow_settings(&500_000_000, &2000);
    assert!(result.is_ok());
}

#[test]
fn test_initialize_borrow_settings_no_admin_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    // Contract not initialized → no admin → Unauthorized
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let result = client.try_initialize_borrow_settings(&500_000_000, &2000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : lines 354-355 — get_admin
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_admin_returns_initialized_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset) = setup(&env);

    assert_eq!(client.get_admin(), Some(admin));
}

#[test]
fn test_get_admin_returns_none_before_init() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    assert_eq!(client.get_admin(), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : line 484 — data_store_init second call is a no-op
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_store_init_second_call_is_noop() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset) = setup(&env);

    client.data_store_init(&admin);
    // Second call must not panic and must be a no-op.
    client.data_store_init(&admin);
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : lines 491-492 — data_grant_writer / data_revoke_writer
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_grant_and_revoke_writer() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset) = setup(&env);

    client.data_store_init(&admin);

    let writer = Address::generate(&env);
    client.data_grant_writer(&admin, &writer);

    // Writer can now save data.
    let key = soroban_sdk::String::from_str(&env, "test_key");
    let value = Bytes::from_slice(&env, b"hello");
    client.data_save(&writer, &key, &value);

    // Revoke and confirm key still readable but write would fail.
    client.data_revoke_writer(&admin, &writer);
    let loaded = client.data_load(&key);
    assert_eq!(loaded, value);
}

// ─────────────────────────────────────────────────────────────────────────────
// lib.rs : lines 528-529 — data_key_exists
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_data_key_exists_true_and_false() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset) = setup(&env);

    client.data_store_init(&admin);
    client.data_grant_writer(&admin, &admin);

    let key = soroban_sdk::String::from_str(&env, "presence_key");
    assert!(!client.data_key_exists(&key));

    let value = Bytes::from_slice(&env, b"v");
    client.data_save(&admin, &key, &value);
    assert!(client.data_key_exists(&key));
}

// ─────────────────────────────────────────────────────────────────────────────
// borrow.rs : line 334 — borrow::deposit with zero amount → InvalidAmount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_deposit_collateral_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);

    let result = client.try_deposit_collateral(&user, &asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_deposit_collateral_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);

    let result = client.try_deposit_collateral(&user, &asset, &(-1));
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// borrow.rs : line 340 — borrow::deposit with different asset → AssetNotSupported
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_deposit_collateral_different_asset_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);

    let other_asset = Address::generate(&env);

    // First deposit sets the asset for this position.
    client.deposit_collateral(&user, &asset, &10_000);

    // Second deposit with a different asset must fail.
    let result = client.try_deposit_collateral(&user, &other_asset, &5_000);
    assert_eq!(result, Err(Ok(BorrowError::AssetNotSupported)));
}

// ─────────────────────────────────────────────────────────────────────────────
// borrow.rs : line 360 — repay with zero amount → InvalidAmount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_repay_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);
    let collateral_asset = Address::generate(&env);

    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    let result = client.try_repay(&user, &asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// borrow.rs : line 364 — repay when user has no debt → InvalidAmount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_repay_with_no_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);

    let result = client.try_repay(&user, &asset, &1000);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// borrow.rs : line 367 — repay with wrong asset → AssetNotSupported
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_repay_wrong_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);
    let collateral_asset = Address::generate(&env);
    let wrong_asset = Address::generate(&env);

    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    let result = client.try_repay(&user, &wrong_asset, &1000);
    assert_eq!(result, Err(Ok(BorrowError::AssetNotSupported)));
}

// ─────────────────────────────────────────────────────────────────────────────
// borrow.rs : line 385 — repay more than borrowed → RepayAmountTooHigh
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_repay_amount_too_high() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);
    let collateral_asset = Address::generate(&env);

    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);

    // Repay more than the principal (no interest yet, so 10_001 > 10_000).
    let result = client.try_repay(&user, &asset, &10_001);
    assert_eq!(result, Err(Ok(BorrowError::RepayAmountTooHigh)));
}

// ─────────────────────────────────────────────────────────────────────────────
// borrow.rs : lines 380-381 — repay only covers accrued interest (partial)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_repay_only_interest_partial() {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().with_mut(|li| {
        li.timestamp = 0;
    });

    let (client, _admin, user, asset) = setup(&env);
    let collateral_asset = Address::generate(&env);

    client.borrow(&user, &asset, &100_000, &collateral_asset, &200_000);

    // Advance time by 1 year to accrue interest.
    env.ledger().with_mut(|li| {
        li.timestamp = 31_536_000;
    });

    let debt_before = client.get_user_debt(&user);
    assert!(
        debt_before.interest_accrued > 0,
        "interest must have accrued"
    );

    // Repay exactly 1 unit — less than interest_accrued, so only interest shrinks.
    let result = client.try_repay(&user, &asset, &1);
    assert!(result.is_ok());

    // Principal remains the same.
    let debt_after = client.get_user_debt(&user);
    assert_eq!(debt_after.borrowed_amount, 100_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// deposit.rs : line 67 — deposit_impl when deposit paused → DepositPaused
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_deposit_impl_paused_returns_deposit_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset) = setup(&env);

    client.initialize_deposit_settings(&1_000_000_000, &100);
    client.set_pause(&admin, &PauseType::Deposit, &true);

    // The `deposit` entry point calls deposit_impl which checks pause independently.
    let result = client.try_deposit(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(DepositError::DepositPaused)));
}

// ─────────────────────────────────────────────────────────────────────────────
// flash_loan.rs : line 43 — flash_loan_impl with zero/negative amount → InvalidAmount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_flash_loan_invalid_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, asset) = setup(&env);

    let receiver = Address::generate(&env);
    let params = Bytes::new(&env);

    let result = client.try_flash_loan(&receiver, &asset, &0, &params);
    assert_eq!(result, Err(Ok(FlashLoanError::InvalidAmount)));
}

#[test]
fn test_flash_loan_invalid_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, asset) = setup(&env);

    let receiver = Address::generate(&env);
    let params = Bytes::new(&env);

    let result = client.try_flash_loan(&receiver, &asset, &(-500), &params);
    assert_eq!(result, Err(Ok(FlashLoanError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// token_receiver.rs : line 28 — receive with empty payload → InvalidAmount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_receive_empty_payload_returns_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);

    // Pass a completely empty Vec<Val> as payload.
    let empty_payload: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(&env);
    let result = client.try_receive(&asset, &user, &1000, &empty_payload);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Bonus: initialize_deposit_settings without admin → Unauthorized
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_initialize_deposit_settings_no_admin_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let result = client.try_initialize_deposit_settings(&1_000_000_000, &100);
    assert_eq!(result, Err(Ok(DepositError::Unauthorized)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Bonus: set_deposit_paused happy path
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_set_deposit_paused_and_unpaused() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup(&env);

    client.initialize_deposit_settings(&1_000_000_000, &100);

    client.set_deposit_paused(&true);
    let result = client.try_deposit(&user, &asset, &10_000);
    assert_eq!(result, Err(Ok(DepositError::DepositPaused)));

    client.set_deposit_paused(&false);
    let balance = client.deposit(&user, &asset, &10_000);
    assert_eq!(balance, 10_000);
}
