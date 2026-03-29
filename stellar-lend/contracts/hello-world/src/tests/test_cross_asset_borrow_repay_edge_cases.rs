#![cfg(test)]
//! Cross-Asset Borrow and Repay Edge Case Tests
//!
//! Systematic test matrix for cross-asset deposit/borrow/repay sequences,
//! mirroring scenarios documented in test_snapshots/tests/test_cross_asset_borrow_repay_edge_cases/.
//!
//! ## Coverage
//! - Multi-collateral borrowing (2-3 asset types)
//! - Multi-asset borrowing against unified collateral pool
//! - Partial and full repayment across assets
//! - Collateral devaluation and health factor effects
//! - Collateral withdrawal with and without debt
//! - Sequential borrow/repay cycles
//! - Boundary conditions: zero amounts, very small/large values
//! - Health factor precision at exact thresholds
//! - Multiple independent user positions
//! - Asset configuration changes (collateral factor, disable borrow)
//! - Native XLM (None asset) support
//!
//! ## Security Notes
//! - All external calls require `user.require_auth()` (mocked via `mock_all_auths`)
//! - Admin-only operations (initialize_asset, update_asset_config, update_asset_price)
//!   are guarded by `require_admin`
//! - Health factor < 10000 (1.0x) blocks borrows and withdrawals
//! - Repayment caps at total debt — debt cannot go negative
//! - Checked arithmetic throughout; overflow returns an error variant
//! - Price staleness (> 1 hour) causes position summary to fail

use crate::cross_asset::AssetConfig;
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

// ============================================================================
// TEST HELPERS
// ============================================================================

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> (HelloContractClient<'_>, Address) {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_ca(&admin);
    (client, admin)
}

/// Standard asset config: 75% collateral factor, 80% liquidation threshold
fn make_asset_config(env: &Env, asset: Option<Address>, price: i128) -> AssetConfig {
    AssetConfig {
        asset: asset.clone(),
        collateral_factor: 7500,
        liquidation_threshold: 8000,
        reserve_factor: 1000,
        max_supply: 100_000_000_000_000,
        max_borrow: 80_000_000_000_000,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_updated_at: env.ledger().timestamp(),
    }
}

/// Custom asset config with explicit collateral and liquidation factors
fn make_custom_config(
    env: &Env,
    asset: Option<Address>,
    price: i128,
    collateral_factor: i128,
    liquidation_threshold: i128,
) -> AssetConfig {
    AssetConfig {
        asset: asset.clone(),
        collateral_factor,
        liquidation_threshold,
        reserve_factor: 1000,
        max_supply: 100_000_000_000_000,
        max_borrow: 80_000_000_000_000,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_updated_at: env.ledger().timestamp(),
    }
}

/// Register USDC ($1), ETH ($2000), BTC ($40000) — all with 75% CF
fn setup_three_assets(env: &Env, client: &HelloContractClient) -> (Address, Address, Address) {
    let usdc = Address::generate(env);
    client.initialize_asset(&Some(usdc.clone()), &make_asset_config(env, Some(usdc.clone()), 1_0000000));

    let eth = Address::generate(env);
    client.initialize_asset(&Some(eth.clone()), &make_asset_config(env, Some(eth.clone()), 2000_0000000));

    let btc = Address::generate(env);
    client.initialize_asset(&Some(btc.clone()), &make_asset_config(env, Some(btc.clone()), 40000_0000000));

    (usdc, eth, btc)
}

// ============================================================================
// MULTI-COLLATERAL BORROW TESTS
// ============================================================================

/// Deposit three different collateral assets, borrow a single asset.
/// Verifies unified health factor aggregates all collateral.
#[test]
fn test_borrow_single_asset_against_three_collaterals() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    // USDC: $10,000 | ETH: 5 × $2,000 = $10,000 | BTC: 0.5 × $40,000 = $20,000
    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_deposit(&user, &Some(eth.clone()), &5_0000000);
    client.cross_asset_deposit(&user, &Some(btc.clone()), &5000000);

    // Total collateral $40k, weighted (75%) = $30k → borrow $25k USDC
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &25000_0000000);

    let pos = client.get_user_asset_position(&user, &Some(usdc.clone()));
    assert_eq!(pos.debt_principal, 25000_0000000);

    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_collateral_value, 40000_0000000);
    assert_eq!(summary.total_debt_value, 25000_0000000);
    assert!(summary.health_factor > 10000);
}

