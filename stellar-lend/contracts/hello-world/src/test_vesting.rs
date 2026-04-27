#![cfg(test)]

use crate::vesting::{
    VestingContract, VestingContractClient, VestingDataKey, VestingError, VestingSchedule,
};
use soroban_sdk::{contract, contractimpl, Address, Env};

#[contract]
pub struct TestToken;

#[contractimpl]
impl TestToken {
    pub fn balance(_env: Env, _id: Address) -> i128 {
        1_000_000
    }

    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {
        // Mock implementation for testing
    }
}

fn setup_vesting_test() -> (Env, Address, VestingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let contract_id = env.register(VestingContract, ());
    let client = VestingContractClient::new(&env, &contract_id);

    client.initialize(&admin).unwrap();

    let static_client = unsafe {
        core::mem::transmute::<VestingContractClient<'_>, VestingContractClient<'static>>(client)
    };

    (env, contract_id, static_client, beneficiary)
}

fn advance_time(env: &Env, seconds: u64) {
    let current = env.ledger().timestamp();
    env.ledger().set_timestamp(current + seconds);
}

// --------------------------------------------------------------------------- 
// Cliff boundary tests
// ---------------------------------------------------------------------------

#[test]
fn cliff_exact_boundary_vesting_starts() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    // Create schedule with 100 second cliff, 300 second vesting
    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Check vested amount exactly at cliff time
    advance_time(&mut env, 100);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // At exact cliff time, vesting should just begin
    assert_eq!(vested, 0); // No time has passed in vesting period yet
    assert_eq!(claimable, 0);
    assert!(!fully_vested);
}

#[test]
fn cliff_one_second_after_boundary() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Check vested amount 1 second after cliff
    advance_time(&mut env, 101);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should have minimal vested amount
    assert!(vested > 0);
    assert!(vested < 10); // Should be very small (1000 * 1 / 300)
    assert_eq!(claimable, vested);
    assert!(!fully_vested);
}

#[test]
fn cliff_one_second_before_boundary() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Check vested amount 1 second before cliff
    advance_time(&mut env, 99);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should have no vesting before cliff
    assert_eq!(vested, 0);
    assert_eq!(claimable, 0);
    assert!(!fully_vested);
}

#[test]
fn cliff_zero_duration() {
    let (env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    let result = client.try_create_schedule(
        &beneficiary,
        &1000,
        &0, // Zero cliff
        &300,
        &start_time,
    );
    
    // Zero cliff should be allowed
    assert!(result.is_ok());
}

#[test]
fn cliff_equals_vesting_duration() {
    let (env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    let result = client.try_create_schedule(
        &beneficiary,
        &1000,
        &300, // Cliff equals vesting duration
        &300,
        &start_time,
    );
    
    // Cliff equal to vesting duration should be allowed
    assert!(result.is_ok());
}

#[test]
fn cliff_exceeds_vesting_duration() {
    let (env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    let result = client.try_create_schedule(
        &beneficiary,
        &1000,
        &400, // Cliff exceeds vesting duration
        &300,
        &start_time,
    );
    
    // Cliff exceeding vesting duration should fail
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VestingError::InvalidCliff);
}

// --------------------------------------------------------------------------- 
// Schedule completion tests
// ---------------------------------------------------------------------------

#[test]
fn schedule_exact_completion_time() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Check vested amount exactly at completion time
    advance_time(&mut env, 400); // 100 cliff + 300 vesting
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should be fully vested
    assert_eq!(vested, 1000);
    assert_eq!(claimable, 1000);
    assert!(fully_vested);
}

#[test]
fn schedule_one_second_after_completion() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Check vested amount 1 second after completion
    advance_time(&mut env, 401);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should still be fully vested (no over-vesting)
    assert_eq!(vested, 1000);
    assert_eq!(claimable, 1000);
    assert!(fully_vested);
}

#[test]
fn schedule_one_second_before_completion() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Check vested amount 1 second before completion
    advance_time(&mut env, 399);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should be almost fully vested
    assert!(vested > 990);
    assert!(vested < 1000);
    assert_eq!(claimable, vested);
    assert!(!fully_vested);
}

#[test]
fn schedule_partial_completion_with_claims() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Advance to 50% completion
    advance_time(&mut env, 250); // 100 cliff + 150 vesting (50% of 300)
    
    // Claim half of vested amount
    let claimed = client.claim(&beneficiary, &250).unwrap();
    assert_eq!(claimed, 250);

    // Check schedule after claim
    let schedule = client.get_schedule(&beneficiary).unwrap();
    assert_eq!(schedule.claimed_amount, 250);

    // Advance to completion
    advance_time(&mut env, 150);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    assert_eq!(vested, 1000);
    assert_eq!(claimable, 750); // 1000 - 250 already claimed
    assert!(fully_vested);
}

// --------------------------------------------------------------------------- 
// Zero-release period tests
// ---------------------------------------------------------------------------

#[test]
fn zero_vesting_duration_instant_release() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,  // No cliff
        &0,   // Zero vesting duration - instant release
        &start_time,
    ).unwrap();

    // Should be immediately fully vested
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    assert_eq!(vested, 1000);
    assert_eq!(claimable, 1000);
    assert!(fully_vested);
}

#[test]
fn zero_amount_schedule() {
    let (env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    let result = client.try_create_schedule(
        &beneficiary,
        &0,   // Zero amount
        &100,
        &300,
        &start_time,
    );
    
    // Zero amount should fail
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VestingError::InvalidAmount);
}

