//! Tests for the liquidation protocol: close factor, liquidation incentive, and safety checks.

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, contract, contractimpl,
};
use crate::views::{HEALTH_FACTOR_SCALE};

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

fn setup_liquidate_test(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    
    let admin = Address::generate(env);
    let borrower = Address::generate(env);
    let liquidator = Address::generate(env);
    let asset = Address::generate(env);
    let collateral_asset = Address::generate(env);
    
    client.initialize(&admin, &1_000_000_000, &1000);
    
    let oracle_id = env.register(MockOracle, ());
    client.set_oracle(&admin, &oracle_id);
    
    (client, admin, borrower, liquidator, asset, collateral_asset, oracle_id)
}

#[test]
fn test_liquidation_success_with_close_factor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, liquidator, asset, collateral_asset, _) = setup_liquidate_test(&env);
    
    // 1. Setup a liquidatable position
    // Debt 10_000, Collateral 15_000. LT 60%
    // Weighted = 15_000 * 0.6 = 9_000. HF = 9000/10000 = 0.9 (< 1.0)
    // Initial borrow ratio 150% (15k/10k) is valid.
    client.set_liquidation_threshold_bps(&admin, &6000);
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &15_000);
    
    assert!(client.get_health_factor(&borrower) < HEALTH_FACTOR_SCALE);
    
    // 2. Liquidate
    // Default Close Factor 50% = 5_000 max repayable
    // Try to repay 10_000 -> Should only repay 5_000
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);
    
    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 5_000); // 10_000 - 5_000
    
    // 3. Verify incentive (10% default)
    // SeizedValue = 5_000 * 1.1 = 5_500
    // Price = 1.0 -> Seized Amount = 5_500
    let collateral = client.get_user_collateral(&borrower);
    assert_eq!(collateral.amount, 9_500); // 15_000 - 5_500
}

#[test]
fn test_liquidation_partial_repayment() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, liquidator, asset, collateral_asset, _) = setup_liquidate_test(&env);
    
    client.set_liquidation_threshold_bps(&admin, &6000);
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &15_000);
    
    // Repay only 2_000 (below close factor limit of 5_000)
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &2_000);
    
    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 8_000); // 10_000 - 2_000
    
    // Seized: 2_000 * 1.1 = 2_200
    let collateral = client.get_user_collateral(&borrower);
    assert_eq!(collateral.amount, 12_800); // 15_000 - 2_200
}

#[test]
fn test_liquidation_rejects_healthy_position() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, borrower, liquidator, asset, collateral_asset, _) = setup_liquidate_test(&env);
    
    // Debt 10_000, Collateral 20_000. LT 80% (default)
    // Weighted = 16_000. HF = 1.6
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &20_000);
    
    let result = client.try_liquidate(&liquidator, &borrower, &asset, &collateral_asset, &1_000);
    assert_eq!(result, Err(Ok(BorrowError::PositionNotLiquidatable)));
}

#[test]
fn test_admin_updates_liquidation_parameters() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, liquidator, asset, collateral_asset, _) = setup_liquidate_test(&env);
    
    // Set Close Factor to 100% and Incentive to 20%
    client.set_liquidation_close_factor(&admin, &10000);
    client.set_liquidation_incentive(&admin, &2000);
    
    client.set_liquidation_threshold_bps(&admin, &6000);
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &15_000);
    
    // Liquidate full 10_000
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);
    
    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 0);
    
    // Seized: 10_000 * 1.2 = 12_000
    let collateral = client.get_user_collateral(&borrower);
    assert_eq!(collateral.amount, 3_000); // 15_000 - 12_000
}

#[test]
fn test_liquidation_caps_at_available_collateral() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, borrower, liquidator, asset, collateral_asset, _) = setup_liquidate_test(&env);
    
    // Settings for full liquidation and high incentive (50%)
    client.set_liquidation_close_factor(&admin, &10000);
    client.set_liquidation_incentive(&admin, &5000); 
    
    // 150% min ratio -> borrow 10k needs 15k collateral.
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &15_000);
    
    // Now make it liquidatable by dropping threshold to e.g. 40%
    // Weighted = 15_000 * 0.4 = 6_000. HF = 0.6
    client.set_liquidation_threshold_bps(&admin, &4000);
    
    // Repay 10_000. Incentive 1.5 -> Should seize 15_000. 
    // This exactly drains the collateral.
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);
    
    let collateral = client.get_user_collateral(&borrower);
    assert_eq!(collateral.amount, 0); // Completely drained
    
    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 0); 
}

#[test]
fn test_liquidation_interest_accrual_integration() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1000);
    let (client, admin, borrower, liquidator, asset, collateral_asset, _) = setup_liquidate_test(&env);
    
    client.set_liquidation_threshold_bps(&admin, &6000);
    client.borrow(&borrower, &asset, &10_000, &collateral_asset, &15_000);
    
    // Fast forward 1 year (5% interest)
    env.ledger().with_mut(|li| li.timestamp = 1000 + 31_536_000);
    
    // Debt balance will be roughly 10_500
    let debt_bal = client.get_debt_balance(&borrower);
    assert!(debt_bal >= 10_500);
    
    // Liquidate based on accrued debt
    // Close factor 50% of 10,500 = 5,250
    client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &10_000);
    
    let remaining_debt = client.get_debt_balance(&borrower);
    assert_eq!(remaining_debt, 5_250);
}