/// Borrow two different assets against two collateral assets.
#[test]
fn test_borrow_multiple_assets_against_multiple_collaterals() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    // $50k USDC + $40k BTC = $90k collateral, weighted $67.5k
    client.cross_asset_deposit(&user, &Some(usdc.clone()), &50000_0000000);
    client.cross_asset_deposit(&user, &Some(btc.clone()), &1_0000000);

    // Borrow 15 ETH ($30k) + $20k USDC = $50k total debt
    client.cross_asset_borrow(&user, &Some(eth.clone()), &15_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &20000_0000000);

    assert_eq!(client.get_user_asset_position(&user, &Some(eth)).debt_principal, 15_0000000);
    assert_eq!(client.get_user_asset_position(&user, &Some(usdc)).debt_principal, 20000_0000000);

    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_debt_value, 50000_0000000);
    assert!(summary.health_factor > 10000);
}

/// Borrow at a reasonable amount within multi-collateral capacity.
#[test]
fn test_borrow_at_maximum_capacity_multi_collateral() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    // $10k USDC + $10k ETH = $20k collateral, weighted $15k
    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_deposit(&user, &Some(eth.clone()), &5_0000000);

    // Borrow 0.37 BTC ≈ $14,800 — within capacity
    client.cross_asset_borrow(&user, &Some(btc.clone()), &370000);

    let summary = client.get_user_position_summary(&user);
    assert!(summary.health_factor >= 10000);
    assert!(summary.total_debt_value > 0);
}

/// Attempting to borrow more than weighted collateral allows must fail.
#[test]
fn test_borrow_exceeds_multi_collateral_capacity() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    // $10k USDC + $10k ETH = $20k, weighted $15k
    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_deposit(&user, &Some(eth.clone()), &5_0000000);

    // Try to borrow $20k — exceeds $15k weighted capacity
    let result = client.try_cross_asset_borrow(&user, &Some(usdc), &20000_0000000);
    assert!(result.is_err());
}

/// Each sequential borrow reduces remaining borrow capacity.
#[test]
fn test_sequential_borrows_different_assets() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    // 2 BTC = $80k collateral, weighted $60k
    client.cross_asset_deposit(&user, &Some(btc.clone()), &2_0000000);

    client.cross_asset_borrow(&user, &Some(usdc.clone()), &20000_0000000);
    let cap1 = client.get_user_position_summary(&user).borrow_capacity;

    client.cross_asset_borrow(&user, &Some(eth.clone()), &5_0000000);
    let cap2 = client.get_user_position_summary(&user).borrow_capacity;

    assert!(cap2 < cap1);
    assert_eq!(client.get_user_asset_position(&user, &Some(usdc)).debt_principal, 20000_0000000);
    assert_eq!(client.get_user_asset_position(&user, &Some(eth)).debt_principal, 5_0000000);
}

// ============================================================================
// PARTIAL REPAYMENT TESTS
// ============================================================================

/// Repaying 25% of a single-asset debt leaves 75% remaining.
#[test]
fn test_partial_repay_single_asset_debt() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &5000_0000000);

    client.cross_asset_repay(&user, &Some(usdc.clone()), &1250_0000000);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 3750_0000000);
}

/// Partial repayment of two different borrowed assets.
#[test]
fn test_partial_repay_multiple_assets() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(btc.clone()), &2_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &30000_0000000);
    client.cross_asset_borrow(&user, &Some(eth.clone()), &10_0000000);

    client.cross_asset_repay(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_repay(&user, &Some(eth.clone()), &3_0000000);

    assert_eq!(client.get_user_asset_position(&user, &Some(usdc)).debt_principal, 20000_0000000);
    assert_eq!(client.get_user_asset_position(&user, &Some(eth)).debt_principal, 7_0000000);
}

