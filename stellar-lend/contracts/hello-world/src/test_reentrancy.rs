#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, Address, Env, Symbol,
};
use crate::{HelloContract, HelloContractClient};

#[contract]
pub struct MaliciousToken;

#[contractimpl]
impl MaliciousToken {
    pub fn balance(_env: Env, _id: Address) -> i128 {
        1_000_000 // Always return enough balance
    }

    pub fn transfer_from(env: Env, _spender: Address, from: Address, _to: Address, _amount: i128) {
        Self::attempt_reentrancy(&env, &from);
    }
    
    pub fn transfer(env: Env, _from: Address, to: Address, _amount: i128) {
        Self::attempt_reentrancy(&env, &to);
    }
}

impl MaliciousToken {
    fn attempt_reentrancy(env: &Env, user: &Address) {
        // Retrieve the HelloContract address from temporary storage
        let target_key = Symbol::new(env, "TEST_TARGET");
        if let Some(target) = env.storage().temporary().get::<Symbol, Address>(&target_key) {
            let client = HelloContractClient::new(env, &target);
            let token_opt = Some(env.current_contract_address());
            
            // Try deposit
            let res = client.try_deposit_collateral(user, &token_opt, &100);
            assert!(res.is_err(), "Expected Reentrancy error on deposit, got {:?}", res);

            // Try withdraw
            let res = client.try_withdraw_collateral(user, &token_opt, &100);
            assert!(res.is_err(), "Expected Reentrancy error on withdraw, got {:?}", res);
            
            // Try borrow
            let res = client.try_borrow_asset(user, &token_opt, &100);
            assert!(res.is_err(), "Expected Reentrancy error on borrow, got {:?}", res);

            // Try repay
            let res = client.try_repay_debt(user, &token_opt, &100);
            assert!(res.is_err(), "Expected Reentrancy error on repay, got {:?}", res);
        }
    }
}

fn setup_test(env: &Env) -> (Address, HelloContractClient<'static>, Address, Address) {
    env.mock_all_auths();
    
    let admin = Address::generate(env);
    let user = Address::generate(env);
    
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    
    client.initialize(&admin);
    
    // Register malicious token
    let malicious_token_id = env.register(MaliciousToken, ());
    
    // Set target for the malicious token to use
    let target_key = Symbol::new(env, "TEST_TARGET");
    env.as_contract(&malicious_token_id, || {
        env.storage().temporary().set(&target_key, &contract_id);
    });

    // Set asset params
    env.as_contract(&contract_id, || {
        use crate::deposit::{AssetParams, DepositDataKey};
        let key = DepositDataKey::AssetParams(malicious_token_id.clone());
        env.storage().persistent().set(&key, &AssetParams {
            deposit_enabled: true,
            collateral_factor: 10000,
            max_deposit: 10_000_000,
        });
    });
    
    let static_client = unsafe { 
        core::mem::transmute::<HelloContractClient<'_>, HelloContractClient<'static>>(client) 
    };

    (contract_id, static_client, malicious_token_id, user)
}

#[test]
fn test_reentrancy_on_deposit() {
    let env = Env::default();
    let (_, client, token_id, user) = setup_test(&env);
    
    client.deposit_collateral(&user, &Some(token_id), &1000);
}

#[test]
fn test_reentrancy_on_withdraw() {
    let env = Env::default();
    let (contract_id, client, token_id, user) = setup_test(&env);
    
    env.as_contract(&contract_id, || {
        use crate::deposit::{DepositDataKey, Position};
        env.storage().persistent().set(&DepositDataKey::CollateralBalance(user.clone()), &1000_i128);
        env.storage().persistent().set(&DepositDataKey::Position(user.clone()), &Position {
            collateral: 1000,
            debt: 0,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        });
    });

    client.withdraw_collateral(&user, &Some(token_id), &500);
}

#[test]
fn test_reentrancy_on_borrow() {
    let env = Env::default();
    let (contract_id, client, token_id, user) = setup_test(&env);
    
    env.as_contract(&contract_id, || {
        use crate::deposit::{DepositDataKey, Position};
        env.storage().persistent().set(&DepositDataKey::CollateralBalance(user.clone()), &10000_i128);
        env.storage().persistent().set(&DepositDataKey::Position(user.clone()), &Position {
            collateral: 10000,
            debt: 0,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        });
    });

    client.borrow_asset(&user, &Some(token_id), &500);
}

#[test]
fn test_reentrancy_on_repay() {
    let env = Env::default();
    let (contract_id, client, token_id, user) = setup_test(&env);
    
    env.as_contract(&contract_id, || {
        use crate::deposit::{DepositDataKey, Position};
        env.storage().persistent().set(&DepositDataKey::Position(user.clone()), &Position {
            collateral: 10000,
            debt: 1000,
            borrow_interest: 0,
            last_accrual_time: env.ledger().timestamp(),
        });
    });

    client.repay_debt(&user, &Some(token_id), &500);
}
