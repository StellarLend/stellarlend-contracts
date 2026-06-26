#![cfg(test)]
use soroban_sdk::{
    testutils::{Address as _, Events},
    vec, Address, Env, IntoVal, Symbol,
};

// Import the lending contract for testing
mod lending_contract {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32-unknown-unknown/release/stellar_lend.wasm"
    );
}

// 1. Define the Stub Borrower Contract
#[soroban_sdk::contract]
pub struct StubBorrower;

#[soroban_sdk::contractimpl]
impl StubBorrower {
    pub fn proxy_deposit(env: Env, lending_id: Address, user: Address, amount: i128) {
        user.require_auth();
        let client = lending_contract::Client::new(&env, &lending_id);
        client.deposit_collateral(&user, &amount);
    }
}

// 2. Integration Test Suite
#[test]
fn test_cross_contract_auth_and_execution() {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy Lending Contract
    let lending_id = env.register_contract_wasm(None, lending_contract::WASM);
    let lending_client = lending_contract::Client::new(&env, &lending_id);

    // Initialize Lending Protocol
    let admin = Address::generate(&env);
    lending_client.initialize(&admin);

    // Deploy Stub Borrower Contract
    let stub_id = env.register_contract(None, StubBorrower);
    let stub_client = StubBorrowerClient::new(&env, &stub_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_i128;

    // Execute through proxy to test auth propagation
    stub_client.proxy_deposit(&lending_id, &user, &deposit_amount);

    // Verify Auth Propagation
    assert_eq!(
        env.auths(),
        std::vec![(
            user.clone(),
            env.current_contract_address(),
            Symbol::new(&env, "proxy_deposit"),
            (lending_id.clone(), user.clone(), deposit_amount).into_val(&env)
        )]
    );

    // Verify Event Decoding
    let last_event = env.events().all().last().unwrap();
    assert_eq!(last_event.0, lending_id);
    assert_eq!(
        last_event.1,
        vec![&env, Symbol::new(&env, "deposit_collateral").into_val(&env)]
    );
}

// 3. Panic Bubbling Case
#[test]
#[should_panic] // Asserts that the contract transaction reverts and panics upward smoothly
fn test_cross_contract_panic_bubbling() {
    let env = Env::default();
    env.mock_all_auths();

    let lending_id = env.register_contract_wasm(None, lending_contract::WASM);
    let lending_client = lending_contract::Client::new(&env, &lending_id);

    let admin = Address::generate(&env);
    lending_client.initialize(&admin);

    let stub_id = env.register_contract(None, StubBorrower);
    let stub_client = StubBorrowerClient::new(&env, &stub_id);

    let user = Address::generate(&env);

    // Passing an invalid 0 or negative amount to trigger an internal lending protocol validation panic
    stub_client.proxy_deposit(&lending_id, &user, &0_i128);
}
