use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

#[contract]
pub struct MockOracleLiquidation;

#[contractimpl]
impl MockOracleLiquidation {
    pub fn price(_env: Env, _asset: Address) -> i128 {
        // Return 100.0 for debt, 50.0 for collateral (to trigger liquidation)
        // Actually, let's make them both 1.0 (100_000_000) and then the threshold will trigger it.
        100_000_000
    }
}

fn setup_liquidation_test(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let liquidator = Address::generate(env);
    let borrower = Address::generate(env);
    let debt_asset = Address::generate(env);
    let collateral_asset = Address::generate(env);

    client.initialize(&admin, &i128::MAX, &0);
    
    let oracle_id = env.register(MockOracleLiquidation, ());
    client.set_oracle(&admin, &oracle_id);
    
    // Set 50% close factor, 10% incentive
    client.set_close_factor_bps(&admin, &5000);
    client.set_liquidation_incentive_bps(&admin, &1000);
    
    (client, admin, liquidator, borrower, debt_asset, collateral_asset)
}

#[test]
fn test_liquidation_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) = setup_liquidation_test(&env);

    // 1. Borrower takes a loan
    // LT 80%. Collateral 15_000, Debt 10_000. 
    // Weighted = 15_000 * 0.8 = 12_000. HF = 1.2
    client.set_liquidation_threshold_bps(&admin, &8000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);
    
    assert_eq!(client.get_health_factor(&borrower), 12_000);

    // 2. Drop LT to 40% to make it liquidatable
    // Weighted = 15_000 * 0.4 = 6_000. HF = 0.6
    client.set_liquidation_threshold_bps(&admin, &4000);
    assert_eq!(client.get_health_factor(&borrower), 6000);

    // 3. Liquidator repays 5_000 (Exactly 50% close factor)
    let (repaid, seized) = client.liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &5_000);
    
    assert_eq!(repaid, 5_000);
    // seizure = 5_000 * (1.1) = 5_500
    assert_eq!(seized, 5_500);

    // 4. Check balances
    let debt = client.get_user_debt(&borrower);
    assert_eq!(debt.borrowed_amount, 5_000);
    
    let collat = client.get_user_collateral(&borrower);
    assert_eq!(collat.amount, 15_000 - 5_500);
}

#[test]
fn test_liquidation_exceeds_close_factor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) = setup_liquidation_test(&env);

    client.set_liquidation_threshold_bps(&admin, &4000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &15_000);
    
    // Try to repay 6_000 (above 50% of 10_000)
    let result = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &6_000);
    assert_eq!(result, Err(Ok(BorrowError::ExceedsCloseFactor)));
}

#[test]
fn test_liquidation_not_liquidatable() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, liquidator, borrower, debt_asset, collateral_asset) = setup_liquidation_test(&env);

    client.set_liquidation_threshold_bps(&admin, &8000);
    client.borrow(&borrower, &debt_asset, &10_000, &collateral_asset, &20_000);
    
    let result = client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &5_000);
    assert_eq!(result, Err(Ok(BorrowError::NotLiquidatable)));
}
