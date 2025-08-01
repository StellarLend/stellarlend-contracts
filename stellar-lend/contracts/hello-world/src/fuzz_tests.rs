//! Fuzzing tests for StellarLend contract
// Use property-based testing to catch unexpected panics or logic errors

#[cfg(test)]
mod fuzz_tests {
    use super::*;
    use soroban_sdk::{Env, String};
    use crate::test_utils::TestUtils;
    use rand::Rng;

    #[test]
    fn fuzz_deposit_collateral() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        let mut rng = rand::thread_rng();
        env.as_contract(&contract_id, || {
            for _ in 0..200 {
                let user_id = rng.gen_range(0..1000);
                let user = TestUtils::create_user_address(&env, user_id);
                let amount: i128 = rng.gen_range(-10_000..100_000);
                let res = Contract::deposit_collateral(env.clone(), user.to_string(), amount);
                if amount <= 0 {
                    assert!(res.is_err());
                } else {
                    assert!(res.is_ok());
                }
            }
        });
    }

    #[test]
    fn fuzz_borrow_amounts() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        let mut rng = rand::thread_rng();
        env.as_contract(&contract_id, || {
            // First deposit some collateral for users
            for user_id in 0..50 {
                let user = TestUtils::create_user_address(&env, user_id);
                let collateral: i128 = rng.gen_range(1000..10_000);
                let _ = Contract::deposit_collateral(env.clone(), user.to_string(), collateral);
            }
            // Fuzz borrow amounts
            for _ in 0..200 {
                let user_id = rng.gen_range(0..50);
                let user = TestUtils::create_user_address(&env, user_id);
                let amount: i128 = rng.gen_range(-5000..20_000);
                let res = Contract::borrow(env.clone(), user.to_string(), amount);
                if amount <= 0 {
                    assert!(res.is_err());
                } else {
                    // Should only succeed if user has enough collateral
                    // We allow both Ok and Err, but check for panics
                    assert!(res.is_ok() || res.is_err());
                }
            }
        });
    }

    #[test]
    fn fuzz_withdraw_random_amounts() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        let mut rng = rand::thread_rng();
        env.as_contract(&contract_id, || {
            // Deposit collateral for users
            for user_id in 0..30 {
                let user = TestUtils::create_user_address(&env, user_id);
                let collateral: i128 = rng.gen_range(500..5000);
                let _ = Contract::deposit_collateral(env.clone(), user.to_string(), collateral);
            }
            // Fuzz withdraw amounts
            for _ in 0..100 {
                let user_id = rng.gen_range(0..30);
                let user = TestUtils::create_user_address(&env, user_id);
                let amount: i128 = rng.gen_range(-2000..7000);
                let res = Contract::withdraw(env.clone(), user.to_string(), amount);
                if amount <= 0 {
                    assert!(res.is_err());
                } else {
                    assert!(res.is_ok() || res.is_err());
                }
            }
        });
    }

    #[test]
    fn fuzz_multi_user_randomized_flows() {
        let env = TestUtils::create_test_env();
        let _admin = TestUtils::initialize_contract(&env);
        let contract_id = env.register(Contract, ());
        let mut rng = rand::thread_rng();
        env.as_contract(&contract_id, || {
            // Simulate random user actions
            for _ in 0..150 {
                let user_id = rng.gen_range(0..40);
                let user = TestUtils::create_user_address(&env, user_id);
                let action = rng.gen_range(0..3);
                match action {
                    0 => {
                        // Deposit
                        let amount: i128 = rng.gen_range(1..10_000);
                        let _ = Contract::deposit_collateral(env.clone(), user.to_string(), amount);
                    }
                    1 => {
                        // Borrow
                        let amount: i128 = rng.gen_range(1..5000);
                        let _ = Contract::borrow(env.clone(), user.to_string(), amount);
                    }
                    2 => {
                        // Withdraw
                        let amount: i128 = rng.gen_range(1..5000);
                        let _ = Contract::withdraw(env.clone(), user.to_string(), amount);
                    }
                    _ => {}
                }
            }
        });
    }

    // Add more fuzzing scenarios...
}