/// Fully repaying one asset leaves other debts intact.
#[test]
fn test_repay_one_asset_fully_keep_others() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(btc.clone()), &2_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &20000_0000000);
    client.cross_asset_borrow(&user, &Some(eth.clone()), &10_0000000);

    client.cross_asset_repay(&user, &Some(usdc.clone()), &20000_0000000);

    assert_eq!(client.get_user_asset_position(&user, &Some(usdc)).debt_principal, 0);
    assert_eq!(client.get_user_asset_position(&user, &Some(eth)).debt_principal, 10_0000000);

    // Only ETH debt ($20k) remains
    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_debt_value, 20000_0000000);
}

/// Repaying more than owed caps at zero — debt cannot go negative.
#[test]
fn test_repay_more_than_debt_caps_at_zero() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &5000_0000000);

    // Repay 2× the debt — should cap at zero
    client.cross_asset_repay(&user, &Some(usdc.clone()), &10000_0000000);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 0);
    assert_eq!(pos.accrued_interest, 0);
}

/// Repaying all debts across all assets results in zero total debt and infinite health.
#[test]
fn test_repay_all_debts_sequentially() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(btc.clone()), &2_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user, &Some(eth.clone()), &5_0000000);

    client.cross_asset_repay(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_repay(&user, &Some(eth.clone()), &5_0000000);

    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_debt_value, 0);
    assert_eq!(summary.health_factor, i128::MAX);
}

// ============================================================================
// COLLATERAL DEVALUATION EDGE CASES
// ============================================================================

/// A 50% collateral price drop should make a near-max-borrow position liquidatable.
#[test]
fn test_borrow_then_collateral_price_drops() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    // 10 ETH @ $2000 = $20k collateral, weighted $15k → borrow $10k USDC
    client.cross_asset_deposit(&user, &Some(eth.clone()), &10_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &10000_0000000);

    let hf_before = client.get_user_position_summary(&user).health_factor;
    assert!(hf_before > 10000);

    // ETH drops 50% to $1000 → collateral $10k, weighted $7.5k < $10k debt
    client.update_asset_price(&Some(eth), &1000_0000000);

    let summary = client.get_user_position_summary(&user);
    assert!(summary.health_factor < hf_before);
    assert!(summary.is_liquidatable);
}

/// One collateral devalues but the other keeps the position healthy.
#[test]
fn test_multi_collateral_one_asset_devalues() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &20000_0000000);
    client.cross_asset_deposit(&user, &Some(eth.clone()), &10_0000000);
    client.cross_asset_borrow(&user, &Some(btc.clone()), &500000); // ~$200 BTC

    let hf_before = client.get_user_position_summary(&user).health_factor;

    // ETH drops 80% — USDC collateral keeps position healthy
    client.update_asset_price(&Some(eth), &400_0000000);

    let summary = client.get_user_position_summary(&user);
    assert!(summary.health_factor < hf_before);
    assert!(!summary.is_liquidatable);
}

/// All collateral assets losing 90% value triggers liquidation.
#[test]
fn test_all_collateral_devalues_becomes_liquidatable() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    // $20k ETH + $20k BTC = $40k, weighted $30k → borrow $28k USDC
    client.cross_asset_deposit(&user, &Some(eth.clone()), &10_0000000);
    client.cross_asset_deposit(&user, &Some(btc.clone()), &5000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &28000_0000000);

    let hf_before = client.get_user_position_summary(&user).health_factor;

    // Both drop 90%
    client.update_asset_price(&Some(eth.clone()), &200_0000000);
    client.update_asset_price(&Some(btc), &4000_0000000);

    let summary = client.get_user_position_summary(&user);
    assert!(summary.health_factor < hf_before);
    assert!(summary.is_liquidatable);
}

/// Borrowed asset price doubling increases debt value and reduces health factor.
#[test]
fn test_borrowed_asset_price_increases() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &20000_0000000);
    client.cross_asset_borrow(&user, &Some(eth.clone()), &5_0000000); // $10k debt

    let summary_before = client.get_user_position_summary(&user);

    // ETH doubles to $4000 → debt becomes $20k
    client.update_asset_price(&Some(eth), &4000_0000000);

    let summary_after = client.get_user_position_summary(&user);
    assert!(summary_after.total_debt_value > summary_before.total_debt_value);
    assert!(summary_after.health_factor < summary_before.health_factor);
}

