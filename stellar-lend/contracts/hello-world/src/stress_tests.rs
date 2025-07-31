//! Stress tests for StellarLend contract
// Simulate high-frequency actions and state consistency under load

#[cfg(test)]
mod stress_tests {
    use super::*;
    use soroban_sdk::{Env, String};
    use crate::test_utils::TestUtils;
    use rand::Rng;

    #[test]
    fn test_high_frequency_deposits() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        env.as_contract(&contract_id, || {
            for i in 0..5000 {
                let user = TestUtils::create_user_address(&env, i);
                let _ = Contract::deposit_collateral(env.clone(), user.to_string(), 10);
            }
            // Check a few random users
            for i in [0, 100, 4999].iter() {
                let pos = Contract::get_position(env.clone(), TestUtils::create_user_address(&env, *i).to_string()).unwrap();
                assert_eq!(pos.collateral, 10);
            }
        });
    }

    #[test]
    fn test_high_frequency_borrows() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        env.as_contract(&contract_id, || {
            // Deposit collateral for users
            for i in 0..2000 {
                let user = TestUtils::create_user_address(&env, i);
                let _ = Contract::deposit_collateral(env.clone(), user.to_string(), 1000);
            }
            // High-frequency borrows
            for i in 0..2000 {
                let user = TestUtils::create_user_address(&env, i);
                let res = Contract::borrow(env.clone(), user.to_string(), 500);
                assert!(res.is_ok());
            }
            // Check a few users
            for i in [0, 100, 1999].iter() {
                let pos = Contract::get_position(env.clone(), TestUtils::create_user_address(&env, *i).to_string()).unwrap();
                assert_eq!(pos.debt, 500);
            }
        });
    }

    #[test]
    fn test_high_frequency_withdrawals() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        env.as_contract(&contract_id, || {
            // Deposit collateral for users
            for i in 0..1000 {
                let user = TestUtils::create_user_address(&env, i);
                let _ = Contract::deposit_collateral(env.clone(), user.to_string(), 1000);
            }
            // Withdraw for all users
            for i in 0..1000 {
                let user = TestUtils::create_user_address(&env, i);
                let res = Contract::withdraw(env.clone(), user.to_string(), 500);
                assert!(res.is_ok());
            }
            // Check a few users
            for i in [0, 100, 999].iter() {
                let pos = Contract::get_position(env.clone(), TestUtils::create_user_address(&env, *i).to_string()).unwrap();
                assert_eq!(pos.collateral, 500);
            }
        });
    }

    // Add more stress scenarios as needed
}
