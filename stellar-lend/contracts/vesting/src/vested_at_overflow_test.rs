#![cfg(test)]

use soroban_sdk::{Address, Env, testutils::Address as _};
use crate::Grant;

#[test]
fn test_typical_schedule_exact() {
    let env = Env::default();
    let grantee = Address::generate(&env);
    
    let grant = Grant {
        grantee,
        total_amount: 10_000,
        claimed_amount: 0,
        released_amount: 0,
        start_ts: 1000,
        cliff_secs: 100,
        duration_secs: 1000,
        revoked: false,
    };
    
    // Before start
    assert_eq!(grant.vested_at(500), 0);
    // During cliff
    assert_eq!(grant.vested_at(1050), 0);
    // At cliff end (1100)
    assert_eq!(grant.vested_at(1100), 1000); // 100/1000 * 10000 = 1000
    // Mid point
    assert_eq!(grant.vested_at(1500), 5000); // 500/1000 * 10000 = 5000
    // At end
    assert_eq!(grant.vested_at(2000), 10000);
    // Past end
    assert_eq!(grant.vested_at(3000), 10000);
}

#[test]
fn test_overflow_avoided_with_max_values() {
    let env = Env::default();
    let grantee = Address::generate(&env);
    
    // Max principal and max duration combinations
    let grant = Grant {
        grantee,
        total_amount: i128::MAX, // ~1.7e38
        claimed_amount: 0,
        released_amount: 0,
        start_ts: 0,
        cliff_secs: 0,
        duration_secs: u64::MAX, // ~1.8e19
        revoked: false,
    };
    
    // At t = 0
    assert_eq!(grant.vested_at(0), 0);
    // At t = u64::MAX / 2
    let mid_ts = u64::MAX / 2;
    let mid_vested = grant.vested_at(mid_ts);
    assert!(mid_vested > 0);
    assert!(mid_vested < i128::MAX);
    // At t = u64::MAX
    assert_eq!(grant.vested_at(u64::MAX), i128::MAX);
    // Past end
    assert_eq!(grant.vested_at(u64::MAX), i128::MAX);
}

#[test]
fn test_never_exceeds_principal() {
    let env = Env::default();
    let grantee = Address::generate(&env);
    
    let grant = Grant {
        grantee,
        total_amount: 5000,
        claimed_amount: 0,
        released_amount: 0,
        start_ts: 100,
        cliff_secs: 0,
        duration_secs: 1000,
        revoked: false,
    };
    
    // Far past the end
    assert_eq!(grant.vested_at(u64::MAX), 5000);
    assert_eq!(grant.vested_at(2000), 5000);
}

#[test]
fn test_no_panic_for_various_combinations() {
    let env = Env::default();
    let grantee = Address::generate(&env);
    
    let grant = Grant {
        grantee,
        total_amount: i128::MAX,
        claimed_amount: 0,
        released_amount: 0,
        start_ts: 0,
        cliff_secs: 0,
        duration_secs: u64::MAX,
        revoked: false,
    };
    
    // Choose an elapsed time that would cause elapsed * principal to overflow u128.
    // e.g. elapsed = u64::MAX - 1. principal = i128::MAX. Product is ~3.1e57, which is >> 2^128.
    // Our split quotient-remainder calculation avoids this.
    let elapsed = u64::MAX - 1;
    let vested = grant.vested_at(elapsed);
    assert!(vested <= i128::MAX);
}