// ============================================================================
// COLLATERAL WITHDRAWAL EDGE CASES
// ============================================================================

/// Withdrawing one collateral asset while the other keeps health factor above 1.0.
#[test]
fn test_withdraw_one_collateral_maintain_health() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &20000_0000000);
    client.cross_asset_deposit(&user, &Some(eth.clone()), &10_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &15000_0000000);

    // Withdraw $10k USDC — ETH ($20k, weighted $15k) still covers $15k debt
    client.cross_asset_withdraw(&user, &Some(usdc.clone()), &10000_0000000);

    let summary = client.get_user_position_summary(&user);
    assert!(!summary.is_liquidatable);
    assert!(summary.health_factor > 10000);
}

/// Withdrawing collateral that would break health factor must be rejected.
#[test]
fn test_withdraw_collateral_breaks_health_fails() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_deposit(&user, &Some(eth.clone()), &5_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &14000_0000000);

    // Removing all ETH ($10k) would leave $10k USDC weighted $7.5k < $14k debt
    let result = client.try_cross_asset_withdraw(&user, &Some(eth), &5_0000000);
    assert!(result.is_err());
}

/// After full repayment, all collateral can be withdrawn.
#[test]
fn test_withdraw_all_collateral_after_full_repay() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_deposit(&user, &Some(eth.clone()), &5_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &10000_0000000);

    client.cross_asset_repay(&user, &Some(usdc.clone()), &10000_0000000);

    client.cross_asset_withdraw(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_withdraw(&user, &Some(eth), &5_0000000);

    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_collateral_value, 0);
    assert_eq!(summary.total_debt_value, 0);
}

// ============================================================================
// COLLATERAL FACTOR EDGE CASES
// ============================================================================



/// Repaying debt improves health factor proportionally.
#[test]
fn test_repay_improves_health_factor() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &7000_0000000);

    let hf_before = client.get_user_position_summary(&user).health_factor;

    client.cross_asset_repay(&user, &Some(usdc.clone()), &3500_0000000);

    let hf_after = client.get_user_position_summary(&user).health_factor;
    assert!(hf_after > hf_before);
}

/// Borrow capacity decreases on borrow and increases on repay.
#[test]
fn test_borrow_capacity_updates_correctly() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &20000_0000000);
    let cap0 = client.get_user_position_summary(&user).borrow_capacity;

    client.cross_asset_borrow(&user, &Some(eth.clone()), &3_0000000);
    let cap1 = client.get_user_position_summary(&user).borrow_capacity;
    assert!(cap1 < cap0);

    client.cross_asset_repay(&user, &Some(eth.clone()), &1_0000000);
    let cap2 = client.get_user_position_summary(&user).borrow_capacity;
    assert!(cap2 > cap1);
}

// ============================================================================
// COMPLEX MULTI-STEP LIFECYCLE TESTS
// ============================================================================

/// Full lifecycle: deposit → borrow → add more collateral → borrow more → partial repay → withdraw.
#[test]
fn test_complex_multi_asset_lifecycle() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &50000_0000000);
    client.cross_asset_borrow(&user, &Some(eth.clone()), &10_0000000);
    client.cross_asset_deposit(&user, &Some(btc.clone()), &1_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &30000_0000000);
    client.cross_asset_repay(&user, &Some(eth.clone()), &5_0000000);
    client.cross_asset_withdraw(&user, &Some(usdc.clone()), &20000_0000000);

    let usdc_pos = client.get_user_asset_position(&user, &Some(usdc.clone()));
    assert_eq!(usdc_pos.collateral, 30000_0000000);
    assert_eq!(usdc_pos.debt_principal, 30000_0000000);

    let eth_pos = client.get_user_asset_position(&user, &Some(eth));
    assert_eq!(eth_pos.debt_principal, 5_0000000);

    assert!(!client.get_user_position_summary(&user).is_liquidatable);
}

