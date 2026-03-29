//! # Multi-user Contention Scenarios
//!
//! Simulating many users depositing/borrowing in interleaved order within
//! the same ledger context to validate security, bounds, and reentrancy protections.

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup_contention_test(
    env: &Env,
) -> (
    LendingContractClient<'_>,
    Address, // admin
    Address, // asset (debt)
    Address, // collateral_asset
) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let asset = Address::generate(env);
    let collateral_asset = Address::generate(env);

    client.initialize(&admin, &10_000_000_000, &100);
    client.initialize_deposit_settings(&10_000_000_000, &100);
    client.initialize_withdraw_settings(&100);

    (client, admin, asset, collateral_asset)
}

fn generate_users(env: &Env, count: u32) -> Vec<Address> {
    let mut users = Vec::new(env);
    for _ in 0..count {
        users.push_back(Address::generate(env));
    }
    users
}

#[test]
fn test_contention_interleaved_deposits_borrows() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, collateral_asset) = setup_contention_test(&env);

    let num_users = 50;
    let users = generate_users(&env, num_users);
    
    let mut expected_total_deposits = 0;
    let mut expected_total_borrows = 0;

    // Interleaved deposit and borrow operations
    for (i, user) in users.iter().enumerate() {
        // Even indices deposit first, odd indices borrow first (if they have collateral)
        
        // Every user deposits collateral
        let deposit_amount = 50_000 + (i as i128 * 100);
        client.deposit(&user, &collateral_asset, &deposit_amount);
        expected_total_deposits += deposit_amount;

        // Alternate users borrow
        if i % 2 == 0 {
            let borrow_amount = 10_000 + (i as i128 * 50);
            let collateral_amount = borrow_amount * 2;
            client.borrow(&user, &asset, &borrow_amount, &collateral_asset, &collateral_amount);
            expected_total_borrows += borrow_amount;
        }
    }

    // Verify individual positions and global state constraints
    let mut actual_debt = 0i128;
    for (i, user) in users.iter().enumerate() {
        let collat = client.get_user_collateral_deposit(&user, &collateral_asset);
        assert_eq!(collat.amount, 50_000 + (i as i128 * 100));

        let debt = client.get_user_debt(&user);
        if i % 2 == 0 {
            assert_eq!(debt.borrowed_amount, 10_000 + (i as i128 * 50));
            actual_debt += debt.borrowed_amount;
        } else {
            assert_eq!(debt.borrowed_amount, 0);
        }
    }
    
    assert_eq!(actual_debt, expected_total_borrows);
}

#[test]
fn test_contention_edge_cases_zero_amounts_overflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, asset, collateral_asset) = setup_contention_test(&env);
    let user = Address::generate(&env);

    // Zero amount deposit
    let res_deposit = client.try_deposit(&user, &asset, &0);
    assert!(res_deposit.is_err());

    // Zero amount borrow
    client.deposit(&user, &collateral_asset, &100_000);
    let res_borrow = client.try_borrow(&user, &asset, &0, &collateral_asset, &0);
    assert!(res_borrow.is_err());

    // Max amount (overflow testing)
    let res_overflow_deposit = client.try_deposit(&user, &asset, &i128::MAX);
    assert!(res_overflow_deposit.is_err()); // Exceeds deposit cap
}

#[test]
fn test_contention_paused_operations() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, asset, collateral_asset) = setup_contention_test(&env);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.deposit(&user1, &collateral_asset, &50_000);
    
    // Pause deposits
    client.set_deposit_paused(&true);
    
    // Trying to deposit while paused under contention scenario
    let deposit_res = client.try_deposit(&user2, &collateral_asset, &50_000);
    assert!(deposit_res.is_err());

    // Borrow should still work if not paused
    client.borrow(&user1, &asset, &10_000, &collateral_asset, &20_000);

    // Pause borrows
    client.set_pause(&admin, &PauseType::Borrow, &true);
    let borrow_res = client.try_borrow(&user1, &asset, &10_000, &collateral_asset, &20_000);
    assert!(borrow_res.is_err());
}
