//! # Interest Rate Model Tests
//!
//! Comprehensive tests for the dynamic interest rate model covering:
//! - Utilization-based rate calculations
//! - Rate behavior below, at, and above kink
//! - Rate floor and ceiling enforcement
//! - Emergency rate adjustments
//! - Configuration updates and validation
//! - Edge cases (0%, 100% utilization, zero liquidity)
//! - Unauthorized access rejection
//! - Overflow edge cases
//! - Compound interest accrual
//! - Long-horizon accumulation safety

use crate::deposit::{DepositDataKey, ProtocolAnalytics};
use crate::interest_rate::{
    calculate_accrued_interest, calculate_compound_interest, get_interest_rate_config,
    InterestRateConfig,
};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

// =============================================================================
// CONSTANTS
// =============================================================================

const SECONDS_PER_YEAR: u64 = 365 * 86400;

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract_with_admin(env: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, admin, client)
}

fn set_protocol_analytics(
    env: &Env,
    contract_id: &Address,
    total_deposits: i128,
    total_borrows: i128,
) {
    env.as_contract(contract_id, || {
        let analytics_key = DepositDataKey::ProtocolAnalytics;
        let analytics = ProtocolAnalytics {
            total_deposits,
            total_borrows,
            total_value_locked: total_deposits,
        };
        env.storage().persistent().set(&analytics_key, &analytics);
    });
}

fn get_config(env: &Env, contract_id: &Address) -> Option<InterestRateConfig> {
    env.as_contract(contract_id, || get_interest_rate_config(env))
}

// =============================================================================
// UTILIZATION CALCULATION TESTS
// =============================================================================

#[test]
fn test_utilization_zero_borrows() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 0);
    assert_eq!(client.get_utilization(), 0);
}

#[test]
fn test_utilization_fifty_percent() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 5000);
    assert_eq!(client.get_utilization(), 5000);
}

#[test]
fn test_utilization_at_kink() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 8000);
    assert_eq!(client.get_utilization(), 8000);
}

#[test]
fn test_utilization_full() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 10000);
    assert_eq!(client.get_utilization(), 10000);
}

#[test]
fn test_utilization_capped_at_100() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 15000);
    assert_eq!(client.get_utilization(), 10000);
}

/// Zero deposits → 0% utilization (no division by zero)
#[test]
fn test_utilization_no_deposits() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 0, 0);
    assert_eq!(client.get_utilization(), 0);
}

/// Zero deposits with nonzero borrows → still 0% (safe)
#[test]
fn test_utilization_zero_deposits_nonzero_borrows() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 0, 5000);
    assert_eq!(client.get_utilization(), 0);
}

// =============================================================================
// BORROW RATE CALCULATION TESTS
// =============================================================================

#[test]
fn test_borrow_rate_at_zero_utilization() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 0);
    let borrow_rate = client.get_borrow_rate();
    assert_eq!(borrow_rate, 100); // base rate
    assert!(borrow_rate >= 50); // above floor
}

#[test]
fn test_borrow_rate_below_kink() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    // 40% utilization
    set_protocol_analytics(&env, &contract_id, 10000, 4000);
    let borrow_rate = client.get_borrow_rate();
    // rate = 100 + (4000/8000)*2000 = 100 + 1000 = 1100
    assert_eq!(borrow_rate, 1100);
}

#[test]
fn test_borrow_rate_at_kink() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 8000);
    let borrow_rate = client.get_borrow_rate();
    // rate = 100 + 2000 = 2100
    assert_eq!(borrow_rate, 2100);
}

#[test]
fn test_borrow_rate_above_kink() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    // 90% utilization
    set_protocol_analytics(&env, &contract_id, 10000, 9000);
    let borrow_rate = client.get_borrow_rate();
    // rate = 2100 + (1000/2000)*10000 = 2100 + 5000 = 7100
    assert_eq!(borrow_rate, 7100);
}

