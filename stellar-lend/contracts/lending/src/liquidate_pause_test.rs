#[cfg(test)]
mod liquidate_pause_tests {
    use crate::{
        liquidate_transfer_test::MockToken, liquidate_transfer_test::MockTokenClient,
        rounding_strategy::SECONDS_PER_YEAR, EmergencyState, LendingContract,
        LendingContractClient, PauseType,
    };
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        Address, Env,
    };

    fn setup() -> (
        Env,
        LendingContractClient<'static>,
        Address,
        Address,
        Address,
        Address,
        Address,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);
        let debt_asset = env.register(MockToken, ());
        let collateral_asset = env.register(MockToken, ());
        client.initialize(&admin);
        MockTokenClient::new(&env, &debt_asset).mint(&liquidator, &1_000_000);
        MockTokenClient::new(&env, &collateral_asset).mint(&contract_id, &1_000_000);
        // configure a simple asset
        let asset = Address::generate(&env);
        client.set_asset_params(
            &admin,
            &asset,
            &7500,
            &8000,
            &1_000_000_000_000i128,
            &0i128,
            &0i128,
        );
        // Make the borrower unhealthy: deposit 100, borrow the maximum the
        // 80% liquidation-threshold solvency check allows at issuance (80),
        // then let a year of interest accrual push debt just past that
        // threshold. `borrow()` enforces solvency at open time (hf >= 10000),
        // so a position can only become liquidatable after time passes.
        client.deposit(&borrower, &100);
        client.borrow(&borrower, &80);
        let mut info = env.ledger().get();
        info.timestamp = info.timestamp.saturating_add(SECONDS_PER_YEAR);
        info.sequence_number = info.sequence_number.saturating_add(1);
        env.ledger().set(info);
        (
            env,
            client,
            admin,
            borrower,
            liquidator,
            debt_asset,
            collateral_asset,
        )
    }

    #[test]
    #[should_panic(expected = "OperationPaused")]
    fn liquidate_blocked_when_global_pause() {
        let (_env, client, _admin, borrower, liquidator, debt_asset, collateral_asset) = setup();
        client.set_pause(&PauseType::All, &true, &u32::MAX);
        client.liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
    }

    #[test]
    #[should_panic(expected = "OperationPaused")]
    fn liquidate_blocked_when_liquidation_pause_granular() {
        let (_env, client, _admin, borrower, liquidator, debt_asset, collateral_asset) = setup();
        client.set_pause(&PauseType::Liquidation, &true, &u32::MAX);
        client.liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
    }

    #[test]
    fn liquidate_allowed_in_recovery() {
        let (_env, client, _admin, borrower, liquidator, debt_asset, collateral_asset) = setup();
        client.set_emergency_state(&EmergencyState::Recovery);
        client.set_pause(&PauseType::All, &false, &0);
        let res =
            client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
        assert!(res.is_ok());
    }

    #[test]
    fn liquidate_allowed_in_shutdown() {
        let (_env, client, _admin, borrower, liquidator, debt_asset, collateral_asset) = setup();
        client.set_emergency_state(&EmergencyState::Shutdown);
        let res =
            client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
        assert!(res.is_ok(), "liquidation must be allowed during Shutdown");
    }
}
