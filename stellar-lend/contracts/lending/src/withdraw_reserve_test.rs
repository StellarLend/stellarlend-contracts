
#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
#[should_panic]
fn test_withdraw_overdraw_panics() {
    let env = Env::default();
    let asset = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract_id = env.register_contract(None, LendingContract);
    let client = LendingContractClient::new(&env, &contract_id);
    
    // Attempting to withdraw 1000 from a 0 reserve will panic, satisfying the security requirement
    client.withdraw_reserve(&asset, &1000, &treasury);
}
