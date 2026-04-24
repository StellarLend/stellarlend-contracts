//! # Zero-Amount Semantics Tests
//!
//! Locks the expected behavior of every public entrypoint when called with
//! `amount == 0` or `amount < 0`. These tests are the machine-readable
//! complement to `docs/ZERO_AMOUNT_SEMANTICS.md`.
//!
//! ## Policy
//! All monetary entrypoints **reject** zero/negative amounts with a typed
//! `InvalidAmount` error variant. No state mutation occurs on rejection.
//!
//! Reference: docs/ZERO_AMOUNT_SEMANTICS.md

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

// ─────────────────────────────────────────────────────────────────────────────
// Shared setup helpers
// ─────────────────────────────────────────────────────────────────────────────

fn setup(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let user = Address::generate(env);
    let asset = Address::generate(env);
    let collateral_asset = Address::generate(env);

    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_deposit_settings(&1_000_000_000, &0);
    client.initialize_withdraw_settings(&0);

    (client, admin, user, asset, collateral_asset)
}

fn setup_cross(env: &Env) -> (LendingContractClient<'_>, Address, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let user = Address::generate(env);
    let asset = Address::generate(env);

    client.initialize_admin(&admin);
    client.set_asset_params(
        &asset,
        &AssetParams {
            ltv: 8000,
            liquidation_threshold: 8500,
            price_feed: Address::generate(env),
            debt_ceiling: 1_000_000_000,
            is_active: true,
        },
    );

    (client, admin, user, asset)
}

// ═════════════════════════════════════════════════════════════════════════════
// 1. deposit
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_deposit_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _col) = setup(&env);

    let result = client.try_deposit(&user, &asset, &0);
    assert_eq!(result, Err(Ok(DepositError::InvalidAmount)));
}

#[test]
fn test_deposit_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _col) = setup(&env);

    let result = client.try_deposit(&user, &asset, &-1);
    assert_eq!(result, Err(Ok(DepositError::InvalidAmount)));
}

#[test]
fn test_deposit_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _col) = setup(&env);

    client.deposit(&user, &asset, &5_000);
    let before = client.get_user_collateral_deposit(&user, &asset);

    let _ = client.try_deposit(&user, &asset, &0);

    let after = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        before.amount, after.amount,
        "state must not change on zero deposit"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 2. deposit_collateral (borrow module path)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_deposit_collateral_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, collateral_asset) = setup(&env);

    let result = client.try_deposit_collateral(&user, &collateral_asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_deposit_collateral_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, collateral_asset) = setup(&env);

    let result = client.try_deposit_collateral(&user, &collateral_asset, &-100);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_deposit_collateral_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, collateral_asset) = setup(&env);

    client.deposit_collateral(&user, &collateral_asset, &10_000);
    let before = client.get_user_collateral(&user);

    let _ = client.try_deposit_collateral(&user, &collateral_asset, &0);

    let after = client.get_user_collateral(&user);
    assert_eq!(
        before.amount, after.amount,
        "state must not change on zero deposit_collateral"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 3. withdraw
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_withdraw_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _col) = setup(&env);

    client.deposit(&user, &asset, &10_000);

    let result = client.try_withdraw(&user, &asset, &0);
    assert_eq!(result, Err(Ok(WithdrawError::InvalidAmount)));
}

#[test]
fn test_withdraw_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _col) = setup(&env);

    client.deposit(&user, &asset, &10_000);

    let result = client.try_withdraw(&user, &asset, &-500);
    assert_eq!(result, Err(Ok(WithdrawError::InvalidAmount)));
}

