use stellarlend_lending::rounding_strategy::{
    calculate_interest_with_rounding, RoundingMode, SECONDS_PER_YEAR,
};

const MAX_DRIFT_UNITS: i128 = 365;

fn accrue_repeated_fixed_principal(principal: i128, rate_bps: i128, periods: u64) -> i128 {
    let elapsed = SECONDS_PER_YEAR / periods;
    let mut total = 0i128;

    for _ in 0..periods {
        let interest =
            calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Bankers)
                .expect("interest calculation should not overflow")
                .interest;
        total = total
            .checked_add(interest)
            .expect("total interest overflow");
    }

    total
}

#[test]
fn daily_accrual_stays_close_to_annual_simple_interest() {
    let principal = 1_000_000_000_000i128;
    let annual_rate_bps = 1_000i128;

    let daily_interest = accrue_repeated_fixed_principal(principal, annual_rate_bps, 365);
    let annual_interest = calculate_interest_with_rounding(
        principal,
        SECONDS_PER_YEAR,
        annual_rate_bps,
        RoundingMode::Bankers,
    )
    .expect("annual interest should not overflow")
    .interest;

    let drift = (daily_interest - annual_interest).abs();
    assert!(
        drift <= MAX_DRIFT_UNITS,
        "daily accrual drift exceeded {MAX_DRIFT_UNITS} units: got {drift}"
    );
}

#[test]
fn hourly_and_daily_accrual_paths_remain_close() {
    let principal = 1_000_000_000_000i128;
    let annual_rate_bps = 1_000i128;

    let daily_interest = accrue_repeated_fixed_principal(principal, annual_rate_bps, 365);
    let hourly_interest = accrue_repeated_fixed_principal(principal, annual_rate_bps, 365 * 24);

    let drift = (hourly_interest - daily_interest).abs();
    assert!(
        drift <= 365 * 24,
        "hourly vs daily accrual drift too large: got {drift}"
    );
}

#[test]
fn small_principal_high_rate_never_accrues_negative_interest() {
    let interest = accrue_repeated_fixed_principal(1_000, 5_000, 365);
    assert!(interest >= 0, "interest must never be negative");
}

#[test]
fn zero_rate_has_no_drift() {
    let interest = accrue_repeated_fixed_principal(1_000_000_000_000, 0, 365);
    assert_eq!(interest, 0, "zero rate must produce no interest");
}

#[test]
fn rounding_modes_have_bounded_integer_divergence() {
    let principal = 1_000_000_000_000i128;
    let rate_bps = 100i128;
    let periods = 100u64;
    let elapsed = SECONDS_PER_YEAR / periods;

    let mut floor_total = 0i128;
    let mut bankers_total = 0i128;
    let mut ceil_total = 0i128;

    for _ in 0..periods {
        floor_total +=
            calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Floor)
                .expect("floor calculation should not overflow")
                .interest;
        bankers_total +=
            calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Bankers)
                .expect("bankers calculation should not overflow")
                .interest;
        ceil_total +=
            calculate_interest_with_rounding(principal, elapsed, rate_bps, RoundingMode::Ceil)
                .expect("ceil calculation should not overflow")
                .interest;
    }

    assert!(floor_total <= bankers_total);
    assert!(bankers_total <= ceil_total);
    assert!(
        ceil_total - floor_total <= periods as i128,
        "rounding mode divergence should be bounded by one integer unit per period"
    );
}
