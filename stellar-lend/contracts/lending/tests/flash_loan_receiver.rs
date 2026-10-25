#![cfg(test)]
#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env, Symbol};

/// Honest flash loan receiver that repays the full amount + fee.
#[contract]
pub struct HonestReceiver;

#[contractimpl]
impl HonestReceiver {
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        // Transfer back amount + fee to the lending contract
        let token = soroban_sdk::token::Client::new(&env, &asset);
        let lending_contract = env.invoker();
        let total_repay = amount + fee;
        token.transfer(&env.current_contract_address(), &lending_contract, &total_repay);
    }
}

/// Underpaying receiver that returns less than amount + fee.
#[contract]
pub struct UnderpayingReceiver;

#[contractimpl]
impl UnderpayingReceiver {
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        // Return only half of what's required
        let token = soroban_sdk::token::Client::new(&env, &asset);
        let lending_contract = env.invoker();
        let partial_repay = (amount + fee) / 2;
        token.transfer(&env.current_contract_address(), &lending_contract, &partial_repay);
    }
}

/// Reentrant receiver that attempts to call deposit during the flash loan callback.
#[contract]
pub struct ReentrantReceiver;

#[contractimpl]
impl ReentrantReceiver {
    pub fn on_flash_loan(
        env: Env,
        _initiator: Address,
        asset: Address,
        amount: i128,
        fee: i128,
        _params: Bytes,
    ) {
        let lending_contract = env.invoker();
        let user = env.current_contract_address();
        
        // Try to call deposit on the lending contract (should fail with reentrancy error)
        let _result: Result<(), soroban_sdk::Val> = env.try_invoke_contract(
            &lending_contract,
            &Symbol::new(&env, "deposit"),
            soroban_sdk::vec![&env, user.into_val(&env), 100i128.into_val(&env)],
        );
        
        // Still repay to avoid other errors
        let token = soroban_sdk::token::Client::new(&env, &asset);
        let total_repay = amount + fee;
        token.transfer(&env.current_contract_address(), &lending_contract, &total_repay);
    }
}
