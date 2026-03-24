
#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, Vec};

/// Deterministic PRNG for reproducible fuzzing failures.
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

    fn chance(&mut self, num: u64, denom: u64) -> bool {
        self.next() % denom < num
    }
}

/// Setup environment with initialized lending contract
fn setup_fuzz_test(env: &Env) -> (LendingContractClient<'_>, Address, Address, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let asset = Address::generate(env);
    let collateral_asset = Address::generate(env);

    // Initialize with generous limits for fuzz testing
    client.initialize(&admin, &1_000_000_000_000, &1_000);
    client.initialize_deposit_settings(&1_000_000_000_000, &1_000);
    client.initialize_withdraw_settings(&100);

    (client, admin, asset, collateral_asset)
}

fn generate_fuzz_users(env: &Env, count: u32) -> Vec<Address> {
    let mut users = Vec::new(env);
    for _ in 0..count {
        users.push_back(Address::generate(env));
    }
    users
}

fn check_invariants(client: &LendingContractClient<'_>, users: &Vec<Address>, asset: &Address) {
    for user in users.iter() {
        let debt = client.get_user_debt(&user);
        let collateral = client.get_user_collateral(&user);
        let deposit = client.get_user_collateral_deposit(&user, asset);

        // INV: Non-negative balances
        assert!(debt.borrowed_amount >= 0, "Negative debt for user {:?}", user);
        assert!(collateral.amount >= 0, "Negative collateral for user {:?}", user);
        assert!(deposit.amount >= 0, "Negative deposit for user {:?}", user);

        // INV: Debt health factor (if debt exists)
        if debt.borrowed_amount > 0 {
            let position = client.get_user_position(&user);
            // Health factor should be >= 10000 unless we explicitly allow liquidations
            // In a purely adversarial sequence, some users might become liquidatable
            // but the state itself must remain consistent.
            assert!(position.debt_balance >= 0);
            assert!(position.collateral_balance >= 0);
        }
    }
}

fn fuzz_round(seed: u64, num_users: u32, max_ops: u32) {
    let env = Env::default();
    env.mock_all_auths();
    
    let (client, _admin, asset, collateral_asset) = setup_fuzz_test(&env);
    let users = generate_fuzz_users(&env, num_users);
    let mut rng = Xorshift64::new(seed);

    for _ in 0..max_ops {
        let user_idx = rng.range(0, (num_users - 1) as u64) as u32;
        let user = users.get(user_idx).unwrap();
        let op = rng.range(0, 4);

        match op {
            // Deposit
            0 => {
                let amount = rng.range(1_000, 1_000_000_000) as i128;
                let _ = client.try_deposit(&user, &asset, &amount);
            }
            // Borrow
            1 => {
                let amount = rng.range(1_000, 100_000_000) as i128;
                let coll_amount = amount * 2; // Keep it mostly healthy for growth
                let _ = client.try_borrow(&user, &asset, &amount, &collateral_asset, &coll_amount);
            }
            // Repay
            2 => {
                let debt = client.get_user_debt(&user);
                if debt.borrowed_amount > 0 {
                    let amount = rng.range(1, debt.borrowed_amount as u64) as i128;
                    let _ = client.try_repay(&user, &asset, &amount);
                }
            }
            // Withdraw
            3 => {
                let deposit = client.get_user_collateral_deposit(&user, &asset);
                if deposit.amount > 0 {
                    let amount = rng.range(1, deposit.amount as u64) as i128;
                    let _ = client.try_withdraw(&user, &asset, &amount);
                }
            }
            // Advance Ledger (simulating time passing/interest)
            _ => {
                env.ledger().with_mut(|li| {
                    li.timestamp += rng.range(3600, 86400 * 7); // 1h to 1 week
                });
            }
        }

        // Check invariants after every operation
        check_invariants(&client, &users, &asset);
    }
}

#[test]
fn test_fuzz_lending_operations() {
    for seed in 1..=50 {
        fuzz_round(seed, 3, 50);
    }
}

#[test]
fn test_fuzz_high_load_adversarial() {
    // 10 users, 200 operations
    fuzz_round(0xCAFE_BABE_BEEF_D00D, 10, 200);
}

#[test]
fn test_fuzz_edge_cases() {
    for seed in [0, u64::MAX, 0x1234_5678_9ABC_DEF0] {
        fuzz_round(seed, 2, 100);
    }
}
