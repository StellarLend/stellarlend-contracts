//! # Liquidate Module Tests — Issue #523
//!
//! Tests for `liquidate_position`: partial and full liquidations, post-health
//! invariants, edge cases (zero amounts, overflow, paused protocol), and
//! authorization.
//!
//! ## Coverage targets
//! - Happy-path partial & full liquidation
//! - Post-liquidation health: remaining_debt == 0 => HF == HEALTH_FACTOR_NO_DEBT
//! - Close-factor capping
//! - Collateral-seizure incentive calculation
//! - Collateral capped at available balance
//! - Oracle absent => rejected
//! - Pause state => rejected
//! - Asset mismatch => rejected
//! - Zero / negative amount => rejected
//! - Sequential liquidations converge
//! - Global total debt decremented
//! - Accrued interest included in liquidatable debt

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use views::HEALTH_FACTOR_SCALE;

// ─────────────────────────────────────────────────────────────────────────────
// Mock oracle — price = 1.0 (100_000_000 with 8 decimals)
// ─────────────────────────────────────────────────────────────────────────────

#[contract]
pub struct LiqMockOracle;

#[contractimpl]
impl LiqMockOracle {
    pub fn price(_env: Env, _asset: Address) -> i128 {
        100_000_000
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Initialize contract, set oracle, set liquidation threshold to 40%
/// (so a 150%-collateralised position is under-water).
fn setup_liquidatable(
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

    // Register oracle
    let oracle_id = env.register(LiqMockOracle, ());
    client.set_oracle(&admin, &oracle_id);

    // Threshold 40%: with 150% collateral the HF = 0.6 < 1.0
    client.set_liquidation_threshold_bps(&admin, &4000);

    (client, admin, user, asset, collateral_asset)
}

/// Create a liquidatable position: borrow 10_000 with 15_000 collateral.
/// Health factor = 15_000 * 0.40 * 10_000 / 10_000 = 6_000 < 10_000.
fn create_underwater_position(
    client: &LendingContractClient<'_>,
    user: &Address,
    asset: &Address,
    collateral_asset: &Address,
) {
    client.borrow(user, asset, &10_000, collateral_asset, &15_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// Happy-path: partial liquidation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_partial_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let liquidator = Address::generate(&env);
    // Repay 3_000 (within 50% close factor cap of 5_000)
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &3_000);

    let debt = client.get_user_debt(&borrower);
    let remaining = debt.borrowed_amount + debt.interest_accrued;
    assert!(remaining < 10_000, "debt must have decreased");
}

#[test]
fn test_liquidate_partial_debt_reduced_correctly() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let liquidator = Address::generate(&env);
    // Repay exactly 5_000 (= default 50% close factor of 10_000 debt)
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);

    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 5_000);
    assert_eq!(debt.interest_accrued, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Happy-path: full repay (close factor = 100%)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_full_repay_clears_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    // Allow full closure
    client.set_close_factor_bps(&admin, &10_000);
    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);

    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 0);
    assert_eq!(debt.interest_accrued, 0);
}

#[test]
fn test_liquidate_full_repay_health_factor_no_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    client.set_close_factor_bps(&admin, &10_000);
    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);

    let hf = client.get_health_factor(&borrower);
    assert_eq!(hf, views::HEALTH_FACTOR_NO_DEBT);
}

// ─────────────────────────────────────────────────────────────────────────────
// Post-liquidation: health improves or position is cleared
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_post_health_improves() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let hf_before = client.get_health_factor(&borrower);
    assert!(hf_before < HEALTH_FACTOR_SCALE);

    let liquidator = Address::generate(&env);
    // Partial repay: 3_000
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &3_000);

    let hf_after = client.get_health_factor(&borrower);
    // Position should be at least as healthy as before
    assert!(hf_after >= hf_before || hf_after == 0);
}

#[test]
fn test_liquidate_position_above_threshold_after_full_repay() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    client.set_close_factor_bps(&admin, &10_000);
    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);

    // After full repay, no debt => healthy
    let hf = client.get_health_factor(&borrower);
    assert!(hf >= HEALTH_FACTOR_SCALE || hf == views::HEALTH_FACTOR_NO_DEBT);
}

// ─────────────────────────────────────────────────────────────────────────────
// Incentive: collateral seized includes bonus
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_collateral_seized_includes_incentive() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let collateral_before = client.get_collateral_balance(&borrower);

    let liquidator = Address::generate(&env);
    let repay = 3_000_i128;
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &repay);

    let collateral_after = client.get_collateral_balance(&borrower);
    // Seized = repay * (10_000 + 1_000) / 10_000 = repay * 1.1
    let expected_seized = repay * 11_000 / 10_000; // 3_300
    assert_eq!(collateral_before - collateral_after, expected_seized);
}