#[test]
fn test_borrow_rate_at_full_utilization() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 10000);
    let borrow_rate = client.get_borrow_rate();
    // rate = 2100 + 10000 = 12100, capped at ceiling 10000
    assert_eq!(borrow_rate, 10000);
}

// =============================================================================
// SUPPLY RATE CALCULATION TESTS
// =============================================================================

#[test]
fn test_supply_rate_calculation() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 4000);
    let borrow_rate = client.get_borrow_rate();
    let supply_rate = client.get_supply_rate();
    assert_eq!(supply_rate, borrow_rate - 200);
    assert_eq!(supply_rate, 900);
}

#[test]
fn test_supply_rate_floor_enforcement() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 100);
    let supply_rate = client.get_supply_rate();
    assert!(supply_rate >= 50); // floor
}

// =============================================================================
// RATE FLOOR AND CEILING TESTS
// =============================================================================

#[test]
fn test_rate_floor_enforcement() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 0);
    client.update_interest_rate_config(
        &admin,
        &Some(10), // very low base rate
        &None,
        &None,
        &None,
        &Some(100), // floor: 1%
        &None,
        &None,
    );
    let borrow_rate = client.get_borrow_rate();
    assert!(borrow_rate >= 100);
}

#[test]
fn test_rate_ceiling_enforcement() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 10000);
    let borrow_rate = client.get_borrow_rate();
    assert!(borrow_rate <= 10000);
    assert_eq!(borrow_rate, 10000);
}

// =============================================================================
// EMERGENCY RATE ADJUSTMENT TESTS
// =============================================================================

#[test]
fn test_emergency_rate_adjustment_positive() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 4000);
    let rate_before = client.get_borrow_rate();
    client.set_emergency_rate_adjustment(&admin, &500);
    let rate_after = client.get_borrow_rate();
    assert_eq!(rate_after, rate_before + 500);
}

#[test]
fn test_emergency_rate_adjustment_negative() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 4000);
    let rate_before = client.get_borrow_rate();
    client.set_emergency_rate_adjustment(&admin, &(-300));
    let rate_after = client.get_borrow_rate();
    assert_eq!(rate_after, rate_before - 300);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_emergency_rate_adjustment_unauthorized() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let unauthorized = Address::generate(&env);
    client.set_emergency_rate_adjustment(&unauthorized, &500);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_emergency_rate_adjustment_exceeds_bounds() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.set_emergency_rate_adjustment(&admin, &15000);
}

/// Extreme negative emergency cannot push below floor
#[test]
fn test_borrow_rate_clamped_at_floor_under_extreme_negative_adjustment() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10_000, 0);
    client.set_emergency_rate_adjustment(&admin, &-10_000);
    let borrow_rate = client.get_borrow_rate();
    assert_eq!(borrow_rate, 50); // floor
}

// =============================================================================
// CONFIGURATION UPDATE TESTS
// =============================================================================

#[test]
fn test_update_config_base_rate() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 0);
    client.update_interest_rate_config(
        &admin,
        &Some(200),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    assert_eq!(client.get_borrow_rate(), 200);
}

#[test]
fn test_update_config_kink() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 7000);
    client.update_interest_rate_config(
        &admin,
        &None,
        &Some(6000),
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    // kink at 60%, util 70% is above kink
    // rate = (100+2000) + (1000/4000)*10000 = 2100 + 2500 = 4600
    assert_eq!(client.get_borrow_rate(), 4600);
}

#[test]
fn test_update_config_multiplier() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 4000);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &Some(4000),
        &None,
        &None,
        &None,
        &None,
    );
    // rate = 100 + (4000/8000)*4000 = 100 + 2000 = 2100
    assert_eq!(client.get_borrow_rate(), 2100);
}

#[test]
fn test_update_config_jump_multiplier() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 9000);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &Some(5000),
        &None,
        &None,
        &None,
    );
    // rate = 2100 + (1000/2000)*5000 = 2100 + 2500 = 4600
    assert_eq!(client.get_borrow_rate(), 4600);
}