#[test]
fn minimal_vesting_duration() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,
        &1,   // Minimal vesting duration (1 second)
        &start_time,
    ).unwrap();

    // Advance 1 second
    advance_time(&mut env, 1);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should be fully vested after 1 second
    assert_eq!(vested, 1000);
    assert_eq!(claimable, 1000);
    assert!(fully_vested);
}

// --------------------------------------------------------------------------- 
// Edge case and leap year tests
// ---------------------------------------------------------------------------

#[test]
fn future_start_time() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let current_time = env.ledger().timestamp();
    let future_start = current_time + 1000;
    
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &future_start,
    ).unwrap();

    // Should have no vesting before start time
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    assert_eq!(vested, 0);
    assert_eq!(claimable, 0);
    assert!(!fully_vested);

    // Advance to future start time
    advance_time(&mut env, 1000);
    let (vested_after_start, claimable_after_start, fully_vested_after_start) = 
        client.calculate_vested(&beneficiary).unwrap();
    
    // Still no vesting at start time (cliff hasn't passed)
    assert_eq!(vested_after_start, 0);
    assert_eq!(claimable_after_start, 0);
    assert!(!fully_vested_after_start);
}

#[test]
fn past_start_time() {
    let (env, _contract_id, client, beneficiary) = setup_vesting_test();

    let current_time = env.ledger().timestamp();
    let past_start = current_time - 1000; // Past start time
    
    let result = client.try_create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &past_start,
    );
    
    // Past start time should fail
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VestingError::InvalidStartTime);
}

#[test]
fn leap_year_handling() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    // Set timestamp to February 28, 2024 (leap year)
    env.ledger().set_timestamp(1709078400); // Feb 28, 2024 00:00:00 UTC
    
    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,
        &86400, // 1 day (covers Feb 29)
        &start_time,
    ).unwrap();

    // Advance through February 29 (leap day)
    advance_time(&mut env, 86400);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should handle leap day correctly
    assert_eq!(vested, 1000);
    assert_eq!(claimable, 1000);
    assert!(fully_vested);
}

#[test]
fn very_long_vesting_period() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,
        &31536000, // 1 year in seconds
        &start_time,
    ).unwrap();

    // Advance 6 months
    advance_time(&mut env, 15768000);
    let (vested, claimable, fully_vested) = client.calculate_vested(&beneficiary).unwrap();
    
    // Should be 50% vested
    assert!(vested > 490 && vested < 510);
    assert_eq!(claimable, vested);
    assert!(!fully_vested);
}

// --------------------------------------------------------------------------- 
// Claim and error handling tests
// ---------------------------------------------------------------------------

#[test]
fn claim_zero_amount() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,
        &300,
        &start_time,
    ).unwrap();

    // Advance to have some vested amount
    advance_time(&mut env, 150);
    
    // Claim with amount=0 should claim maximum available
    let claimed = client.claim(&beneficiary, &0).unwrap();
    assert!(claimed > 0);
}

#[test]
fn claim_more_than_available() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,
        &300,
        &start_time,
    ).unwrap();

    // Advance to have some vested amount
    advance_time(&mut env, 150);
    
    // Try to claim more than available
    let result = client.try_claim(&beneficiary, &1000);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VestingError::OverClaim);
}

#[test]
fn claim_before_cliff() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &100,
        &300,
        &start_time,
    ).unwrap();

    // Try to claim before cliff
    advance_time(&mut env, 50);
    let result = client.try_claim(&beneficiary, &100);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VestingError::NothingToClaim);
}

#[test]
fn claim_from_inactive_schedule() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();
    let admin = Address::generate(&env);

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,
        &300,
        &start_time,
    ).unwrap();

    // Deactivate schedule
    client.deactivate_schedule(&admin, &beneficiary).unwrap();

    // Try to claim from inactive schedule
    let result = client.try_claim(&beneficiary, &100);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), VestingError::InactiveSchedule);
}

#[test]
fn double_claim_same_amount() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &1000,
        &0,
        &300,
        &start_time,
    ).unwrap();

    // Advance to have vested amount
    advance_time(&mut env, 150);
    
    // First claim
    let first_claim = client.claim(&beneficiary, &200).unwrap();
    assert_eq!(first_claim, 200);

    // Second claim should account for previous claim
    let second_claim = client.claim(&beneficiary, &0).unwrap();
    assert!(second_claim > 0);
    assert!(second_claim < 300); // Should be remaining vested amount
}

#[test]
fn schedule_arithmetic_overflow_protection() {
    let (env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    let result = client.try_create_schedule(
        &beneficiary,
        &i128::MAX,
        &100,
        &300,
        &start_time,
    );
    
    // Very large amounts should still work (no overflow in creation)
    assert!(result.is_ok());
}

#[test]
fn exact_boundary_calculations() {
    let (mut env, _contract_id, client, beneficiary) = setup_vesting_test();

    let start_time = env.ledger().timestamp();
    client.create_schedule(
        &beneficiary,
        &100, // Use round number for easy calculation
        &0,
        &100, // 100 seconds for easy math
        &start_time,
    ).unwrap();

    // Test exact percentages
    advance_time(&mut env, 25); // 25%
    let (vested, _, _) = client.calculate_vested(&beneficiary).unwrap();
    assert_eq!(vested, 25);

    advance_time(&mut env, 25); // 50% total
    let (vested, _, _) = client.calculate_vested(&beneficiary).unwrap();
    assert_eq!(vested, 50);

    advance_time(&mut env, 25); // 75% total
    let (vested, _, _) = client.calculate_vested(&beneficiary).unwrap();
    assert_eq!(vested, 75);

    advance_time(&mut env, 25); // 100% total
    let (vested, _, _) = client.calculate_vested(&beneficiary).unwrap();
    assert_eq!(vested, 100);
}
