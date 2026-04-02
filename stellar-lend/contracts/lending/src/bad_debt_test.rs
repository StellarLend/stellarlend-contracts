#[cfg(test)]
mod test {
    use crate::{LendingContract, LendingContractClient};
    use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

    #[test]
    fn test_bad_debt_accounting_auto_offset() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);
        let asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);

        let client =
            LendingContractClient::new(&env, &env.register_contract(None, LendingContract));

        // Initialize protocol
        client.initialize(&admin, &1_000_000, &10);

        // 1. Credit Insurance Fund with 50 units
        client.credit_insurance_fund(&admin, &asset, &50);
        assert_eq!(client.get_insurance_fund_balance(&asset), 50);

        // 2. Setup a borrow position for the borrower
        // We deposit enough to pass the initial ratio check, then advance time
        // to accrue interest until the position becomes insolvent.
        client.deposit_collateral(&borrower, &collateral_asset, &150);
        client.borrow(&borrower, &asset, &100, &collateral_asset, &0);

        // Advance time by 20 years to accrue ~100% interest (at 5% simple)
        // Debt will be 100 (principal) + 100 (interest) = 200.
        // Collateral is still 150. Shortfall = 200 - 150 = 50.
        env.ledger().with_mut(|li| {
            li.timestamp += 20 * 31536000;
        });

        // Verify initial state
        assert_eq!(client.get_total_bad_debt(&asset), 0);

        // 3. Liquidate: Repay all debt (200), but only 150 collateral exists.
        // Repay amount 200. Collateral 150. Shortfall = 50.
        client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &200);

        // 4. Verify Accounting
        // Shortfall of 50 should have been created.
        // Since Insurance Fund had 50, it should auto-offset all of it.
        // Final Bad Debt: 0. Final Fund Balance: 0.
        assert_eq!(client.get_total_bad_debt(&asset), 0);
        assert_eq!(client.get_insurance_fund_balance(&asset), 0);
    }

    #[test]
    fn test_bad_debt_exceeding_insurance_fund() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);
        let asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);

        let client =
            LendingContractClient::new(&env, &env.register_contract(None, LendingContract));

        // Initialize protocol
        client.initialize(&admin, &1_000_000, &10);

        // 1. Credit Insurance Fund with 10 units
        client.credit_insurance_fund(&admin, &asset, &10);

        // 2. Setup insolvent position: Debt ~200, Collateral 150 -> Shortfall 50
        client.deposit_collateral(&borrower, &collateral_asset, &150);
        client.borrow(&borrower, &asset, &100, &collateral_asset, &0);

        // Advance time by 20 years to accrue ~100 interest
        env.ledger().with_mut(|li| {
            li.timestamp += 20 * 31536000;
        });

        // 3. Liquidate
        // Total debt is 200. Collateral is 150. Offset from fund is 10.
        // Expected Bad Debt = (200 - 150) - 10 = 40.
        client.liquidate(&liquidator, &borrower, &asset, &collateral_asset, &200);

        // 4. Verify Accounting
        assert_eq!(client.get_total_bad_debt(&asset), 40);
        assert_eq!(client.get_insurance_fund_balance(&asset), 0);

        // 5. Manual offset: Credit fund with 50 and offset the remaining 40 bad debt
        client.credit_insurance_fund(&admin, &asset, &50);
        assert_eq!(client.get_insurance_fund_balance(&asset), 50);

        client.offset_bad_debt(&admin, &asset, &40);

        assert_eq!(client.get_total_bad_debt(&asset), 0);
        assert_eq!(client.get_insurance_fund_balance(&asset), 10);
    }
}