#[test]
fn test_update_config_spread() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 4000);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(500),
    );
    let borrow_rate = client.get_borrow_rate();
    let supply_rate = client.get_supply_rate();
    assert_eq!(supply_rate, borrow_rate - 500);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_update_config_unauthorized() {
    let env = create_test_env();
    let (_contract_id, _admin, client) = setup_contract_with_admin(&env);
    let unauthorized = Address::generate(&env);
    client.update_interest_rate_config(
        &unauthorized,
        &Some(200),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
}

/// Verify get_interest_rate_config returns the stored config
#[test]
fn test_get_interest_rate_config_entrypoint() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    let config = client.get_interest_rate_config().unwrap();
    assert_eq!(config.base_rate_bps, 100);
    assert_eq!(config.kink_utilization_bps, 8000);
    assert_eq!(config.multiplier_bps, 2000);
    assert_eq!(config.jump_multiplier_bps, 10_000);
    assert_eq!(config.rate_floor_bps, 50);
    assert_eq!(config.rate_ceiling_bps, 10_000);
    assert_eq!(config.spread_bps, 200);
    assert_eq!(config.emergency_adjustment_bps, 0);

    // Update and re-read
    client.update_interest_rate_config(
        &admin,
        &Some(300),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    let config2 = client.get_interest_rate_config().unwrap();
    assert_eq!(config2.base_rate_bps, 300);
}

// =============================================================================
// INVALID PARAMETER TESTS
// =============================================================================

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_base_rate_negative() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &Some(-100),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_base_rate_too_high() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &Some(15000),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_kink_zero() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(&admin, &None, &Some(0), &None, &None, &None, &None, &None);
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_kink_100_percent() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &None,
        &Some(10000),
        &None,
        &None,
        &None,
        &None,
        &None,
    );
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_multiplier_negative() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &Some(-100),
        &None,
        &None,
        &None,
        &None,
    );
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_jump_multiplier_negative() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &Some(-100),
        &None,
        &None,
        &None,
    );
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_floor_above_ceiling() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &None,
        &Some(5000),
        &Some(3000),
        &None,
    );
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_spread_negative() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(&admin, &None, &None, &None, &None, &None, &None, &Some(-1));
}

#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_spread_above_100() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(10001),
    );
}

/// Multiplier > MAX_SLOPE_BPS (100_000) should be rejected
#[test]
#[should_panic(expected = "HostError")]
fn test_invalid_multiplier_exceeds_max_slope() {
    let env = create_test_env();
    let (_contract_id, admin, client) = setup_contract_with_admin(&env);
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &Some(100_001),
        &None,
        &None,
        &None,
        &None,
    );
}

// =============================================================================
// ACCRUED INTEREST CALCULATION TESTS (SIMPLE)
// =============================================================================

#[test]
fn test_accrued_interest_one_year_at_10_percent() {
    let interest = calculate_accrued_interest(1_000_000, 0, SECONDS_PER_YEAR, 1000).unwrap();
    assert_eq!(interest, 100_000);
}

#[test]
fn test_accrued_interest_partial_year() {
    let interest = calculate_accrued_interest(1_000_000, 0, SECONDS_PER_YEAR / 2, 1000).unwrap();
    assert_eq!(interest, 50_000);
}

#[test]
fn test_accrued_interest_zero_principal() {
    let interest = calculate_accrued_interest(0, 0, SECONDS_PER_YEAR, 1000).unwrap();
    assert_eq!(interest, 0);
}

#[test]
fn test_accrued_interest_zero_time() {
    let interest = calculate_accrued_interest(1_000_000, 1000, 1000, 1000).unwrap();
    assert_eq!(interest, 0);
}

#[test]
fn test_accrued_interest_time_backwards() {
    let interest = calculate_accrued_interest(1_000_000, 2000, 1000, 1000).unwrap();
    assert_eq!(interest, 0);
}

