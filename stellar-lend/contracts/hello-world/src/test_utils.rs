//! Test utilities for StellarLend contract
// Helpers for creating mock environments, addresses, and contract state

use soroban_sdk::{Env, Address, String};

pub struct TestUtils;

impl TestUtils {
    pub fn create_test_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    pub fn create_test_address(env: &Env, address_str: &str) -> Address {
        Address::from_string(env, &String::from_str(env, address_str))
    }

    pub fn create_admin_address(env: &Env) -> Address {
        Self::create_test_address(env, "admin")
    }

    pub fn create_user_address(env: &Env, user_id: u32) -> Address {
        Self::create_test_address(env, &format!("user{}", user_id))
    }

    // Add more helpers as needed
}