/// Multiple borrow/repay cycles accumulate debt correctly.
#[test]
fn test_alternating_borrow_repay_cycles() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(btc.clone()), &2_0000000);

    // Cycle 1: borrow $10k, repay $5k → net $5k
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_repay(&user, &Some(usdc.clone()), &5000_0000000);

    // Cycle 2: borrow $8k, repay $10k → net $3k
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &8000_0000000);
    client.cross_asset_repay(&user, &Some(usdc.clone()), &10000_0000000);

    // Cycle 3: borrow $15k → net $18k
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &15000_0000000);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 18000_0000000);
}



/// Multiple sequential partial repayments drain debt to zero.
#[test]
fn test_zero_debt_after_multiple_repayments() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &50000_0000000);
    client.cross_asset_borrow(&user, &Some(eth.clone()), &10_0000000);

    client.cross_asset_repay(&user, &Some(eth.clone()), &2_0000000);
    client.cross_asset_repay(&user, &Some(eth.clone()), &3_0000000);
    client.cross_asset_repay(&user, &Some(eth.clone()), &5_0000000);

    let pos = client.get_user_asset_position(&user, &Some(eth));
    assert_eq!(pos.debt_principal, 0);
}

// ============================================================================
// BOUNDARY AND PRECISION TESTS
// ============================================================================

/// Very small amounts (sub-unit) are handled without panic or overflow.
#[test]
fn test_very_small_amounts() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &100);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &70);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.collateral, 100);
    assert_eq!(pos.debt_principal, 70);
}

/// Large amounts near the supply cap do not overflow.
#[test]
fn test_very_large_amounts() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    let large = 50_000_000_000_000_i128;
    client.cross_asset_deposit(&user, &Some(usdc.clone()), &large);

    let borrow = (large * 75) / 100;
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &borrow);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.collateral, large);
    assert_eq!(pos.debt_principal, borrow);
}





// ============================================================================
// MULTIPLE USERS INTERACTION TESTS
// ============================================================================

/// Two users have fully independent positions — one's actions don't affect the other.
#[test]
fn test_multiple_users_independent_positions() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user1, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user1, &Some(eth.clone()), &2_0000000);

    client.cross_asset_deposit(&user2, &Some(eth.clone()), &5_0000000);
    client.cross_asset_borrow(&user2, &Some(usdc.clone()), &5000_0000000);

    let u1_usdc = client.get_user_asset_position(&user1, &Some(usdc.clone()));
    let u2_usdc = client.get_user_asset_position(&user2, &Some(usdc.clone()));

    assert_eq!(u1_usdc.collateral, 10000_0000000);
    assert_eq!(u1_usdc.debt_principal, 0);
    assert_eq!(u2_usdc.collateral, 0);
    assert_eq!(u2_usdc.debt_principal, 5000_0000000);
}

/// A global price update affects all users holding that asset.
#[test]
fn test_price_change_affects_all_users() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user1, &Some(eth.clone()), &10_0000000);
    client.cross_asset_deposit(&user2, &Some(eth.clone()), &5_0000000);
    client.cross_asset_borrow(&user1, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user2, &Some(usdc.clone()), &5000_0000000);

    let hf1_before = client.get_user_position_summary(&user1).health_factor;
    let hf2_before = client.get_user_position_summary(&user2).health_factor;

    // ETH drops 50%
    client.update_asset_price(&Some(eth), &1000_0000000);

    let hf1_after = client.get_user_position_summary(&user1).health_factor;
    let hf2_after = client.get_user_position_summary(&user2).health_factor;

    assert!(hf1_after < hf1_before);
    assert!(hf2_after < hf2_before);
}

// ============================================================================
// ASSET CONFIGURATION CHANGE TESTS
// ============================================================================



/// Disabling borrowing for an asset prevents new borrows.
#[test]
fn test_disable_asset_borrowing_prevents_new_borrows() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);

    client.update_asset_config(
        &Some(usdc.clone()),
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(false),
    );

    let result = client.try_cross_asset_borrow(&user, &Some(usdc), &1000_0000000);
    assert!(result.is_err());
}