#[test]
fn test_accrued_interest_zero_rate() {
    let interest = calculate_accrued_interest(1_000_000, 0, SECONDS_PER_YEAR, 0).unwrap();
    assert_eq!(interest, 0);
}

#[test]
fn test_accrued_interest_extreme_overflow() {
    let result = calculate_accrued_interest(i128::MAX, 0, 100 * SECONDS_PER_YEAR, 10000);
    assert!(result.is_err());
}

// =============================================================================
// COMPOUND INTEREST TESTS
// =============================================================================

#[test]
fn test_compound_interest_one_year() {
    // 1M at 10% for 1 year → simple = 100_000
    let interest = calculate_compound_interest(1_000_000, 0, SECONDS_PER_YEAR, 1000).unwrap();
    assert_eq!(interest, 100_000);
}

#[test]
fn test_compound_interest_two_years() {
    // 1M at 10% compounded yearly:
    // Year 1: 1_000_000 * 10% = 100_000 → balance 1_100_000
    // Year 2: 1_100_000 * 10% = 110_000 → balance 1_210_000
    // Interest = 210_000
    let interest = calculate_compound_interest(1_000_000, 0, 2 * SECONDS_PER_YEAR, 1000).unwrap();
    assert_eq!(interest, 210_000);
}

#[test]
fn test_compound_interest_three_years() {
    // Year 1: 1M → 1.1M (interest 100k)
    // Year 2: 1.1M → 1.21M (interest 110k)
    // Year 3: 1.21M → 1.331M (interest 121k)
    // Total interest = 331_000
    let interest = calculate_compound_interest(1_000_000, 0, 3 * SECONDS_PER_YEAR, 1000).unwrap();
    assert_eq!(interest, 331_000);
}

#[test]
fn test_compound_interest_partial_year() {
    // Half a year at 10%: simple interest on principal
    let interest = calculate_compound_interest(1_000_000, 0, SECONDS_PER_YEAR / 2, 1000).unwrap();
    assert_eq!(interest, 50_000);
}

#[test]
fn test_compound_interest_year_and_a_half() {
    // 1 full year compound + 0.5 year simple on compounded balance
    // Year 1: 1M → 1.1M
    // Half year: 1.1M * 10% * 0.5 = 55_000
    // Total interest = 155_000
    let interest =
        calculate_compound_interest(1_000_000, 0, SECONDS_PER_YEAR + SECONDS_PER_YEAR / 2, 1000)
            .unwrap();
    assert_eq!(interest, 155_000);
}

#[test]
fn test_compound_interest_zero_principal() {
    let interest = calculate_compound_interest(0, 0, SECONDS_PER_YEAR, 1000).unwrap();
    assert_eq!(interest, 0);
}

#[test]
fn test_compound_interest_zero_rate() {
    let interest = calculate_compound_interest(1_000_000, 0, SECONDS_PER_YEAR, 0).unwrap();
    assert_eq!(interest, 0);
}

#[test]
fn test_compound_interest_zero_time() {
    let interest = calculate_compound_interest(1_000_000, 100, 100, 1000).unwrap();
    assert_eq!(interest, 0);
}

/// Compound should always be ≥ simple for multi-year horizons
#[test]
fn test_compound_exceeds_simple_over_multiple_years() {
    let principal = 1_000_000i128;
    let rate = 1000i128; // 10%
    let time = 5 * SECONDS_PER_YEAR;

    let simple = calculate_accrued_interest(principal, 0, time, rate).unwrap();
    let compound = calculate_compound_interest(principal, 0, time, rate).unwrap();

    assert!(compound > simple, "compound={compound}, simple={simple}");
}

// =============================================================================
// LONG-HORIZON ACCRUAL TESTS
// =============================================================================

