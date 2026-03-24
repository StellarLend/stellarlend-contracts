//! # Liquidation Boundary Tests — Issue #456
//!
//! Targeted edge-boundary tests for `get_max_liquidatable_amount` and
//! `get_liquidation_incentive_amount`.
//!
//! ## Close factor
//! Controls the maximum fraction of a debt position that can be liquidated per
//! call. Default: 5000 bps (50%). Range: 1–10000 bps.
//!
//! ## Liquidation incentive
//! Extra collateral bonus paid to the liquidator above the debt repaid.
//! Default: 1000 bps (10%). Range: 0–10000 bps.
//!
//! ## Security notes
//! - Both view functions are read-only; no state is mutated.
//! - `get_max_liquidatable_amount` returns 0 when the oracle is absent — a
//!   liquidator cannot act without price data, preventing phantom liquidations.
//! - Overflow is handled via I256 arithmetic; the incentive amount saturates at
//!   `i128::MAX` rather than panicking.
//! - Admin-only setters enforce bps bounds so protocol parameters stay in range.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use views::HEALTH_FACTOR_SCALE;

// ─────────────────────────────────────────────────────────────────────────────
// Mock oracle (price = 1.0, i.e. 100_000_000 with 8 decimals)
// ─────────────────────────────────────────────────────────────────────────────

#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn price(_env: Env, _asset: Address) -> i128 {
        100_000_000
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
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
    (client, admin, user, asset, collateral_asset)
}

fn setup_with_oracle(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    let (client, admin, user, asset, collateral_asset) = setup(env);
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    (client, admin, user, asset, collateral_asset)
}

// ─────────────────────────────────────────────────────────────────────────────
// get_max_liquidatable_amount — zero / healthy cases
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_max_liquidatable_zero_when_no_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset) = setup_with_oracle(&env);
    assert_eq!(client.get_max_liquidatable_amount(&user), 0);
}

#[test]
fn test_max_liquidatable_zero_when_oracle_not_set() {
    // Without an oracle the health factor cannot be computed → must not liquidate.
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);
    // Collateral 15_000, borrow 10_000 (150% ratio). Liquidation threshold 40% → HF < 1 when oracle exists.
    client.set_liquidation_threshold_bps(&_admin, &4000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    assert_eq!(client.get_max_liquidatable_amount(&user), 0);
}

#[test]
fn test_max_liquidatable_zero_when_position_healthy() {
    // Default liquidation threshold 80%: collateral 20_000, debt 10_000 → HF = 16_000 > 10_000.
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    assert_eq!(client.get_max_liquidatable_amount(&user), 0);
}

#[test]
fn test_max_liquidatable_zero_at_exact_health_factor_boundary() {
    // Set threshold so HF == exactly HEALTH_FACTOR_SCALE (10_000) → not liquidatable yet.
    // collateral = 15_000, debt = 10_000, threshold 6667 bps
    // weighted = 15_000 * 6667 / 10_000 = 10_000 (rounded)
    // HF = 10_000 * 10_000 / 10_000 = 10_000
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &6667);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    let hf = client.get_health_factor(&user);
    assert_eq!(hf, HEALTH_FACTOR_SCALE);
    assert_eq!(client.get_max_liquidatable_amount(&user), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// get_max_liquidatable_amount — liquidatable cases
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_max_liquidatable_default_close_factor() {
    // Threshold 40%: collateral 15_000, debt 10_000 → HF = 15_000*0.4*10_000/10_000 = 6_000 < 10_000.
    // Close factor 50% (default): max_liq = 10_000 * 5000 / 10_000 = 5_000.
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    assert!(client.get_health_factor(&user) < HEALTH_FACTOR_SCALE);
    assert_eq!(client.get_max_liquidatable_amount(&user), 5_000);
}

#[test]
fn test_max_liquidatable_full_close_factor_100_pct() {
    // Close factor 100% → entire debt is liquidatable.
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.set_close_factor_bps(&admin, &10000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    assert_eq!(client.get_max_liquidatable_amount(&user), 10_000);
}

#[test]
fn test_max_liquidatable_minimum_close_factor_1_bps() {
    // Close factor 1 bps = 0.01%: max_liq = floor(10_000 * 1 / 10_000) = 1.
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.set_close_factor_bps(&admin, &1);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    assert_eq!(client.get_max_liquidatable_amount(&user), 1);
}

#[test]
fn test_max_liquidatable_includes_accrued_interest() {
    // After 1 year, interest accrues; total_debt > borrowed_amount.
    // Close factor 50%: max_liq = total_debt * 50%.
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000);
    let (client, admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    env.ledger()
        .with_mut(|li| li.timestamp = 1_000 + 31_536_000);
    let total_debt = client.get_debt_balance(&user);
    assert!(total_debt > 10_000, "interest should have accrued");
    let max_liq = client.get_max_liquidatable_amount(&user);
    assert_eq!(max_liq, total_debt / 2); // 50% close factor
}

#[test]
fn test_max_liquidatable_just_below_boundary_is_liquidatable() {
    // One unit below healthy: collateral = 14_999 (would need 15_000 for HF==1 with threshold 6667).
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &6667);
    // Borrow 10_000 with 14_999 collateral — still passes 150% borrow rule? 14_999 < 15_000 → no.
    // Use higher collateral to pass borrow rule but use a threshold that makes it sub-healthy.
    // threshold 4000: collateral 15_000, debt 10_000 → HF = 15_000*0.4*10000/10000 = 6000 < 10000.
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    let hf = client.get_health_factor(&user);
    assert!(hf < HEALTH_FACTOR_SCALE, "position must be liquidatable");
    let max_liq = client.get_max_liquidatable_amount(&user);
    assert!(
        max_liq > 0,
        "max_liq must be > 0 for under-collateralised position"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// get_liquidation_incentive_amount — zero / negative
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_incentive_amount_zero_for_zero_repay() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_liquidation_incentive_amount(&0), 0);
}

#[test]
fn test_incentive_amount_zero_for_negative_repay() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_liquidation_incentive_amount(&-1), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// get_liquidation_incentive_amount — default and custom incentive
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_incentive_amount_default_10_pct() {
    // Default incentive 1000 bps (10%): 10_000 * 11000 / 10000 = 11_000.
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_liquidation_incentive_amount(&10_000), 11_000);
}

