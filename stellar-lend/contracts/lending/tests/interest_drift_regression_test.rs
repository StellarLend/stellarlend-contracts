use soroban_sdk::{testutils::Address as _, Address, Env};
use stellar_lend_contract::LendingContract;

// Import the live module paths from the lending crate
use stellar_lend_contract::rounding_strategy::{
    compute_compound_interest, 
    RoundingMode,
};
use stellar_lend_contract::interest_model::{
    InterestRateModel,
    AccrualConfig,
};

/// Maximum acceptable drift per year of accrual (1 basis point = 0.01%)
const MAX_DRIFT_BPS: i128 = 1;

/// Test that compound interest does not drift over 365 daily accruals.
#[test]
fn test_daily_accrual_drift_over_one_year() {
    let env = Env::default();
    
    let principal: i128 = 1_000_000_000_000; // 1,000,000.000000
    let annual_rate_bps: u32 = 1000; // 10% APR
    let daily_rate = annual_rate_bps / 365;
    
    let mut accumulated = principal;
    let model = InterestRateModel::default();
    
    // Simulate 365 daily accruals
    for day in 0..365 {
        let interest = compute_compound_interest(
            &env,
            accumulated,
            daily_rate as i128,
            1, // 1 day
            RoundingMode::HalfUp,
        );
        accumulated = accumulated.checked_add(interest)
            .expect("accumulation overflow");
    }
    
    // Expected: principal * (1 + 0.10) = 1,100,000.000000
    let expected = principal * 11000 / 10000;
    let drift = if accumulated > expected {
        accumulated - expected
    } else {
        expected - accumulated
    };
    
    let drift_bps = drift * 10000 / principal;
    
    assert!(
        drift_bps <= MAX_DRIFT_BPS,
        "Interest drift exceeded {} bps over 365 accruals: got {} bps (accumulated={}, expected={})",
        MAX_DRIFT_BPS,
        drift_bps,
        accumulated,
        expected
    );
}

/// Test that hourly accrual converges to same annual result as daily.
#[test]
fn test_hourly_vs_daily_accrual_convergence() {
    let env = Env::default();
    
    let principal: i128 = 1_000_000_000_000;
    let annual_rate_bps: u32 = 1000;
    
    // Daily path: 365 accruals
    let daily_rate = annual_rate_bps / 365;
    let mut daily_accumulated = principal;
    for _ in 0..365 {
        let interest = compute_compound_interest(
            &env,
            daily_accumulated,
            daily_rate as i128,
            1,
            RoundingMode::HalfUp,
        );
        daily_accumulated = daily_accumulated.checked_add(interest).unwrap();
    }
    
    // Hourly path: 365 * 24 = 8760 accruals
    let hourly_rate = annual_rate_bps / (365 * 24);
    let mut hourly_accumulated = principal;
    for _ in 0..(365 * 24) {
        let interest = compute_compound_interest(
            &env,
            hourly_accumulated,
            hourly_rate as i128,
            1,
            RoundingMode::HalfUp,
        );
        hourly_accumulated = hourly_accumulated.checked_add(interest).unwrap();
    }
    
    let diff = if daily_accumulated > hourly_accumulated {
        daily_accumulated - hourly_accumulated
    } else {
        hourly_accumulated - daily_accumulated
    };
    
    let diff_bps = diff * 10000 / principal;
    
    assert!(
        diff_bps <= MAX_DRIFT_BPS,
        "Hourly vs daily accrual divergence exceeded {} bps: got {} bps",
        MAX_DRIFT_BPS,
        diff_bps
    );
}

/// Test edge case: very small principal with high rate.
#[test]
fn test_small_principal_high_rate_drift() {
    let env = Env::default();
    
    let principal: i128 = 1_000; // Very small
    let annual_rate_bps: u32 = 5000; // 50% APR
    let daily_rate = annual_rate_bps / 365;
    
    let mut accumulated = principal;
    
    for _ in 0..365 {
        let interest = compute_compound_interest(
            &env,
            accumulated,
            daily_rate as i128,
            1,
            RoundingMode::HalfUp,
        );
        accumulated = accumulated.checked_add(interest).unwrap_or(accumulated);
    }
    
    // With small principals, rounding dominates; ensure no negative drift
    assert!(accumulated >= principal, "Interest must never reduce principal");
}

/// Test edge case: zero rate produces no drift.
#[test]
fn test_zero_rate_no_drift() {
    let env = Env::default();
    
    let principal: i128 = 1_000_000_000_000;
    let mut accumulated = principal;
    
    for _ in 0..365 {
        let interest = compute_compound_interest(
            &env,
            accumulated,
            0,
            1,
            RoundingMode::HalfUp,
        );
        assert_eq!(interest, 0, "Zero rate must produce zero interest");
        accumulated = accumulated.checked_add(interest).unwrap();
    }
    
    assert_eq!(accumulated, principal, "Zero rate must produce no drift");
}

/// Test that different rounding modes produce bounded divergence.
#[test]
fn test_rounding_mode_divergence_bound() {
    let env = Env::default();
    
    let principal: i128 = 1_000_000_000_000;
    let rate: i128 = 100; // 1% per period
    let periods = 100;
    
    let mut half_up_acc = principal;
    let mut down_acc = principal;
    
    for _ in 0..periods {
        let half_up_interest = compute_compound_interest(
            &env, half_up_acc, rate, 1, RoundingMode::HalfUp,
        );
        let down_interest = compute_compound_interest(
            &env, down_acc, rate, 1, RoundingMode::Down,
        );
        
        half_up_acc = half_up_acc.checked_add(half_up_interest).unwrap();
        down_acc = down_acc.checked_add(down_interest).unwrap();
    }
    
    let diff = if half_up_acc > down_acc {
        half_up_acc - down_acc
    } else {
        down_acc - half_up_acc
    };
    
    // Divergence should be bounded by number of periods * 1 unit
    assert!(
        diff <= periods as i128,
        "Rounding mode divergence too large: {} over {} periods",
        diff,
        periods
    );
}