// ─────────────────────────────────────────────────────────────────────────────
// Close-factor cap
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_clamped_by_close_factor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let liquidator = Address::generate(&env);
    // Try to repay MORE than close factor allows (50% of 10_000 = 5_000)
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &9_000);

    // Actual repaid is clamped to 5_000
    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 5_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// Collateral capped at available
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_collateral_capped_at_available() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    // Set 100% incentive so seized = 2x repay; with 15_000 collateral and
    // 5_000 repay, incentive would be 10_000.  Collateral should be capped.
    client.set_liquidation_incentive_bps(&admin, &10_000); // 100%
    client.set_close_factor_bps(&admin, &10_000);

    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);

    // Collateral must not go negative
    let remaining_col = client.get_collateral_balance(&borrower);
    assert!(remaining_col >= 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Global total debt decremented
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_reduces_total_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    // Capture global total debt before liquidation
    let total_debt_before = crate::borrow::get_total_debt(&env);

    client.set_close_factor_bps(&admin, &10_000);
    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);

    // Global total debt should be reduced by the principal repaid (5_000)
    let total_debt_after = crate::borrow::get_total_debt(&env);
    assert_eq!(total_debt_before - total_debt_after, 5_000);

    // Borrower's remaining principal should also be 5_000
    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 5_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// Accrued interest included in liquidatable total
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_interest_included_in_principal() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000);

    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &15_000);

    // Fast-forward 1 year so interest accrues
    env.ledger()
        .with_mut(|li| li.timestamp = 1_000 + 31_536_000);

    let total = client.get_debt_balance(&borrower);
    assert!(total > 10_000);

    // Max liquidatable is based on total (with interest)
    let max_liq = client.get_max_liquidatable_amount(&borrower);
    assert_eq!(max_liq, total / 2);

    client.set_close_factor_bps(&admin, &10_000);
    let liquidator = Address::generate(&env);
    // Repay the full debt (clamped to total via close factor 100%)
    client.liquidate(
        &liquidator,
        &borrower,
        &asset,
        &collateral_asset,
        &(total + 1),
    );

    let debt_after = client.get_user_debt(&borrower);
    assert_eq!(debt_after.borrowed_amount, 0);
    assert_eq!(debt_after.interest_accrued, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Sequential liquidations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_sequential_converges() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let liquidator = Address::generate(&env);

    // First call: repay 5_000 (50% close factor)
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);
    let debt1 = client.get_user_debt(&borrower);
    let remaining1 = debt1.borrowed_amount + debt1.interest_accrued;

    // Second call: repay up to remaining (still under water)
    if client.get_health_factor(&borrower) < HEALTH_FACTOR_SCALE {
        client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);
        let debt2 = client.get_user_debt(&borrower);
        let remaining2 = debt2.borrowed_amount + debt2.interest_accrued;
        assert!(remaining2 < remaining1, "debt must decrease on each call");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: zero amount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_zero_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &0);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: negative amount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_negative_amount_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &-1);
    assert_eq!(result, Err(Ok(BorrowError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: healthy position rejected
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_healthy_position_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);

    // Healthy: HF >= 1.0 with default 80% threshold and 200% collateral
    client.set_liquidation_threshold_bps(&admin, &8000);
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &20_000);

    let hf = client.get_health_factor(&borrower);
    assert!(hf >= HEALTH_FACTOR_SCALE);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &1_000);
    assert_eq!(result, Err(Ok(BorrowError::InsufficientCollateral)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: no oracle configured
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_no_oracle_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    // Do NOT set oracle
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &15_000);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);
    // Without oracle, HF can't be computed → treated as healthy
    assert_eq!(result, Err(Ok(BorrowError::InsufficientCollateral)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: liquidation paused
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_paused_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    client.set_pause(&admin, &PauseType::Liquidation, &true);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: emergency shutdown blocked
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_emergency_shutdown_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    // Trigger emergency shutdown
    client.emergency_shutdown(&admin);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);
    assert_eq!(result, Err(Ok(BorrowError::ProtocolPaused)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: asset mismatch — wrong debt asset
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_wrong_debt_asset_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let wrong_asset = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(
        &liquidator,
        &borrower,
        &wrong_asset,
        &collateral_asset,
        &5_000,
    );
    assert_eq!(result, Err(Ok(BorrowError::AssetNotSupported)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Error: asset mismatch — wrong collateral asset
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_wrong_collateral_asset_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    let wrong_collateral = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &wrong_collateral, &5_000);
    assert_eq!(result, Err(Ok(BorrowError::AssetNotSupported)));
}

// ─────────────────────────────────────────────────────────────────────────────
// Edge: borrower with no debt
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_no_debt_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);

    // No borrow — no debt at all
    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &5_000);
    // Asset mismatch because debt_position.asset defaults to borrower address
    assert!(result.is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// Edge: zero incentive — liquidator gets back exactly repay amount
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_zero_incentive_seizes_exact_repay() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    client.set_liquidation_incentive_bps(&admin, &0);

    let collateral_before = client.get_collateral_balance(&borrower);
    let repay = 3_000_i128;

    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &repay);

    let collateral_after = client.get_collateral_balance(&borrower);
    assert_eq!(collateral_before - collateral_after, repay);
}

// ─────────────────────────────────────────────────────────────────────────────
// Maximum incentive (100%) — seized = 2x repay, capped at available
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidate_max_incentive_100pct() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, asset, collateral_asset) = setup_liquidatable(&env);
    create_underwater_position(&client, &borrower, &asset, &collateral_asset);

    client.set_liquidation_incentive_bps(&admin, &10_000); // 100%

    let collateral_before = client.get_collateral_balance(&borrower);
    let repay = 3_000_i128;

    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &repay);

    let collateral_after = client.get_collateral_balance(&borrower);
    let seized = collateral_before - collateral_after;
    // Expected: 3_000 * 2 = 6_000, but cap at 15_000 available → 6_000
    let expected = repay * 2;
    assert_eq!(seized, expected.min(collateral_before));
}
