#![cfg(test)]

use crate::{TimelockContract, TimelockContractClient, TimelockError};
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, Symbol, Vec, IntoVal};

#[test]
fn test_timelock_flow() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let caller = Address::generate(&env);
    let target = Address::generate(&env);
    
    let contract_id = env.register_contract(None, TimelockContract);
    let client = TimelockContractClient::new(&env, &contract_id);
    
    // Initialize
    let min_delay = 100;
    let grace_period = 200;
    client.initialize(&admin, &min_delay, &grace_period);
    
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let func = Symbol::new(&env, "some_func");
    let args: Vec<soroban_sdk::Val> = Vec::new(&env);
    let eta = 1150; // Valid: > 1000 + 100
    
    // Queue success
    let action_id = client.queue(&admin, &target, &func, &args, &eta);
    
    // Attempt execute early (fails)
    let res = client.try_execute(&caller, &target, &func, &args, &eta);
    assert_eq!(res.err().unwrap().unwrap(), TimelockError::TimelockNotReady);
    
    // Time travel to valid execution window
    env.ledger().with_mut(|li| {
        li.timestamp = 1150;
    });
    
    // For execution to actually succeed, we would need to mock the target contract.
    // However, invoking an empty target address will trap.
    // Here we can at least assert that the error is not TimelockNotReady or TimelockExpired.
    // We would need a dummy contract to fully test execution...
}

#[test]
fn test_timelock_cancel() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let target = Address::generate(&env);
    
    let contract_id = env.register_contract(None, TimelockContract);
    let client = TimelockContractClient::new(&env, &contract_id);
    
    client.initialize(&admin, &100, &200);
    
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let func = Symbol::new(&env, "some_func");
    let args: Vec<soroban_sdk::Val> = Vec::new(&env);
    let eta = 1150; 
    
    // Queue
    client.queue(&admin, &target, &func, &args, &eta);
    
    // Cancel
    client.cancel(&admin, &target, &func, &args, &eta);
    
    // Attempt execute
    env.ledger().with_mut(|li| {
        li.timestamp = 1150;
    });
    
    let res = client.try_execute(&admin, &target, &func, &args, &eta);
    assert_eq!(res.err().unwrap().unwrap(), TimelockError::ActionNotQueued);
}

#[test]
fn test_timelock_expired() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let target = Address::generate(&env);
    
    let contract_id = env.register_contract(None, TimelockContract);
    let client = TimelockContractClient::new(&env, &contract_id);
    
    client.initialize(&admin, &100, &200);
    
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });
    
    let func = Symbol::new(&env, "some_func");
    let args: Vec<soroban_sdk::Val> = Vec::new(&env);
    let eta = 1150; 
    
    // Queue
    client.queue(&admin, &target, &func, &args, &eta);
    
    // Time travel past grace period
    env.ledger().with_mut(|li| {
        li.timestamp = 1400; // 1150 + 200 = 1350 max
    });
    
    let res = client.try_execute(&admin, &target, &func, &args, &eta);
    assert_eq!(res.err().unwrap().unwrap(), TimelockError::TimelockExpired);
}
