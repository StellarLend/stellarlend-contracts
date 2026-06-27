#[cfg(test)]
mod liquidate_pause_test {
    use crate::{LendingContract, LendingContractClient, PauseType, EmergencyState};
    use soroban_sdk::{testutils::Address as _, Address, Env};

    fn setup() -> (Env, LendingContractClient<'static>, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);
        client.initialize(&admin);
        // configure a simple asset
        let asset = Address::generate(&env);
        client.set_asset_params(&admin, &asset, &7500, &8000, &1_000_000_000_000i128);
        // make borrower unhealthy: deposit 100, borrow 200
        client.deposit(&borrower, &100);
        client.borrow(&borrower, &200);
        (env, client, admin, borrower, liquidator)
    }

    #[test]
    #[should_panic(expected = "OperationPaused")]
    fn liquidate_blocked_when_global_pause() {
        let (env, client, admin, borrower, liquidator) = setup();
        client.set_pause(&admin, &PauseType::All, &true, &u32::MAX);
        client.liquidate(&liquidator, &borrower, &100);
    }

    #[test]
    #[should_panic(expected = "OperationPaused")]
    fn liquidate_blocked_when_liquidation_pause_granular() {
        let (env, client, admin, borrower, liquidator) = setup();
        client.set_pause(&admin, &PauseType::Liquidation, &true, &u32::MAX);
        client.liquidate(&liquidator, &borrower, &100);
    }

    #[test]
    fn liquidate_allowed_in_recovery() {
        let (env, client, admin, borrower, liquidator) = setup();
        client.set_emergency_state(&admin, &EmergencyState::Recovery);
        client.set_pause(&admin, &PauseType::All, &false, &0);
        let res = client.liquidate(&liquidator, &borrower, &100);
        assert!(res.is_ok());
    }

    #[test]
    #[should_panic(expected = "OperationDisabledDuringShutdown")]
    fn liquidate_blocked_in_shutdown() {
        let (env, client, admin, borrower, liquidator) = setup();
        client.set_emergency_state(&admin, &EmergencyState::Shutdown);
        client.liquidate(&liquidator, &borrower, &100);
    }
}