#[test]
fn test_withdraw_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, _col) = setup(&env);

    client.deposit(&user, &asset, &10_000);
    let before = client.get_user_collateral_deposit(&user, &asset);

    let _ = client.try_withdraw(&user, &asset, &0);

    let after = client.get_user_collateral_deposit(&user, &asset);
    assert_eq!(
        before.amount, after.amount,
        "state must not change on zero withdraw"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 4. borrow
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_borrow_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    let result = client.try_borrow(&user, &asset, &0, &collateral_asset, &20_000);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_borrow_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    let result = client.try_borrow(&user, &asset, &-1, &collateral_asset, &20_000);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_borrow_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    let before = client.get_user_debt(&user);

    let _ = client.try_borrow(&user, &asset, &0, &collateral_asset, &20_000);

    let after = client.get_user_debt(&user);
    assert_eq!(
        before.borrowed_amount, after.borrowed_amount,
        "debt must not change on zero borrow"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 5. repay
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_repay_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    client.borrow(&user, &asset, &5_000, &collateral_asset, &15_000);

    let result = client.try_repay(&user, &asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_repay_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    client.borrow(&user, &asset, &5_000, &collateral_asset, &15_000);

    let result = client.try_repay(&user, &asset, &-100);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_repay_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    client.borrow(&user, &asset, &5_000, &collateral_asset, &15_000);
    let before = client.get_user_debt(&user);

    let _ = client.try_repay(&user, &asset, &0);

    let after = client.get_user_debt(&user);
    assert_eq!(
        before.borrowed_amount, after.borrowed_amount,
        "debt must not change on zero repay"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 6. liquidate
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_liquidate_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    let liquidator = Address::generate(&env);
    client.borrow(&user, &asset, &5_000, &collateral_asset, &15_000);

    let result = client.try_liquidate(&liquidator, &user, &asset, &collateral_asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_liquidate_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    let liquidator = Address::generate(&env);
    client.borrow(&user, &asset, &5_000, &collateral_asset, &15_000);

    let result = client.try_liquidate(&liquidator, &user, &asset, &collateral_asset, &-1);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_liquidate_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);

    let liquidator = Address::generate(&env);
    client.borrow(&user, &asset, &5_000, &collateral_asset, &15_000);
    let before = client.get_user_debt(&user);

    let _ = client.try_liquidate(&liquidator, &user, &asset, &collateral_asset, &0);

    let after = client.get_user_debt(&user);
    assert_eq!(
        before.borrowed_amount, after.borrowed_amount,
        "debt must not change on zero liquidate"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 7. credit_insurance_fund (admin)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_credit_insurance_fund_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, asset, _col) = setup(&env);

    let result = client.try_credit_insurance_fund(&admin, &asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_credit_insurance_fund_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, asset, _col) = setup(&env);

    let result = client.try_credit_insurance_fund(&admin, &asset, &-500);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_credit_insurance_fund_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, asset, _col) = setup(&env);

    client.credit_insurance_fund(&admin, &asset, &10_000);
    let before = client.get_insurance_fund_balance(&asset);

    let _ = client.try_credit_insurance_fund(&admin, &asset, &0);

    let after = client.get_insurance_fund_balance(&asset);
    assert_eq!(
        before, after,
        "insurance fund must not change on zero credit"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 8. offset_bad_debt (admin)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_offset_bad_debt_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, asset, _col) = setup(&env);

    let result = client.try_offset_bad_debt(&admin, &asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_offset_bad_debt_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, asset, _col) = setup(&env);

    let result = client.try_offset_bad_debt(&admin, &asset, &-1);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ═════════════════════════════════════════════════════════════════════════════
// 9. Cross-asset: deposit_collateral_asset
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cross_deposit_collateral_asset_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    let result = client.try_deposit_collateral_asset(&user, &asset, &0);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_deposit_collateral_asset_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    let result = client.try_deposit_collateral_asset(&user, &asset, &-100);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_deposit_collateral_asset_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &10_000);
    let before = client.get_cross_position_summary(&user);

    let _ = client.try_deposit_collateral_asset(&user, &asset, &0);

    let after = client.get_cross_position_summary(&user);
    assert_eq!(
        before.total_collateral_usd, after.total_collateral_usd,
        "cross collateral must not change on zero deposit"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 10. Cross-asset: borrow_asset
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cross_borrow_asset_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &100_000);

    let result = client.try_borrow_asset(&user, &asset, &0);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_borrow_asset_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &100_000);

    let result = client.try_borrow_asset(&user, &asset, &-1);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_borrow_asset_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &100_000);
    let before = client.get_cross_position_summary(&user);

    let _ = client.try_borrow_asset(&user, &asset, &0);

    let after = client.get_cross_position_summary(&user);
    assert_eq!(
        before.total_debt_usd, after.total_debt_usd,
        "cross debt must not change on zero borrow"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 11. Cross-asset: repay_asset
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cross_repay_asset_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &100_000);
    client.borrow_asset(&user, &asset, &5_000);

    let result = client.try_repay_asset(&user, &asset, &0);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_repay_asset_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &100_000);
    client.borrow_asset(&user, &asset, &5_000);

    let result = client.try_repay_asset(&user, &asset, &-1);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_repay_asset_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &100_000);
    client.borrow_asset(&user, &asset, &5_000);
    let before = client.get_cross_position_summary(&user);

    let _ = client.try_repay_asset(&user, &asset, &0);

    let after = client.get_cross_position_summary(&user);
    assert_eq!(
        before.total_debt_usd, after.total_debt_usd,
        "cross debt must not change on zero repay"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 12. Cross-asset: withdraw_asset
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_cross_withdraw_asset_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &10_000);

    let result = client.try_withdraw_asset(&user, &asset, &0);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_withdraw_asset_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &10_000);

    let result = client.try_withdraw_asset(&user, &asset, &-1);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_cross_withdraw_asset_zero_does_not_mutate_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset) = setup_cross(&env);

    client.deposit_collateral_asset(&user, &asset, &10_000);
    let before = client.get_cross_position_summary(&user);

    let _ = client.try_withdraw_asset(&user, &asset, &0);

    let after = client.get_cross_position_summary(&user);
    assert_eq!(
        before.total_collateral_usd, after.total_collateral_usd,
        "cross collateral must not change on zero withdraw"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 13. Admin config: set_liquidation_threshold_bps
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_set_liquidation_threshold_bps_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _col) = setup(&env);

    let result = client.try_set_liquidation_threshold_bps(&admin, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_set_liquidation_threshold_bps_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _col) = setup(&env);

    let result = client.try_set_liquidation_threshold_bps(&admin, &-1);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ═════════════════════════════════════════════════════════════════════════════
