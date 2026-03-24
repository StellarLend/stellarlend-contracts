#![cfg(test)]

use crate::{HelloContract, HelloContractClient};
use crate::deposit::{DepositDataKey, Position};
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env};

extern crate std;
use std::println;
use std::vec::Vec;

struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0xDEAD_BEEF_CAFE_1234 } else { seed })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(hi >= lo);
        lo + (self.next() % (hi - lo + 1))
    }
}

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env.budget().reset_unlimited(); // Prevent budget exhaustion during large loops
    env
}

fn get_user_position(env: &Env, contract_id: &Address, user: &Address) -> Option<Position> {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage().persistent().get::<DepositDataKey, Position>(&key)
    })
}

fn fuzz_round(seed: u64, num_users: usize, max_ops: usize) {
    let env = create_test_env();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    
    // Only try to initialize if it's required (some tests just skip it or initialize implicitly).
    // In repay_test.rs, we see `client.initialize(&admin);` is used.
    client.initialize(&admin);

    let native_asset_addr = env.register_stellar_asset_contract(admin.clone());
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DepositDataKey::NativeAssetAddress, &native_asset_addr);
    });

    let native_token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset_addr);

    let mut rng = Xorshift64::new(seed);
    
    // Create users and give them initial balances
    let mut users = Vec::new();
    let initial_mint = 1_000_000 * 10_000;
    
    for _ in 0..num_users {
        let user = Address::generate(&env);
        native_token_client.mint(&user, &initial_mint);
        native_token_client.approve(&user, &contract_id, &initial_mint, &(env.ledger().sequence() + 100_000));
        users.push(user);
    }

    let scale = 10_000i128; // Using scaling factor

    for _step in 0..max_ops {
        let user_idx = rng.range(0, num_users as u64 - 1) as usize;
        let user = &users[user_idx];
        let op = rng.range(0, 3);
        
        match op {
            0 => { // deposit
                let amount = (rng.range(1, 1000) as i128) * scale;
                let _ = client.try_deposit_collateral(user, &None, &amount);
            }
            1 => { // borrow
                let amount = (rng.range(1, 500) as i128) * scale;
                let _ = client.try_borrow_asset(user, &None, &amount);
            }
            2 => { // repay
                let amount = (rng.range(1, 500) as i128) * scale;
                let _ = client.try_repay_debt(user, &None, &amount);
            }
            3 => { // withdraw
                let amount = (rng.range(1, 500) as i128) * scale;
                let _ = client.try_withdraw_collateral(user, &None, &amount);
            }
            _ => {}
        }

        // Advance ledger occasionally to test interest accrual (1 in 5 chance)
        if rng.range(0, 4) == 0 {
            env.ledger().with_mut(|li| {
                li.timestamp += rng.range(60, 86400 * 7); // up to 1 week
                li.sequence += 1;
            });
        }
        
        // Invariant checks
        if let Some(pos) = get_user_position(&env, &contract_id, user) {
            assert!(pos.collateral >= 0, "INV: Negative collateral balance");
            assert!(pos.debt >= 0, "INV: Negative debt balance");
            
            // Note: Total debt <= Total supply can also be checked if we track global stats or sum up all users.
            // Strict invariants require checking entire state, which is heavy but possible.
        }
    }
}

#[test]
fn test_soroban_env_fuzz_property_based() {
    // Run multiple seeds
    for seed in 1..=20 {
        fuzz_round(seed, 3, 50);
    }
}

#[test]
fn test_soroban_env_fuzz_high_load() {
    fuzz_round(0xCAFE_BABE, 10, 500);
}
