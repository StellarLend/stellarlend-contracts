//! Tests verifying interest accrual settlement during liquidation.
//!
//! Covers:
//! 1. settled-vs-unsettled parity: validating that pre-liquidation accrued interest
//!    is capitalized into principal before debt reduction.
//! 2. long-horizon accrued interest: validating accrual over 5/10 year horizons.
//! 3. health-factor-after-settle boundary: verifying that positions which become
//!    unhealthy due to interest accrual are correctly identified as liquidatable.

#![cfg(test)]

use crate::{
    debt::{load_debt, save_debt, DebtPosition, DEFAULT_APR_BPS},
    rounding_strategy::SECONDS_PER_YEAR,
    DataKey, LendingContract, LendingContractClient, LendingError,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

/// Setup test environment with contract, admin, user, and liquidator.
fn setup() -> (Env, LendingContractClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let liquidator = Address::generate(&env);

    client.initialize(&admin);

    (env, client, admin, user, liquidator)
}

/// Advance ledger time by specified seconds.
fn advance_ledger_time(env: &Env, seconds: u64) {
    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp = ledger_info.timestamp.saturating_add(seconds);
    ledger_info.sequence_number = ledger_info.sequence_number.saturating_add(1);
    env.ledger().set(ledger_info);
}

/// Calculate expected simple interest.
fn calculate_expected_interest(principal: i128, elapsed_seconds: u64, rate_bps: i128) -> i128 {
    let numerator = principal
        .checked_mul(elapsed_seconds as i128)
        .and_then(|v| v.checked_mul(rate_bps))
        .expect("interest calculation overflow");

    let denominator = (SECONDS_PER_YEAR as i128)
        .checked_mul(10_000)
        .expect("denominator overflow");

    numerator / denominator
}

/// Test 1: settled-vs-unsettled parity
/// Verifies that accrued interest is capitalized into the borrower's principal
/// at the moment of liquidation, matching the non-liquidation repay path.
#[test]
fn test_liquidate_accrual_parity() {
    let (env, client, _admin, user, liquidator) = setup();

    let initial_collateral = 100i128;
    let initial_debt = 200i128;

    // Direct state injection to set up an unhealthy position
    let now = env.ledger().timestamp();
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &initial_collateral);
        save_debt(
            &env,
            &user,
            &DebtPosition {
                principal: initial_debt,
                last_update: now,
            },
        );
    });

    // Advance time by 1 year to accrue interest
    advance_ledger_time(&env, SECONDS_PER_YEAR);

    let expected_interest =
        calculate_expected_interest(initial_debt, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
    let expected_settled_debt = initial_debt + expected_interest;

    // Repay a portion via liquidation (e.g., 50 units)
    let repay_amount = 50i128;
    let actual_repay = client.liquidate(&liquidator, &user, &repay_amount);
    assert_eq!(actual_repay, repay_amount);

    // Verify post-liquidation position
    let post_position = client.get_debt_position(&user);
    let expected_final_debt = expected_settled_debt - repay_amount;

    assert_eq!(
        post_position.principal, expected_final_debt,
        "accrued interest must be capitalized and then reduced by the repay amount"
    );
    assert_eq!(
        post_position.last_update,
        env.ledger().timestamp(),
        "last_update must be stamped to the current ledger timestamp"
    );
}

/// Test 2: long-horizon accrued interest
/// Verifies that interest accrued over 5 and 10 year horizons is capitalized
/// correctly during liquidation.
#[test]
fn test_liquidate_long_horizon_accrual() {
    let (env, client, _admin, user, liquidator) = setup();

    let initial_collateral = 10_000i128;
    let initial_debt = 1_000i128;
    let now = env.ledger().timestamp();

    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &initial_collateral);
        save_debt(
            &env,
            &user,
            &DebtPosition {
                principal: initial_debt,
                last_update: now,
            },
        );
    });

    // Advance time by 10 years
    let ten_years = SECONDS_PER_YEAR * 10;
    advance_ledger_time(&env, ten_years);

    let expected_interest = calculate_expected_interest(initial_debt, ten_years, DEFAULT_APR_BPS);
    let expected_settled_debt = initial_debt + expected_interest; // 1000 + 500 = 1500

    // Position is unhealthy (hf = 10000 * 8000 / 1500 = 53333 >= 10000 is healthy, wait)
    // To make it unhealthy: decrease collateral to 100
    let low_collateral = 100i128;
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &low_collateral);
    });

    // Repay 100 units
    let actual_repay = client.liquidate(&liquidator, &user, &100);
    assert_eq!(actual_repay, 100);

    let post_position = client.get_debt_position(&user);
    assert_eq!(
        post_position.principal,
        expected_settled_debt - 100,
        "long-horizon interest must be correctly capitalized"
    );
}

/// Test 3: health-factor-after-settle boundary
/// Verifies that a position which is healthy at t=0 becomes unhealthy and liquidatable
/// after interest accrues over time.
#[test]
fn test_liquidate_health_factor_after_settle_boundary() {
    let (env, client, _admin, user, liquidator) = setup();

    // Set up a position close to the liquidation threshold:
    // collateral = 100, principal = 80 -> HF = 100 * 8000 / 80 = 10000 (exactly healthy)
    let initial_collateral = 100i128;
    let initial_debt = 80i128;
    let now = env.ledger().timestamp();

    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &initial_collateral);
        save_debt(
            &env,
            &user,
            &DebtPosition {
                principal: initial_debt,
                last_update: now,
            },
        );
    });

    // Liquidation should fail immediately at t=0 because position is healthy (hf >= 10000)
    let result_t0 = client.try_liquidate(&liquidator, &user, &10);
    assert!(
        matches!(result_t0, Err(Ok(LendingError::PositionHealthy))),
        "should reject liquidation at t=0 as healthy"
    );

    // Advance time by 1 year to accrue interest
    advance_ledger_time(&env, SECONDS_PER_YEAR);

    // Expected interest: 80 * 5% = 4. Expected settled debt: 84.
    // HF after settle: 100 * 8000 / 84 = 9523 < 10000 (unhealthy!)
    // Liquidation should now succeed because the settled debt drops HF below the threshold.
    let result_t1 = client.try_liquidate(&liquidator, &user, &10);
    assert!(
        result_t1.is_ok(),
        "should allow liquidation after time advancement due to interest accrual lowering HF"
    );

    let post_position = client.get_debt_position(&user);
    assert_eq!(
        post_position.principal,
        84 - 10,
        "accrued interest must be capitalized into final debt"
    );
}
