//! Hardened Liquidation Logic Tests
//!
//! This module contains comprehensive tests for the hardened liquidation math.
//! It specifically validates:
//! - Accurate cross-asset conversion using oracle prices
//! - Multi-step liquidation math with I256 precision
//! - Proper incentive calculation and seizure capping
//! - Strict close factor enforcement
//! - State consistency after liquidations

#![cfg(test)]

use crate::deposit::{DepositDataKey, Position};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _},
    Address, Env, Symbol,
};

/// Creates a test environment with all auths mocked
fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    // Set a non-zero timestamp for interest accrual starts
    env.ledger().with_mut(|li| li.timestamp = 1000);
    env
}

/// Sets up admin and initializes the contract
fn setup_contract(env: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, admin, client)
}

#[test]
fn test_liquidate_hardened_math_cross_asset() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract(&env);

    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // Setup prices:
    // DebtAsset = $10.00 (1000000000 with 8 decimals)
    // CollateralAsset = $2.00 (200000000 with 8 decimals)
    // Ratio: 5 Collateral base units per 1 Debt base unit
    client.update_price_feed(&admin, &debt_asset, &1_000_000_000, &8, &admin);
    client.update_price_feed(&admin, &collateral_asset, &200_000_000, &8, &admin);

    // Setup risk params:
    // CloseFactor = 50% (5000 bps)
    // Liquidation Incentives = 10% (1000 bps)
    client.set_risk_params(&admin, &Some(5000), &None, &None, &Some(1000));

    // Create an undercollateralized position manually in storage to bypass deposit checks
    // Debt: 100 base units ($10 * 100 = $1000)
    // Collateral: 450 base units ($2 * 450 = $900)
    // Current Ratio: 90% (below the typical 105% liquidatable threshold)
    env.as_contract(&contract_id, || {
        let pos_key = DepositDataKey::Position(borrower.clone());
        env.storage().persistent().set(&pos_key, &Position {
            collateral: 450,
            debt: 100,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        });
        let col_key = DepositDataKey::CollateralBalance(borrower.clone());
        env.storage().persistent().set(&col_key, &450i128);
    });

    // ACTION: Liquidate 40 units of debt (well within the 50% close factor)
    // EXPECTATION:
    // 1. Value liquidated: 40 units * $10 = $400
    // 2. Bonus incentive (10%): $40
    // 3. Total value to seize: $440
    // 4. Collateral units to seize: $440 / $2 = 220 units
    let debt_to_liquidate = 40;
    
    // Check return value
    let (liquidated, seized, incentive_debt) = client.liquidate(
        &liquidator, 
        &borrower, 
        &Some(debt_asset.clone()), 
        &Some(collateral_asset.clone()), 
        &debt_to_liquidate
    );

    assert_eq!(liquidated, 40, "Should liquidate exactly requested amount");
    assert_eq!(seized, 220, "Should seize debt_value * (1 + incentive) / collateral_price");
    assert_eq!(incentive_debt, 4, "Incentive in debt asset should be 10% of 40");

    // Verify storage updates
    env.as_contract(&contract_id, || {
        let pos_key = DepositDataKey::Position(borrower.clone());
        let pos: Position = env.storage().persistent().get(&pos_key).unwrap();
        assert_eq!(pos.debt, 60, "Remaining debt mismatch");
        assert_eq!(pos.collateral, 230, "Remaining collateral mismatch (450 - 220)");
        
        let col_key = DepositDataKey::CollateralBalance(borrower.clone());
        let col: i128 = env.storage().persistent().get(&col_key).unwrap();
        assert_eq!(col, 230);
    });
}

#[test]
fn test_liquidate_seizure_capped_by_available_collateral() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract(&env);

    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);

    // One-to-one prices for simplicity in this test
    client.update_price_feed(&admin, &debt_asset, &100_000_000, &8, &admin);
    client.update_price_feed(&admin, &collateral_asset, &100_000_000, &8, &admin);

    client.set_risk_params(&admin, &Some(5000), &None, &None, &Some(1000));

    // Deeply underwater position:
    // Debt: 1000 units
    // Collateral: 100 units (Protocol is in bad debt)
    env.as_contract(&contract_id, || {
        let pos_key = DepositDataKey::Position(borrower.clone());
        env.storage().persistent().set(&pos_key, &Position {
            collateral: 100,
            debt: 1000,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        });
        let col_key = DepositDataKey::CollateralBalance(borrower.clone());
        env.storage().persistent().set(&col_key, &100i128);
    });

    // Liquidate 500 units (50% close factor)
    // Math: $500 * 1.1 = $550 value to seize.
    // BUT only $100 value is available.
    let (liquidated, seized, _incentive) = client.liquidate(
        &liquidator, 
        &borrower, 
        &Some(debt_asset.clone()), 
        &Some(collateral_asset.clone()), 
        &500
    );

    assert_eq!(liquidated, 500);
    assert_eq!(seized, 100, "Should seize all available collateral but no more");

    // Position should have 500 debt and 0 collateral remaining
    env.as_contract(&contract_id, || {
        let pos_key = DepositDataKey::Position(borrower.clone());
        let pos: Position = env.storage().persistent().get(&pos_key).unwrap();
        assert_eq!(pos.debt, 500);
        assert_eq!(pos.collateral, 0);
    });
}

#[test]
#[should_panic(expected = "Error(Contract, #12)")] // LiquidationError::NotLiquidatable (placeholder check)
fn test_liquidate_healthy_position_fails() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract(&env);

    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    // Healthy: Collateral = $200, Debt = $100 -> 200% (Threshold is usually ~105-110%)
    env.as_contract(&contract_id, || {
        let pos_key = DepositDataKey::Position(borrower.clone());
        env.storage().persistent().set(&pos_key, &Position {
            collateral: 200,
            debt: 100,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        });
        let col_key = DepositDataKey::CollateralBalance(borrower.clone());
        env.storage().persistent().set(&col_key, &200i128);
    });

    // Expect fail
    client.liquidate(&liquidator, &borrower, &None, &None, &50);
}

#[test]
fn test_liquidate_close_factor_enforcement_at_limit() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract(&env);

    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    // 1:1 prices
    client.set_risk_params(&admin, &Some(5000), &None, &None, &Some(1000));

    env.as_contract(&contract_id, || {
        let pos_key = DepositDataKey::Position(borrower.clone());
        env.storage().persistent().set(&pos_key, &Position {
            collateral: 1000,
            debt: 1000,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        });
        let col_key = DepositDataKey::CollateralBalance(borrower.clone());
        env.storage().persistent().set(&col_key, &1000i128);
    });

    // Try to liquidate 501 units (CloseFactor is 50%)
    // The hardened code uses .min(max_liquidatable) so it should just liquidate 500 if requested 501, 
    // or we could check if it throws an error if we prefer strict enforcement.
    // The current implementation uses: let actual_debt_liquidated = debt_amount.min(max_liquidatable).min(total_debt);
    // So it should succeed but only liquidate 500.
    
    let (liquidated, _, _) = client.liquidate(&liquidator, &borrower, &None, &None, &501);
    assert_eq!(liquidated, 500, "Should cap liquidation at close factor");
}