// 14. Admin config: set_close_factor_bps
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_set_close_factor_bps_zero_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _col) = setup(&env);

    let result = client.try_set_close_factor_bps(&admin, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

#[test]
fn test_set_close_factor_bps_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _col) = setup(&env);

    let result = client.try_set_close_factor_bps(&admin, &-1);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ═════════════════════════════════════════════════════════════════════════════
// 15. set_flash_loan_fee_bps — zero IS valid (fee-free loans allowed by design)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_set_flash_loan_fee_bps_zero_is_valid() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _col) = setup(&env);

    let result = client.try_set_flash_loan_fee_bps(&0);
    assert!(
        result.is_ok(),
        "zero fee_bps must be accepted for flash loans"
    );
}

#[test]
fn test_set_flash_loan_fee_bps_negative_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _col) = setup(&env);

    let result = client.try_set_flash_loan_fee_bps(&-1);
    assert_eq!(result, Err(Ok(FlashLoanError::InvalidFee)));
}

// ═════════════════════════════════════════════════════════════════════════════
// 16. View helpers with zero input (must not trap or corrupt state)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_get_liquidation_incentive_amount_zero_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _col) = setup(&env);

    let result = client.get_liquidation_incentive_amount(&0);
    assert_eq!(result, 0);
}

#[test]
fn test_get_max_liquidatable_amount_no_position_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _col) = setup(&env);

    let result = client.get_max_liquidatable_amount(&user);
    assert_eq!(result, 0);
}
