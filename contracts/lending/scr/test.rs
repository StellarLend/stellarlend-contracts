#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::{Ledger, LedgerInfo}, Env, Address};

fn setup_test_environment(e: &Env) -> (Address, Address, Address, Address) {
    let admin = Address::generate(&e);
    let oracle = Address::generate(&e);
    let token_a = Address::generate(&e);
    let token_b = Address::generate(&e);

    LendingContract::initialize(e.clone(), admin.clone());
    LendingContract::set_oracle(e.clone(), admin.clone(), oracle.clone());
    
    (admin, oracle, token_a, token_b)
}

#[test]
fn test_fresh_price_valuation_succeeds() {
    let e = Env::default();
    e.ledger().set(LedgerInfo {
        timestamp: 10000,
        protocol_version: 20,
        sequence_number: 1,
        network_id: [0; 32],
        base_reserve: 10,
    });

    let (_, oracle, token_a, _) = setup_test_environment(&e);
    LendingContract::update_price_feed(e.clone(), oracle.clone(), token_a.clone(), 150_0000000, 10000, 7);

    let evaluated_price = LendingContract::get_price(e.clone(), token_a);
    assert_eq!(evaluated_price, 150_0000000);
}

#[test]
#[should_panic(expected = "Oracle price rejection: Data stream bounds breach staleness limits")]
fn test_stale_price_data_triggers_panic() {
    let e = Env::default();
    e.ledger().set(LedgerInfo {
        timestamp: 20000, // Move ledger beyond configured limits
        protocol_version: 20,
        sequence_number: 1,
        network_id: [0; 32],
        base_reserve: 10,
    });

    let (admin, oracle, token_a, _) = setup_test_environment(&e);
    LendingContract::set_max_age(e.clone(), admin, token_a.clone(), 300); // 5 min allowance window
    LendingContract::update_price_feed(e.clone(), oracle, token_a.clone(), 100, 19000, 7); // Age = 1000s > 300s

    LendingContract::get_price(e.clone(), token_a);
}

#[test]
fn test_decimal_mismatch_scaling_normalization() {
    let e = Env::default();
    e.ledger().set(LedgerInfo {
        timestamp: 10000,
        protocol_version: 20,
        sequence_number: 1,
        network_id: [0; 32],
        base_reserve: 10,
    });

    let (_, oracle, token_a, token_b) = setup_test_environment(&e);
    
    // Test upward normalization from 4 decimals up to internal standard (7 decimals)
    LendingContract::update_price_feed(e.clone(), oracle.clone(), token_a.clone(), 1200, 10000, 4);
    let scale_up = LendingContract::get_price(e.clone(), token_a);
    assert_eq!(scale_up, 1200000);

    // Test downward normalization from 9 decimals down to internal standard (7 decimals)
    LendingContract::update_price_feed(e.clone(), oracle, token_b.clone(), 500000000, 10000, 9);
    let scale_down = LendingContract::get_price(e.clone(), token_b);
    assert_eq!(scale_down, 5000000);
}