// ════════════════════════════════════════════════════════════════
// PERFORMANCE REGRESSION TEST: liquidate Storage Read Budget
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod liquidate_perf_tests {
    use crate::{
        get_storage_read_count, reset_storage_read_count, LendingContract,
        LendingContractClient,
    };
    use soroban_sdk::{
        testutils::Address as _,
        Address, Env,
    };

    fn setup() -> (Env, LendingContractClient<'static>, Address, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let liquidator = Address::generate(&env);
        
        client.initialize(&admin);
        
        (env, client, admin, user, liquidator, id)
    }

    /// ✅ Test Case 1: Minimal Position
    /// Collateral and debt are in the same asset.
    /// Asserts that the storage-read count matches the strict budget of 6 reads.
    #[test]
    fn test_minimal_position_read_budget() {
        let (env, client, _admin, user, liquidator, _) = setup();
        let asset = Address::generate(&env);

        // 1. Setup asset parameters and price
        client.set_asset_params(&asset, &true, &8000, &1000); // 80% CF, 10% bonus
        client.set_asset_price(&asset, &100); // $100 price

        // 2. Setup user position: deposit 100 collateral, borrow 90 debt
        // Since CF is 80%, max borrow is 80. Borrowing 90 makes the position unhealthy.
        client.deposit_asset(&user, &asset, &100);
        client.borrow_asset(&user, &asset, &90);

        // 3. Reset the atomic storage read counter
        reset_storage_read_count();

        // 4. Perform liquidation: liquidator repays 40 debt
        // client.liquidate returns LiquidationResult directly (panics on error)
        let liq_res = client.liquidate(&liquidator, &user, &asset, &asset, &40);

        // 5. Verify the measured storage-read count is exactly 6 (our strict budget)
        let reads = get_storage_read_count();
        
        // Assertions
        assert_eq!(reads, 6, "Storage-read count {} exceeded strict budget of 6", reads);
        
        // Repay 40 debt. Seized collateral value = 40 * 1.1 = 44.
        // At price 100, collateral to seize = 44.
        assert_eq!(liq_res.debt_repaid, 40);
        assert_eq!(liq_res.collateral_seized, 44);
        assert_eq!(liq_res.bad_debt, 0);

        // Verify updated balances
        let remaining_col = client.deposit_asset(&user, &asset, &0);
        let remaining_debt = client.borrow_asset(&user, &asset, &0);
        assert_eq!(remaining_col, 56); // 100 - 44
        assert_eq!(remaining_debt, 50); // 90 - 40
    }

    /// ✅ Test Case 2: Cross-Asset Position
    /// Collateral and debt are in different assets (e.g. XLM collateral, USDC debt).
    /// Asserts that the storage-read count stays within the strict budget of 6 reads.
    #[test]
    fn test_cross_asset_position_read_budget() {
        let (env, client, _admin, user, liquidator, _) = setup();
        let collateral_asset = Address::generate(&env); // e.g. XLM
        let debt_asset = Address::generate(&env);       // e.g. USDC

        // 1. Setup asset parameters
        client.set_asset_params(&collateral_asset, &true, &7500, &1500); // 75% CF, 15% bonus
        client.set_asset_params(&debt_asset, &true, &8000, &1000);

        // 2. Setup asset prices
        client.set_asset_price(&collateral_asset, &10); // XLM = $10
        client.set_asset_price(&debt_asset, &1);       // USDC = $1

        // 3. Setup position: deposit 20 XLM ($200 value), borrow 160 USDC ($160 value)
        // With 75% CF, max borrow is $150. Borrowing $160 makes the position unhealthy.
        client.deposit_asset(&user, &collateral_asset, &20);
        client.borrow_asset(&user, &debt_asset, &160);

        // 4. Reset the atomic storage read counter
        reset_storage_read_count();

        // 5. Perform liquidation: repay 50 USDC
        let liq_res = client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &50);

        // 6. Verify the measured storage-read count is exactly 6
        let reads = get_storage_read_count();
        
        // Assertions
        assert_eq!(reads, 6, "Storage-read count {} exceeded strict budget of 6", reads);
        
        assert_eq!(liq_res.debt_repaid, 50);
        assert_eq!(liq_res.collateral_seized, 5); // 57.5 scaled down/truncated to 5
        assert_eq!(liq_res.bad_debt, 0);

        // Verify updated balances
        let remaining_col = client.deposit_asset(&user, &collateral_asset, &0);
        let remaining_debt = client.borrow_asset(&user, &debt_asset, &0);
        assert_eq!(remaining_col, 15);  // 20 - 5
        assert_eq!(remaining_debt, 110); // 160 - 50
    }

    /// ✅ Test Case 3: Shortfall Path
    /// Collateral value is less than the debt value (insolvent position).
    /// Asserts that the storage-read count stays within the strict budget of 6 reads
    /// and that the bad debt shortfall is correctly recorded.
    #[test]
    fn test_shortfall_path_read_budget() {
        let (env, client, _admin, user, liquidator, _) = setup();
        let collateral_asset = Address::generate(&env);
        let debt_asset = Address::generate(&env);

        // 1. Setup asset parameters
        client.set_asset_params(&collateral_asset, &true, &8000, &1000); // 80% CF, 10% bonus
        client.set_asset_params(&debt_asset, &true, &8000, &1000);

        // 2. Setup asset prices: price of collateral drops dramatically
        client.set_asset_price(&collateral_asset, &1); // Collateral drops to $1
        client.set_asset_price(&debt_asset, &10);     // Debt rises to $10

        // 3. Setup position: deposit 50 collateral ($50 value), borrow 10 debt ($100 value)
        // Highly insolvent position (debt value $100 > collateral value $50).
        client.deposit_asset(&user, &collateral_asset, &50);
        client.borrow_asset(&user, &debt_asset, &10);

        // 4. Reset the atomic storage read counter
        reset_storage_read_count();

        // 5. Perform liquidation: repay 5 debt (max allowed by 50% close factor)
        // Repay 5 debt = $50 value. Seized collateral value (with 10% bonus) = $55.
        // But borrower only has 50 collateral ($50 value)!
        // This triggers the shortfall path.
        let liq_res = client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &5);

        // 6. Verify the measured storage-read count is exactly 6
        let reads = get_storage_read_count();
        
        // Assertions
        assert_eq!(reads, 6, "Storage-read count {} exceeded strict budget of 6", reads);
        
        // Repayed 5 debt. Seized all 50 available collateral.
        assert_eq!(liq_res.debt_repaid, 5);
        assert_eq!(liq_res.collateral_seized, 50);
        
        // Shortfall calculation:
        // Collateral needed = 5 * 10 * 1.1 / 1 = 55.
        // Collateral seized = 50.
        // Shortfall in collateral = 5. Value = $5.
        // Shortfall in debt units = $5 / 10 = 0.
        assert_eq!(liq_res.bad_debt, 0);

        // Verify updated balances
        let remaining_col = client.deposit_asset(&user, &collateral_asset, &0);
        let remaining_debt = client.borrow_asset(&user, &debt_asset, &0);
        assert_eq!(remaining_col, 0);  // All collateral seized
        assert_eq!(remaining_debt, 5); // 10 - 5
    }
}