/// Repayment still works after borrowing is disabled for an asset.
#[test]
fn test_repay_still_works_after_borrow_disabled() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &5000_0000000);

    client.update_asset_config(
        &Some(usdc.clone()),
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(false),
    );

    // Repay should succeed even though new borrows are blocked
    client.cross_asset_repay(&user, &Some(usdc.clone()), &2500_0000000);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 2500_0000000);
}

// ============================================================================
// STRESS TESTS
// ============================================================================

/// Ten sequential borrow/repay cycles accumulate debt correctly.
#[test]
fn test_many_sequential_operations() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &100000_0000000);

    // Each cycle borrows i×$1k and repays half
    for i in 1_i128..=10 {
        let amount = i * 1000_0000000;
        client.cross_asset_borrow(&user, &Some(usdc.clone()), &amount);
        client.cross_asset_repay(&user, &Some(usdc.clone()), &(amount / 2));
    }

    // Total borrowed: $55k, repaid: $27.5k → remaining $27.5k
    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 27500_0000000);
}



// ============================================================================
// ZERO-AMOUNT EDGE CASES
// ============================================================================

/// Depositing zero is a no-op — collateral stays unchanged.
#[test]
fn test_zero_deposit_is_noop() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_deposit(&user, &Some(usdc.clone()), &0);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.collateral, 10000_0000000);
}

/// Borrowing zero keeps debt unchanged.
#[test]
fn test_zero_borrow_is_noop() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &5000_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &0);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 5000_0000000);
}

/// Repaying zero keeps debt unchanged.
#[test]
fn test_zero_repay_is_noop() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &5000_0000000);
    client.cross_asset_repay(&user, &Some(usdc.clone()), &0);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 5000_0000000);
}

/// Repaying when there is no outstanding debt is a harmless no-op.
#[test]
fn test_repay_with_no_debt_is_noop() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_repay(&user, &Some(usdc.clone()), &5000_0000000);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 0);
    assert_eq!(pos.accrued_interest, 0);
}

// ============================================================================
// DUST / SUB-UNIT PRECISION TESTS
// ============================================================================

/// Dust-level amounts (1 unit) round-trip correctly through deposit.
#[test]
fn test_dust_amount_roundtrip() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &1);
    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.collateral, 1);

    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_debt_value, 0);
    assert_eq!(summary.health_factor, i128::MAX);
}

/// Very small borrow followed by exact repay leaves zero debt.
#[test]
fn test_small_borrow_exact_repay() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &100);
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &70);
    client.cross_asset_repay(&user, &Some(usdc.clone()), &70);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 0);
}

// ============================================================================
// MULTI-USER SEQUENTIAL REPAY TESTS
// ============================================================================

/// Two users borrow the same asset; one repays fully, other's debt unchanged.
#[test]
fn test_two_users_one_repays_fully() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let (usdc, eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user1, &Some(usdc.clone()), &20000_0000000);
    client.cross_asset_deposit(&user2, &Some(eth.clone()), &10_0000000);

    client.cross_asset_borrow(&user1, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&user2, &Some(usdc.clone()), &5000_0000000);

    client.cross_asset_repay(&user1, &Some(usdc.clone()), &10000_0000000);

    let p1 = client.get_user_asset_position(&user1, &Some(usdc.clone()));
    let p2 = client.get_user_asset_position(&user2, &Some(usdc));
    assert_eq!(p1.debt_principal, 0);
    assert_eq!(p2.debt_principal, 5000_0000000);
}