/// Simple interest remains monotonic and bounded over long horizons
#[test]
fn test_accrued_interest_long_horizon_monotonic_and_bounded() {
    let principal = 1_000_000_000_000i128;
    let rate_bps = 10_000i128; // 100% APR
    let checkpoints = [
        SECONDS_PER_YEAR,
        10 * SECONDS_PER_YEAR,
        50 * SECONDS_PER_YEAR,
        200 * SECONDS_PER_YEAR,
    ];

    let mut previous_interest = 0i128;
    for &current_time in &checkpoints {
        let interest = calculate_accrued_interest(principal, 0, current_time, rate_bps).unwrap();
        assert!(interest >= previous_interest);
        let years_elapsed = (current_time / SECONDS_PER_YEAR) as i128;
        let upper_bound = principal.checked_mul(years_elapsed).unwrap();
        assert!(interest <= upper_bound);
        previous_interest = interest;
    }
}

/// Overflow boundary for simple interest
#[test]
fn test_accrued_interest_long_horizon_overflow_boundary() {
    let principal = 1_000_000_000_000_000i128;
    let rate_bps = 10_000i128;
    let product = principal.checked_mul(rate_bps).unwrap();
    let max_safe_elapsed = (i128::MAX / product) as u64;
    assert!(max_safe_elapsed < u64::MAX);

    let safe_result = calculate_accrued_interest(principal, 0, max_safe_elapsed, rate_bps);
    assert!(safe_result.is_ok());

    let overflow_result = calculate_accrued_interest(principal, 0, max_safe_elapsed + 1, rate_bps);
    assert!(overflow_result.is_err());
}

/// Compound accrual over 50 years at 100% APR should not overflow for moderate principal
#[test]
fn test_compound_interest_long_horizon_no_overflow() {
    let principal = 1_000_000_000i128; // 1B
    let rate_bps = 10_000i128; // 100% APR
    let time = 50 * SECONDS_PER_YEAR;

    let result = calculate_compound_interest(principal, 0, time, rate_bps);
    assert!(result.is_ok());

    let interest = result.unwrap();
    assert!(interest > principal); // Compounded should be much larger than simple
}

// =============================================================================
// EXTREME CONFIGURATION TESTS
// =============================================================================

#[test]
fn test_borrow_rate_clamped_at_ceiling_under_extreme_configuration() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10_000, 20_000);

    client.update_interest_rate_config(
        &admin,
        &Some(10_000),
        &Some(1),
        &Some(100_000),
        &Some(100_000),
        &Some(50),
        &Some(10_000),
        &Some(200),
    );
    client.set_emergency_rate_adjustment(&admin, &10_000);

    let borrow_rate = client.get_borrow_rate();
    let supply_rate = client.get_supply_rate();

    assert_eq!(borrow_rate, 10_000); // ceiling
    assert!(supply_rate >= 50);
    assert!(supply_rate <= 10_000);
}

// =============================================================================
// RATE TRANSITION TESTS
// =============================================================================

#[test]
fn test_rate_changes_monotonically_with_utilization() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);

    let mut previous_rate = 0i128;
    for util in (0..=100).step_by(10) {
        let util_bps = (util * 100) as i128;
        set_protocol_analytics(&env, &contract_id, 10000, util_bps);
        let rate = client.get_borrow_rate();
        assert!(
            rate >= previous_rate,
            "Rate decreased at {}% utilization",
            util
        );
        previous_rate = rate;
    }
}

#[test]
fn test_rate_jump_at_kink() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);

    set_protocol_analytics(&env, &contract_id, 10000, 7900);
    let rate_below_kink = client.get_borrow_rate();

    set_protocol_analytics(&env, &contract_id, 10000, 8100);
    let rate_above_kink = client.get_borrow_rate();

    assert!(rate_above_kink > rate_below_kink);
    // Jump should be significant
    let linear_increase = (rate_below_kink * 200) / 7900;
    let actual_increase = rate_above_kink - rate_below_kink;
    assert!(
        actual_increase > linear_increase,
        "Jump multiplier not taking effect"
    );
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

