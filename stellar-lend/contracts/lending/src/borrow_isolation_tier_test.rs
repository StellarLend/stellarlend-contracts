//! Borrow isolation tier test suite for LendingContract.
//!
//! Covers isolation-tier interactions with borrow entrypoints:
//! - Setting and querying asset isolation parameters
//! - Enforcing debt ceilings on isolated assets during borrow
//! - Tracking cumulative isolation debt across multiple borrow calls
//! - Resetting isolation debt on full repayment
//! - Freeing capacity after partial or full repayment
//! - Immediate enforcement of updated isolation debt ceilings

#[cfg(test)]
mod borrow_isolation_tier_tests {
    use crate::{LendingContract, LendingContractClient, LendingError};
    use soroban_sdk::{testutils::Address as _, Address, Env};

    fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, user)
    }

    fn make_asset(env: &Env) -> Address {
        Address::generate(env)
    }

    #[test]
    fn test_isolation_tier_unset_by_default() {
        let (env, client, _admin, _user) = setup();
        let tok = make_asset(&env);
        assert!(client.get_asset_isolation(&tok).is_none());
        assert_eq!(client.get_isolation_debt(&tok), 0);
    }

    #[test]
    fn test_admin_can_set_and_read_isolation_tier() {
        let (env, client, _admin, _user) = setup();
        let tok = make_asset(&env);
        let ceiling = 500_000i128;

        client.set_asset_isolation(&tok, &true, &ceiling);

        let cfg = client
            .get_asset_isolation(&tok)
            .expect("isolation config should be set");
        assert!(cfg.isolated);
        assert_eq!(cfg.isolation_debt_ceiling, ceiling);
    }

    #[test]
    fn test_borrow_within_isolation_ceiling_succeeds() {
        let (env, client, _admin, user) = setup();
        let tok = make_asset(&env);
        client.set_asset_isolation(&tok, &true, &10_000i128);

        let result = client.borrow_against_collateral(&user, &5_000i128, &tok);
        assert_eq!(result, 5_000);
        assert_eq!(client.get_isolation_debt(&tok), 5_000);
    }

    #[test]
    fn test_borrow_exceeding_isolation_ceiling_rejected() {
        let (env, client, _admin, user) = setup();
        let tok = make_asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        let res = client.try_borrow_against_collateral(&user, &1_001i128, &tok);
        assert!(matches!(
            res,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));
    }

    #[test]
    fn test_cumulative_borrows_reach_isolation_ceiling() {
        let (env, client, _admin, user) = setup();
        let tok = make_asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        client.borrow_against_collateral(&user, &600i128, &tok);
        let res = client.try_borrow_against_collateral(&user, &401i128, &tok);
        assert!(matches!(
            res,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));

        // Borrowing exact remaining capacity succeeds
        let total = client.borrow_against_collateral(&user, &400i128, &tok);
        assert_eq!(total, 1_000);
        assert_eq!(client.get_isolation_debt(&tok), 1_000);
    }

    #[test]
    fn test_repay_frees_isolation_tier_capacity() {
        let (env, client, _admin, user) = setup();
        let tok = make_asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        client.borrow_against_collateral(&user, &1_000i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 1_000);

        client.repay_against_collateral(&user, &400i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 600);

        let total = client.borrow_against_collateral(&user, &400i128, &tok);
        assert_eq!(total, 1_000);
    }

    #[test]
    fn test_full_repay_resets_isolation_debt() {
        let (env, client, _admin, user) = setup();
        let tok = make_asset(&env);
        client.set_asset_isolation(&tok, &true, &2_000i128);

        client.borrow_against_collateral(&user, &1_500i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 1_500);

        client.repay_against_collateral(&user, &1_500i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 0);
    }

    #[test]
    fn test_disabling_isolation_removes_ceiling_check() {
        let (env, client, _admin, user) = setup();
        let tok = make_asset(&env);
        client.set_asset_isolation(&tok, &true, &100i128);

        let res = client.try_borrow_against_collateral(&user, &200i128, &tok);
        assert!(matches!(
            res,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));

        client.set_asset_isolation(&tok, &false, &100i128);

        let result = client.borrow_against_collateral(&user, &200i128, &tok);
        assert_eq!(result, 200);
    }
}