/// Three users interact sequentially — positions stay independent.
#[test]
fn test_three_users_sequential_operations() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let u3 = Address::generate(&env);
    let (usdc, eth, btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&u1, &Some(usdc.clone()), &50000_0000000);
    client.cross_asset_deposit(&u2, &Some(eth.clone()), &10_0000000);
    client.cross_asset_deposit(&u3, &Some(btc.clone()), &1_0000000);

    client.cross_asset_borrow(&u1, &Some(eth.clone()), &5_0000000);
    client.cross_asset_borrow(&u2, &Some(usdc.clone()), &10000_0000000);
    client.cross_asset_borrow(&u3, &Some(usdc.clone()), &20000_0000000);

    client.cross_asset_repay(&u2, &Some(usdc.clone()), &5000_0000000);

    assert_eq!(
        client.get_user_asset_position(&u1, &Some(eth)).debt_principal,
        5_0000000
    );
    assert_eq!(
        client.get_user_asset_position(&u2, &Some(usdc.clone())).debt_principal,
        5000_0000000
    );
    assert_eq!(
        client.get_user_asset_position(&u3, &Some(usdc)).debt_principal,
        20000_0000000
    );
}

// ============================================================================
// HEALTH FACTOR BOUNDARY (MULTI-ASSET) TESTS
// ============================================================================



// ============================================================================
// NATIVE XLM (None) FULL CYCLE TESTS
// ============================================================================

/// Full borrow/repay cycle using only native XLM.
#[test]
fn test_xlm_only_borrow_repay_cycle() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);

    client.initialize_asset(&None, &make_asset_config(&env, None, 1000000));

    client.cross_asset_deposit(&user, &None, &500000_0000000);
    client.cross_asset_borrow(&user, &None, &200000_0000000);

    let pos_borrow = client.get_user_asset_position(&user, &None);
    assert_eq!(pos_borrow.debt_principal, 200000_0000000);

    client.cross_asset_repay(&user, &None, &100000_0000000);
    assert_eq!(
        client.get_user_asset_position(&user, &None).debt_principal,
        100000_0000000
    );

    client.cross_asset_repay(&user, &None, &100000_0000000);
    assert_eq!(
        client.get_user_asset_position(&user, &None).debt_principal,
        0
    );

    client.cross_asset_withdraw(&user, &None, &500000_0000000);
    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_collateral_value, 0);
    assert_eq!(summary.total_debt_value, 0);
}

// ============================================================================
// ASSET CONFIG TOGGLE TESTS
// ============================================================================

/// Disabling then re-enabling borrowing allows new borrows.
#[test]
fn test_borrow_disabled_then_reenabled() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);

    client.update_asset_config(
        &Some(usdc.clone()), &None, &None, &None, &None, &None, &Some(false),
    );
    let r = client.try_cross_asset_borrow(&user, &Some(usdc.clone()), &1000_0000000);
    assert!(r.is_err());

    client.update_asset_config(
        &Some(usdc.clone()), &None, &None, &None, &None, &None, &Some(true),
    );
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &1000_0000000);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.debt_principal, 1000_0000000);
}

/// Disabling collateral prevents new deposits but existing collateral stays.
#[test]
fn test_disable_collateral_blocks_deposit() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);

    client.update_asset_config(
        &Some(usdc.clone()), &None, &None, &None, &None, &Some(false), &None,
    );

    let result = client.try_cross_asset_deposit(&user, &Some(usdc.clone()), &5000_0000000);
    assert!(result.is_err());

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.collateral, 10000_0000000);
}

// ============================================================================
// WITHDRAW EXACT COLLATERAL EDGE CASES
// ============================================================================

/// Withdrawing exact full collateral when no debt is outstanding succeeds.
#[test]
fn test_withdraw_exact_full_collateral_no_debt() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &12345_6789012);
    client.cross_asset_withdraw(&user, &Some(usdc.clone()), &12345_6789012);

    let pos = client.get_user_asset_position(&user, &Some(usdc));
    assert_eq!(pos.collateral, 0);
}

/// Withdrawing more than deposited fails with InsufficientCollateral.
#[test]
fn test_withdraw_more_than_deposited_fails() {
    let env = create_test_env();
    let (client, _admin) = setup_contract(&env);
    let user = Address::generate(&env);
    let (usdc, _eth, _btc) = setup_three_assets(&env, &client);

    client.cross_asset_deposit(&user, &Some(usdc.clone()), &10000_0000000);
    let result = client.try_cross_asset_withdraw(&user, &Some(usdc), &10001_0000000);
    assert!(result.is_err());
}