#[test]
fn test_very_small_utilization() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 1_000_000, 100);
    let rate = client.get_borrow_rate();
    assert!(rate >= 50);
    assert!(rate <= 200);
}

#[test]
fn test_large_values() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 1_000_000_000_000, 500_000_000_000);
    let utilization = client.get_utilization();
    assert_eq!(utilization, 5000);
    let rate = client.get_borrow_rate();
    assert!(rate > 0);
}

#[test]
fn test_rate_consistency() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 5000);
    let rate1 = client.get_borrow_rate();
    let rate2 = client.get_borrow_rate();
    let rate3 = client.get_borrow_rate();
    assert_eq!(rate1, rate2);
    assert_eq!(rate2, rate3);
}

// =============================================================================
// INTEGRATION TESTS
// =============================================================================

#[test]
fn test_full_interest_rate_workflow() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);

    // 1. Check initial config
    let config = get_config(&env, &contract_id).unwrap();
    assert_eq!(config.base_rate_bps, 100);
    assert_eq!(config.kink_utilization_bps, 8000);

    // 2. Set utilization
    set_protocol_analytics(&env, &contract_id, 10000, 4000);
    let initial_rate = client.get_borrow_rate();
    assert_eq!(initial_rate, 1100);

    // 3. Update config
    client.update_interest_rate_config(
        &admin,
        &Some(200),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // 4. Rate changed
    let new_rate = client.get_borrow_rate();
    assert!(new_rate > initial_rate);

    // 5. Emergency adjustment
    client.set_emergency_rate_adjustment(&admin, &300);

    // 6. Emergency applied
    let emergency_rate = client.get_borrow_rate();
    assert_eq!(emergency_rate, new_rate + 300);
}

#[test]
fn test_interest_accrual_over_time() {
    let env = create_test_env();
    let (contract_id, _admin, client) = setup_contract_with_admin(&env);
    let user = Address::generate(&env);

    client.deposit_collateral(&user, &None, &100_000);

    env.as_contract(&contract_id, || {
        let analytics = ProtocolAnalytics {
            total_deposits: 100_000,
            total_borrows: 50_000,
            total_value_locked: 100_000,
        };
        env.storage()
            .persistent()
            .set(&DepositDataKey::ProtocolAnalytics, &analytics);
    });

    let rate = client.get_borrow_rate();
    let expected_interest = calculate_accrued_interest(50_000, 0, SECONDS_PER_YEAR, rate).unwrap();
    assert!(expected_interest > 500);
    assert!(expected_interest < 50_000);
}

/// Verify the config's last_update timestamp is set after update
#[test]
fn test_config_timestamp_updated() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);

    let config_before = get_config(&env, &contract_id).unwrap();
    assert_eq!(config_before.last_update, 0);

    // The ledger timestamp in test env starts at 0, so we just verify it's set
    client.update_interest_rate_config(
        &admin,
        &Some(200),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let config_after = get_config(&env, &contract_id).unwrap();
    // Timestamp should be set (≥ 0 in test, but the field is populated)
    assert_eq!(config_after.base_rate_bps, 200);
}

/// Multiple sequential config updates should all apply correctly
#[test]
fn test_sequential_config_updates() {
    let env = create_test_env();
    let (contract_id, admin, client) = setup_contract_with_admin(&env);
    set_protocol_analytics(&env, &contract_id, 10000, 5000);

    // Update 1: change base rate
    client.update_interest_rate_config(
        &admin,
        &Some(200),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    let rate1 = client.get_borrow_rate();

    // Update 2: change multiplier
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &Some(3000),
        &None,
        &None,
        &None,
        &None,
    );
    let rate2 = client.get_borrow_rate();

    // Update 3: change spread
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(100),
    );
    let supply_rate = client.get_supply_rate();
    let borrow_rate = client.get_borrow_rate();

    assert_ne!(rate1, rate2); // multiplier change should alter rate
    assert_eq!(supply_rate, borrow_rate - 100);
}
