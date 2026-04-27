//! Comprehensive tests for view functions: collateral value, debt value, health factor, position summary.
//! Covers edge cases (zero collateral, zero debt, boundary health factor) and security (no state change, oracle usage).

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use views::{HEALTH_FACTOR_NO_DEBT, HEALTH_FACTOR_SCALE};

/// Mock oracle contract: returns fixed price (1.0 with 8 decimals) for any asset.
#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    /// Returns price with 8 decimals (100_000_000 = 1.0).
    pub fn price(_env: Env, _asset: Address) -> i128 {
        100_000_000
    }
}

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
    Address,
) {
    let (client, admin, user, asset, collateral_asset) = setup(env);
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    (client, admin, user, asset, collateral_asset, oracle_id)
}

// ─────────────────────────────────────────────────────────────────────────────
// get_collateral_balance
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_collateral_balance_zero_when_no_position() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_collateral_balance(&user), 0);
}

#[test]
fn test_get_collateral_balance_returns_amount_after_borrow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    assert_eq!(client.get_collateral_balance(&user), 20_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// get_debt_balance
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_debt_balance_zero_when_no_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(client.get_debt_balance(&user), 0);
}

#[test]
fn test_get_debt_balance_returns_principal_plus_interest() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    let (client, _admin, user, asset, collateral_asset) = setup(&env);
    client.borrow(&user, &asset, &100_000, &collateral_asset, &200_000);
    assert_eq!(client.get_debt_balance(&user), 100_000);
    env.ledger().with_mut(|li| li.timestamp = 1000 + 31_536_000);
    let debt_balance = client.get_debt_balance(&user);
    assert!(debt_balance > 100_000);
    assert!(debt_balance <= 105_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// get_collateral_value / get_debt_value (oracle)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_collateral_value_zero_when_oracle_not_set() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    assert_eq!(client.get_collateral_value(&user), 0);
}

#[test]
fn test_get_debt_value_zero_when_oracle_not_set() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    assert_eq!(client.get_debt_value(&user), 0);
}

#[test]
fn test_get_collateral_value_with_oracle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    // value = 20_000 * 100_000_000 / 100_000_000 = 20_000 (same unit as amount when price = 1)
    assert_eq!(client.get_collateral_value(&user), 20_000);
}

#[test]
fn test_get_debt_value_with_oracle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    assert_eq!(client.get_debt_value(&user), 10_000);
}

#[test]
fn test_get_collateral_value_zero_collateral() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset, _oracle) = setup_with_oracle(&env);
    assert_eq!(client.get_collateral_value(&user), 0);
}

#[test]
fn test_get_debt_value_zero_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset, _oracle) = setup_with_oracle(&env);
    assert_eq!(client.get_debt_value(&user), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// get_health_factor
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_health_factor_no_debt_returns_sentinel() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset, _oracle) = setup_with_oracle(&env);
    assert_eq!(client.get_health_factor(&user), HEALTH_FACTOR_NO_DEBT);
}

#[test]
fn test_get_health_factor_zero_when_oracle_not_set() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset) = setup(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    assert_eq!(client.get_health_factor(&user), 0);
}

#[test]
fn test_get_health_factor_healthy_above_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    // Collateral 20_000, debt 10_000. With price 1: cv=20_000, dv=10_000.
    // Default liq threshold 80%. Weighted = 20_000 * 0.8 = 16_000. HF = 16_000 * 10000 / 10_000 = 16000 (> 10000).
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    let hf = client.get_health_factor(&user);
    assert!(hf >= HEALTH_FACTOR_SCALE);
    assert_eq!(hf, 16_000);
}

#[test]
fn test_get_health_factor_liquidatable_below_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    // Liquidation threshold 40%. Collateral 30_000, debt 15_000 (meets 150% borrow rule).
    // Weighted = 30_000 * 0.4 = 12_000, HF = 12_000 * 10000 / 15_000 = 8000 < 10000.
    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&user, &asset, &15_000, &collateral_asset, &30_000);
    let hf = client.get_health_factor(&user);
    assert!(hf < HEALTH_FACTOR_SCALE);
    assert_eq!(hf, 8000);
}

#[test]
fn test_get_health_factor_boundary_at_one() {
    let env = Env::default();
    env.mock_all_auths();
    // At HF = 1.0: weighted_collateral = debt_value. Collateral 1500, debt 1000, lt 6667 -> weighted = 1000, HF = 10000.
    let (client, admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &6667);
    client.borrow(&user, &asset, &1000, &collateral_asset, &1500);
    assert_eq!(client.get_health_factor(&user), HEALTH_FACTOR_SCALE);
}

// ─────────────────────────────────────────────────────────────────────────────
// get_user_position
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_user_position_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset, _oracle) = setup_with_oracle(&env);
    let pos = client.get_user_position(&user);
    assert_eq!(pos.collateral_balance, 0);
    assert_eq!(pos.collateral_value, 0);
    assert_eq!(pos.debt_balance, 0);
    assert_eq!(pos.debt_value, 0);
    assert_eq!(pos.health_factor, HEALTH_FACTOR_NO_DEBT);
}

#[test]
fn test_get_user_position_matches_individual_getters() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    let pos = client.get_user_position(&user);
    assert_eq!(pos.collateral_balance, client.get_collateral_balance(&user));
    assert_eq!(pos.collateral_value, client.get_collateral_value(&user));
    assert_eq!(pos.debt_balance, client.get_debt_balance(&user));
    assert_eq!(pos.debt_value, client.get_debt_value(&user));
    assert_eq!(pos.health_factor, client.get_health_factor(&user));
}

// ─────────────────────────────────────────────────────────────────────────────
// Admin: set_oracle, set_liquidation_threshold_bps
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_set_oracle_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset) = setup(&env);
    let oracle_id = env.register(MockOracle, ());
    let result = client.try_set_oracle(&user, &oracle_id);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_set_liquidation_threshold_bps_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, _asset, _collateral_asset) = setup(&env);
    let result = client.try_set_liquidation_threshold_bps(&user, &8000);
    assert_eq!(result, Err(Ok(BorrowError::Unauthorized)));
}

#[test]
fn test_set_liquidation_threshold_bps_invalid() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _user, _asset, _collateral_asset) = setup(&env);
    assert_eq!(
        client.try_set_liquidation_threshold_bps(&admin, &0),
        Err(Ok(BorrowError::InvalidAmount))
    );
    assert_eq!(
        client.try_set_liquidation_threshold_bps(&admin, &10001),
        Err(Ok(BorrowError::InvalidAmount))
    );
}

#[test]
fn test_set_liquidation_threshold_bps_valid() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    client.set_liquidation_threshold_bps(&admin, &7500);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    let hf = client.get_health_factor(&user);
    // weighted = 20_000 * 0.75 = 15_000, HF = 15_000 * 10000 / 10_000 = 15000
    assert_eq!(hf, 15_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// Security: views are read-only (no state change)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_views_do_not_modify_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, user, asset, collateral_asset, _oracle) = setup_with_oracle(&env);
    client.borrow(&user, &asset, &10_000, &collateral_asset, &20_000);
    let debt_before = client.get_user_debt(&user);
    let _ = client.get_user_position(&user);
    let _ = client.get_health_factor(&user);
    let _ = client.get_collateral_value(&user);
    let _ = client.get_debt_value(&user);
    let debt_after = client.get_user_debt(&user);
    assert_eq!(debt_before.borrowed_amount, debt_after.borrowed_amount);
    assert_eq!(debt_before.interest_accrued, debt_after.interest_accrued);
}
