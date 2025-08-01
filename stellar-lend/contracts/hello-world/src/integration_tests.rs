//! Integration tests for StellarLend contract
// Simulate real user flows and multi-user scenarios

#[cfg(test)]
mod integration_tests {
    use super::*;
    use soroban_sdk::{Env, String};
    use crate::test_utils::TestUtils;

    #[test]
    fn test_deposit_borrow_repay_flow() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        let user = TestUtils::create_user_address(&env, 1);
        env.as_contract(&contract_id, || {
            // Deposit
            let res = Contract::deposit_collateral(env.clone(), user.to_string(), 5000);
            assert!(res.is_ok());
            // Borrow
            let res = Contract::borrow(env.clone(), user.to_string(), 2000);
            assert!(res.is_ok());
            // Repay
            let res = Contract::repay(env.clone(), user.to_string(), 1000);
            assert!(res.is_ok());
            // Check position
            let pos = Contract::get_position(env.clone(), user.to_string()).unwrap();
            assert_eq!(pos.collateral, 5000);
            assert_eq!(pos.debt, 1000);
        });
    }

    #[test]
    fn test_multi_user_interaction() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        let user1 = TestUtils::create_user_address(&env, 1);
        let user2 = TestUtils::create_user_address(&env, 2);
        env.as_contract(&contract_id, || {
            // User 1 deposits and borrows
            Contract::deposit_collateral(env.clone(), user1.to_string(), 3000).unwrap();
            Contract::borrow(env.clone(), user1.to_string(), 1000).unwrap();
            // User 2 deposits and borrows
            Contract::deposit_collateral(env.clone(), user2.to_string(), 4000).unwrap();
            Contract::borrow(env.clone(), user2.to_string(), 2000).unwrap();
            // User 1 repays
            Contract::repay(env.clone(), user1.to_string(), 500).unwrap();
            // User 2 withdraws
            Contract::withdraw(env.clone(), user2.to_string(), 1000).unwrap();
            // Check positions
            let pos1 = Contract::get_position(env.clone(), user1.to_string()).unwrap();
            let pos2 = Contract::get_position(env.clone(), user2.to_string()).unwrap();
            assert_eq!(pos1.collateral, 3000);
            assert_eq!(pos1.debt, 500);
            assert_eq!(pos2.collateral, 3000);
            assert_eq!(pos2.debt, 2000);
        });
    }

    // Add more integration scenarios as needed
}