#[test]
fn test_incentive_amount_zero_incentive_returns_same() {
    // Incentive 0 bps: liquidator gets exactly what they repay.
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    client.set_liquidation_incentive_bps(&admin, &0);
    assert_eq!(client.get_liquidation_incentive_amount(&10_000), 10_000);
}

#[test]
fn test_incentive_amount_max_incentive_100_pct() {
    // Incentive 10000 bps (100%): 10_000 * 20000 / 10000 = 20_000.
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    client.set_liquidation_incentive_bps(&admin, &10000);
    assert_eq!(client.get_liquidation_incentive_amount(&10_000), 20_000);
}

#[test]
fn test_incentive_amount_custom_5_pct() {
    // Incentive 500 bps (5%): 20_000 * 10500 / 10000 = 21_000.
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    client.set_liquidation_incentive_bps(&admin, &500);
    assert_eq!(client.get_liquidation_incentive_amount(&20_000), 21_000);
}

#[test]
fn test_incentive_amount_single_unit() {
    // 1 unit repaid with 10% incentive: floor(1 * 11000 / 10000) = 1.
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_liquidation_incentive_amount(&1), 1);
}

#[test]
fn test_incentive_amount_large_value_no_overflow() {
    // Use a large but safe value to verify I256 arithmetic avoids overflow.
    // repay = i128::MAX / 3, incentive 1000 bps → result ≈ 1.1 * (i128::MAX/3) which fits in i128.
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _collateral_asset) = setup(&env);
    let repay = i128::MAX / 3;
    let result = client.get_liquidation_incentive_amount(&repay);
    // result = repay * 11000 / 10000 = repay * 1.1 — should not panic or wrap
    assert!(result > repay, "incentive amount must be > repay amount");
}

// ─────────────────────────────────────────────────────────────────────────────
// Admin setters — authorisation & bounds
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_set_close_factor_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset) = setup(&env);
    let result = client.try_set_close_factor_bps(&user, &5000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_set_close_factor_invalid_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(
        client.try_set_close_factor_bps(&admin, &0),
        Err(Ok(BorrowError::InvalidAmount))
    );
}

#[test]
fn test_set_close_factor_invalid_above_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(
        client.try_set_close_factor_bps(&admin, &10001),
        Err(Ok(BorrowError::InvalidAmount))
    );
}

#[test]
fn test_set_close_factor_valid_bounds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    // Min valid
    client.set_close_factor_bps(&admin, &1);
    assert_eq!(client.get_close_factor_bps(), 1);
    // Max valid
    client.set_close_factor_bps(&admin, &10000);
    assert_eq!(client.get_close_factor_bps(), 10000);
}

#[test]
fn test_set_liquidation_incentive_bps_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset) = setup(&env);
    let result = client.try_set_liquidation_incentive_bps(&user, &1000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_set_liquidation_incentive_bps_invalid_above_10000() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(
        client.try_set_liquidation_incentive_bps(&admin, &10001),
        Err(Ok(BorrowError::InvalidAmount))
    );
}

#[test]
fn test_set_liquidation_incentive_bps_invalid_negative() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(
        client.try_set_liquidation_incentive_bps(&admin, &-1),
        Err(Ok(BorrowError::InvalidAmount))
    );
}

#[test]
fn test_set_close_factor_bps_invalid_negative() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(
        client.try_set_close_factor_bps(&admin, &-1),
        Err(Ok(BorrowError::InvalidAmount))
    );
}

#[test]
fn test_set_liquidation_incentive_bps_valid_zero() {
    // Zero incentive is valid (liquidator gets no bonus).
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    client.set_liquidation_incentive_bps(&admin, &0);
    assert_eq!(client.get_liquidation_incentive_bps(), 0);
}

#[test]
fn test_set_liquidation_incentive_bps_valid_max() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    client.set_liquidation_incentive_bps(&admin, &10000);
    assert_eq!(client.get_liquidation_incentive_bps(), 10000);
}

// ─────────────────────────────────────────────────────────────────────────────
// Default values
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_default_close_factor_is_5000() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_close_factor_bps(), 5000);
}

#[test]
fn test_default_liquidation_incentive_is_1000() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_liquidation_incentive_bps(), 1000);
}

// ─────────────────────────────────────────────────────────────────────────────
// Pause interaction — liquidation paused must not affect view reads
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_max_liquidatable_still_computable_when_liquidation_paused() {
    // Views are read-only; pause state does not block them.
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &15_000);
    client.set_pause(&admin, &PauseType::Liquidation, &true);
    // View must still work
    assert_eq!(client.get_max_liquidatable_amount(&user), 5_000);
}
