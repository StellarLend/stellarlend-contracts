// ════════════════════════════════════════════════════════════════
// LEDGER-TIME ADVANCEMENT TESTS: Interest Accrual Ordering on Repay
// ════════════════════════════════════════════════════════════════
//
// Purpose: Verify that interest is accrued BEFORE the repay amount is
// subtracted, ensuring correct debt calculation across time boundaries.
//
// Issue: #832
// Branch: testing/interest-ordering-time
//
// Security Invariant:
// The order of operations on repay MUST be:
//   1. Accrue interest based on elapsed time
//   2. Apply repayment to the accrued total
//
// If the order were reversed (apply-then-accrue), users could repay
// before interest accrues, effectively getting interest-free loans.
//
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod interest_ordering_time_tests {
    use crate::debt::{
        borrow_amount, load_debt, repay_amount, save_debt, DebtPosition, DEFAULT_APR_BPS,
    };
    use crate::rounding_strategy::SECONDS_PER_YEAR;
    use crate::{LendingContract, LendingContractClient};
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::{Address, Env};

    // ════════════════════════════════════════════════════════════════
    // Test Helpers
    // ════════════════════════════════════════════════════════════════

    /// Setup test environment with contract and users
    fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        client.initialize(&admin);

        (env, client, admin, user)
    }

    /// Advance ledger time by specified seconds
    fn advance_ledger_time(env: &Env, seconds: u64) {
        let mut ledger_info = env.ledger().get();
        ledger_info.timestamp = ledger_info.timestamp.saturating_add(seconds);
        ledger_info.sequence_number = ledger_info.sequence_number.saturating_add(1);
        env.ledger().set(ledger_info);
    }

    /// Calculate expected interest for a given principal, time, and rate
    fn calculate_expected_interest(principal: i128, elapsed_seconds: u64, rate_bps: i128) -> i128 {
        // Formula: interest = principal * elapsed_seconds * rate_bps / (SECONDS_PER_YEAR * 10_000)
        let numerator = principal
            .checked_mul(elapsed_seconds as i128)
            .and_then(|v| v.checked_mul(rate_bps))
            .expect("interest calculation overflow");

        let denominator = (SECONDS_PER_YEAR as i128)
            .checked_mul(10_000)
            .expect("denominator overflow");

        numerator / denominator
    }

    // ════════════════════════════════════════════════════════════════
    // Core Ordering Tests
    // ════════════════════════════════════════════════════════════════

    /// Test 1: Verify interest accrues BEFORE repay is applied (zero elapsed time)
    ///
    /// Boundary case: Repay immediately after borrow (same timestamp)
    /// Expected: No interest accrued, repay reduces principal exactly
    #[test]
    fn test_repay_immediately_zero_elapsed_time() {
        let (env, client, _admin, user) = setup();

        // Borrow 1000 units
        let borrow_amount = 1000i128;
        client.borrow(&user, &borrow_amount).unwrap();

        // Repay 300 units immediately (same timestamp)
        let repay_amount = 300i128;
        let remaining = client.repay(&user, &repay_amount);

        // Expected: 1000 - 300 = 700 (no interest accrued)
        assert_eq!(
            remaining, 700,
            "Immediate repay should have zero interest"
        );

        // Verify position
        let position = client.get_debt_position(&user);
        assert_eq!(position.principal, 700);
    }

    /// Test 2: Verify interest accrues BEFORE repay after exactly one year
    ///
    /// This is the canonical test for accrue-then-apply ordering.
    /// If the order were wrong, the user would repay against the old principal.
    #[test]
    fn test_repay_after_one_year_accrues_first() {
        let (env, client, _admin, user) = setup();

        // Borrow 10,000 units
        let borrow_amount = 10_000i128;
        client.borrow(&user, &borrow_amount).unwrap();

        // Advance time by exactly one year
        advance_ledger_time(&env, SECONDS_PER_YEAR);

        // Calculate expected interest: 10,000 * 5% = 500
        let expected_interest =
            calculate_expected_interest(borrow_amount, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(expected_interest, 500, "Expected 5% annual interest");

        // Expected debt before repay: 10,000 + 500 = 10,500
        let expected_debt_before_repay = borrow_amount + expected_interest;

        // Repay 1,000 units
        let repay_amount = 1_000i128;
        let remaining = client.repay(&user, &repay_amount);

        // Expected remaining: 10,500 - 1,000 = 9,500
        let expected_remaining = expected_debt_before_repay - repay_amount;
        assert_eq!(
            remaining, expected_remaining,
            "Repay must apply to accrued debt (principal + interest)"
        );

        // Verify the debt position reflects accrued interest
        let position = client.get_debt_position(&user);
        assert_eq!(position.principal, expected_remaining);
    }

    /// Test 3: Repay amount smaller than accrued interest
    ///
    /// Edge case: User repays less than the interest owed.
    /// The repay should still reduce the total debt (principal + interest).
    #[test]
    fn test_repay_smaller_than_accrued_interest() {
        let (env, client, _admin, user) = setup();

        // Borrow 100,000 units
        let borrow_amount = 100_000i128;
        client.borrow(&user, &borrow_amount).unwrap();

        // Advance time by one year
        advance_ledger_time(&env, SECONDS_PER_YEAR);

        // Expected interest: 100,000 * 5% = 5,000
        let expected_interest =
            calculate_expected_interest(borrow_amount, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(expected_interest, 5_000);

        // Expected debt: 100,000 + 5,000 = 105,000
        let expected_debt = borrow_amount + expected_interest;

        // Repay only 2,000 (less than interest)
        let repay_amount = 2_000i128;
        let remaining = client.repay(&user, &repay_amount);

        // Expected remaining: 105,000 - 2,000 = 103,000
        let expected_remaining = expected_debt - repay_amount;
        assert_eq!(
            remaining, expected_remaining,
            "Partial repay must reduce accrued debt"
        );
    }

    /// Test 4: Multiple borrows and repays with time advancement
    ///
    /// Scenario: Borrow → wait → borrow more → wait → repay
    /// Verifies that interest compounds correctly across multiple operations.
    #[test]
    fn test_multiple_borrows_and_repays_with_time() {
        let (env, client, _admin, user) = setup();

        // Initial borrow: 10,000
        client.borrow(&user, &10_000).unwrap();

        // Wait 6 months
        let six_months = SECONDS_PER_YEAR / 2;
        advance_ledger_time(&env, six_months);

        // Interest after 6 months: 10,000 * 2.5% = 250
        let interest_6m = calculate_expected_interest(10_000, six_months, DEFAULT_APR_BPS);
        assert_eq!(interest_6m, 250);

        // Borrow another 5,000 (this should accrue interest first)
        client.borrow(&user, &5_000).unwrap();

        // After second borrow, debt = 10,000 + 250 + 5,000 = 15,250
        let position_after_second_borrow = client.get_debt_position(&user);
        assert_eq!(position_after_second_borrow.principal, 15_250);

        // Wait another 6 months
        advance_ledger_time(&env, six_months);

        // Interest on 15,250 for 6 months: 15,250 * 2.5% = 381 (rounded)
        let interest_second_6m = calculate_expected_interest(15_250, six_months, DEFAULT_APR_BPS);
        assert_eq!(interest_second_6m, 381);

        // Total debt before repay: 15,250 + 381 = 15,631
        let expected_debt_before_repay = 15_250 + interest_second_6m;

        // Repay 5,000
        let remaining = client.repay(&user, &5_000);

        // Expected remaining: 15,631 - 5,000 = 10,631
        assert_eq!(remaining, expected_debt_before_repay - 5_000);
    }

    // ════════════════════════════════════════════════════════════════
    // Boundary and Edge Cases
    // ════════════════════════════════════════════════════════════════

    /// Test 5: Repay exact debt (principal + interest)
    ///
    /// User repays the exact amount owed including interest.
    /// Debt should be zero after repay.
    #[test]
    fn test_repay_exact_debt_including_interest() {
        let (env, client, _admin, user) = setup();

        // Borrow 1,000
        client.borrow(&user, &1_000).unwrap();

        // Wait one year
        advance_ledger_time(&env, SECONDS_PER_YEAR);

        // Interest: 1,000 * 5% = 50
        let interest = calculate_expected_interest(1_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(interest, 50);

        // Total debt: 1,000 + 50 = 1,050
        let total_debt = 1_000 + interest;

        // Repay exact amount
        let remaining = client.repay(&user, &total_debt);

        // Should be zero
        assert_eq!(remaining, 0, "Exact repay should zero out debt");
    }

    /// Test 6: Very short time period (1 second)
    ///
    /// Verifies that even tiny time periods accrue interest correctly.
    #[test]
    fn test_repay_after_one_second() {
        let (env, client, _admin, user) = setup();

        // Borrow large amount to make 1-second interest visible
        let borrow_amount = 100_000_000i128; // 100 million
        client.borrow(&user, &borrow_amount).unwrap();

        // Advance by 1 second
        advance_ledger_time(&env, 1);

        // Interest for 1 second: 100,000,000 * 500 / (31,536,000 * 10,000)
        // = 50,000,000,000 / 315,360,000,000 = 0.158... ≈ 0 (rounds down)
        let interest = calculate_expected_interest(borrow_amount, 1, DEFAULT_APR_BPS);

        // Repay 1,000
        let remaining = client.repay(&user, &1_000);

        // Expected: 100,000,000 + interest - 1,000
        let expected = borrow_amount + interest - 1_000;
        assert_eq!(remaining, expected);
    }

    /// Test 7: Very long time period (10 years)
    ///
    /// Verifies interest accrual over extended periods.
    #[test]
    fn test_repay_after_ten_years() {
        let (env, client, _admin, user) = setup();

        // Borrow 10,000
        let borrow_amount = 10_000i128;
        client.borrow(&user, &borrow_amount).unwrap();

        // Advance by 10 years
        let ten_years = SECONDS_PER_YEAR * 10;
        advance_ledger_time(&env, ten_years);

        // Interest: 10,000 * 5% * 10 = 5,000 (simple interest)
        let interest = calculate_expected_interest(borrow_amount, ten_years, DEFAULT_APR_BPS);
        assert_eq!(interest, 5_000);

        // Total debt: 10,000 + 5,000 = 15,000
        let total_debt = borrow_amount + interest;

        // Repay 3,000
        let remaining = client.repay(&user, &3_000);

        // Expected: 15,000 - 3,000 = 12,000
        assert_eq!(remaining, total_debt - 3_000);
    }

    /// Test 8: Repay more than owed (should fail or cap at zero)
    ///
    /// Edge case: User tries to repay more than they owe.
    /// This should either fail or cap the debt at zero.
    #[test]
    #[should_panic(expected = "Overflow")]
    fn test_repay_more_than_owed_fails() {
        let (env, client, _admin, user) = setup();

        // Borrow 1,000
        client.borrow(&user, &1_000).unwrap();

        // Try to repay 2,000 (more than owed)
        client.repay(&user, &2_000);
    }

    /// Test 9: Sequential repays with time gaps
    ///
    /// Multiple repays with time advancement between each.
    #[test]
    fn test_sequential_repays_with_time_gaps() {
        let (env, client, _admin, user) = setup();

        // Borrow 10,000
        client.borrow(&user, &10_000).unwrap();

        // Wait 3 months
        let three_months = SECONDS_PER_YEAR / 4;
        advance_ledger_time(&env, three_months);

        // First repay: 1,000
        let remaining1 = client.repay(&user, &1_000);

        // Interest for 3 months: 10,000 * 1.25% = 125
        let interest1 = calculate_expected_interest(10_000, three_months, DEFAULT_APR_BPS);
        assert_eq!(interest1, 125);
        assert_eq!(remaining1, 10_000 + interest1 - 1_000);

        // Wait another 3 months
        advance_ledger_time(&env, three_months);

        // Second repay: 1,000
        let remaining2 = client.repay(&user, &1_000);

        // Interest on remaining1 for 3 months
        let interest2 = calculate_expected_interest(remaining1, three_months, DEFAULT_APR_BPS);
        assert_eq!(remaining2, remaining1 + interest2 - 1_000);
    }

    // ════════════════════════════════════════════════════════════════
    // Adversarial Tests (Security-Focused)
    // ════════════════════════════════════════════════════════════════

    /// Test 10: Adversarial - Attempt to exploit ordering by rapid repay
    ///
    /// Attacker tries to repay immediately after borrow to avoid interest.
    /// This should work (zero interest for zero time), but demonstrates
    /// that the ordering is correct.
    #[test]
    fn test_adversarial_rapid_repay_no_interest() {
        let (env, client, _admin, user) = setup();

        // Borrow 1,000,000
        let borrow_amount = 1_000_000i128;
        client.borrow(&user, &borrow_amount).unwrap();

        // Immediately repay (same timestamp)
        let remaining = client.repay(&user, &borrow_amount);

        // Should be zero (no interest accrued)
        assert_eq!(remaining, 0, "Immediate repay should have zero interest");
    }

    /// Test 11: Adversarial - Verify interest cannot be avoided by timing
    ///
    /// Even if user repays 1 second before a year boundary, interest
    /// accrues for the elapsed time.
    #[test]
    fn test_adversarial_timing_cannot_avoid_interest() {
        let (env, client, _admin, user) = setup();

        // Borrow 10,000
        client.borrow(&user, &10_000).unwrap();

        // Wait almost a year (1 second short)
        let almost_year = SECONDS_PER_YEAR - 1;
        advance_ledger_time(&env, almost_year);

        // Repay 1,000
        let remaining = client.repay(&user, &1_000);

        // Interest should still accrue for (SECONDS_PER_YEAR - 1) seconds
        let interest = calculate_expected_interest(10_000, almost_year, DEFAULT_APR_BPS);

        // Expected: 10,000 + interest - 1,000
        let expected = 10_000 + interest - 1_000;
        assert_eq!(
            remaining, expected,
            "Interest must accrue even 1 second before year boundary"
        );
    }

    /// Test 12: Adversarial - Large debt with minimal repay over time
    ///
    /// User borrows large amount and makes tiny repays to test
    /// interest accumulation on large principals.
    #[test]
    fn test_adversarial_large_debt_minimal_repay() {
        let (env, client, _admin, user) = setup();

        // Borrow maximum reasonable amount
        let borrow_amount = 1_000_000_000i128; // 1 billion
        client.borrow(&user, &borrow_amount).unwrap();

        // Wait 1 year
        advance_ledger_time(&env, SECONDS_PER_YEAR);

        // Interest: 1,000,000,000 * 5% = 50,000,000
        let interest = calculate_expected_interest(borrow_amount, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(interest, 50_000_000);

        // Repay tiny amount: 1,000
        let remaining = client.repay(&user, &1_000);

        // Expected: 1,000,000,000 + 50,000,000 - 1,000 = 1,049,999,000
        let expected = borrow_amount + interest - 1_000;
        assert_eq!(remaining, expected);
    }

    // ════════════════════════════════════════════════════════════════
    // Low-Level Debt Module Tests
    // ════════════════════════════════════════════════════════════════

    /// Test 13: Direct debt module test - repay_amount function
    ///
    /// Tests the low-level repay_amount function directly to verify
    /// accrual ordering at the module level.
    #[test]
    fn test_debt_module_repay_amount_accrues_first() {
        let env = Env::default();

        // Create initial debt position
        let initial_position = DebtPosition {
            principal: 10_000,
            last_update: 1000,
        };

        // Save to storage
        let user = Address::generate(&env);
        save_debt(&env, &user, &initial_position);

        // Advance time by 1 year
        let now = 1000 + SECONDS_PER_YEAR;

        // Call repay_amount directly
        let updated = repay_amount(initial_position, now, 1_000, DEFAULT_APR_BPS)
            .expect("repay should succeed");

        // Expected interest: 10,000 * 5% = 500
        let expected_interest = calculate_expected_interest(10_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(expected_interest, 500);

        // Expected principal after repay: 10,000 + 500 - 1,000 = 9,500
        assert_eq!(updated.principal, 9_500);
        assert_eq!(updated.last_update, now);
    }

    /// Test 14: Direct debt module test - borrow then repay
    ///
    /// Tests borrow_amount followed by repay_amount with time gap.
    #[test]
    fn test_debt_module_borrow_then_repay_with_time() {
        let env = Env::default();

        // Initial position (no debt)
        let initial = DebtPosition {
            principal: 0,
            last_update: 1000,
        };

        // Borrow 5,000 at t=1000
        let after_borrow = borrow_amount(initial, 1000, 5_000, DEFAULT_APR_BPS)
            .expect("borrow should succeed");
        assert_eq!(after_borrow.principal, 5_000);

        // Wait 6 months
        let six_months = SECONDS_PER_YEAR / 2;
        let repay_time = 1000 + six_months;

        // Repay 1,000
        let after_repay = repay_amount(after_borrow, repay_time, 1_000, DEFAULT_APR_BPS)
            .expect("repay should succeed");

        // Expected interest: 5,000 * 2.5% = 125
        let interest = calculate_expected_interest(5_000, six_months, DEFAULT_APR_BPS);
        assert_eq!(interest, 125);

        // Expected principal: 5,000 + 125 - 1,000 = 4,125
        assert_eq!(after_repay.principal, 4_125);
    }

    // ════════════════════════════════════════════════════════════════
    // Timestamp Boundary Tests
    // ════════════════════════════════════════════════════════════════

    /// Test 15: Repay at exact timestamp boundaries
    ///
    /// Tests repay at exact second, minute, hour, day, month, year boundaries.
    #[test]
    fn test_repay_at_timestamp_boundaries() {
        let (env, client, _admin, user) = setup();

        // Borrow 10,000
        client.borrow(&user, &10_000).unwrap();

        // Test various time boundaries
        let boundaries = vec![
            1,                      // 1 second
            60,                     // 1 minute
            3600,                   // 1 hour
            86400,                  // 1 day
            2_592_000,              // 30 days (1 month)
            SECONDS_PER_YEAR / 12,  // Exact 1 month
            SECONDS_PER_YEAR / 4,   // Exact 3 months
            SECONDS_PER_YEAR / 2,   // Exact 6 months
            SECONDS_PER_YEAR,       // Exact 1 year
        ];

        for (i, &time_delta) in boundaries.iter().enumerate() {
            // Reset for each test
            let user_i = Address::generate(&env);
            client.borrow(&user_i, &10_000).unwrap();

            advance_ledger_time(&env, time_delta);

            let remaining = client.repay(&user_i, &1_000);

            // Calculate expected
            let interest = calculate_expected_interest(10_000, time_delta, DEFAULT_APR_BPS);
            let expected = 10_000 + interest - 1_000;

            assert_eq!(
                remaining, expected,
                "Boundary test {} failed at {} seconds",
                i, time_delta
            );
        }
    }

    /// Test 16: Leap year handling (if applicable)
    ///
    /// Tests interest accrual over a leap year period.
    #[test]
    fn test_repay_over_leap_year() {
        let (env, client, _admin, user) = setup();

        // Borrow 10,000
        client.borrow(&user, &10_000).unwrap();

        // Leap year has 366 days = 31,622,400 seconds
        let leap_year_seconds = 366 * 24 * 60 * 60;
        advance_ledger_time(&env, leap_year_seconds);

        // Interest calculation uses SECONDS_PER_YEAR (365 days)
        // So leap year will accrue slightly more interest
        let interest = calculate_expected_interest(10_000, leap_year_seconds, DEFAULT_APR_BPS);

        // Repay 1,000
        let remaining = client.repay(&user, &1_000);

        let expected = 10_000 + interest - 1_000;
        assert_eq!(remaining, expected);
    }

    // ════════════════════════════════════════════════════════════════
    // Documentation and Expected Values
    // ════════════════════════════════════════════════════════════════

    /// Test 17: Document expected values for common scenarios
    ///
    /// This test serves as documentation for expected interest values.
    #[test]
    fn test_documented_expected_values() {
        let test_cases = vec![
            // (principal, time_seconds, expected_interest)
            (1_000, SECONDS_PER_YEAR, 50),           // 1,000 @ 5% for 1 year = 50
            (10_000, SECONDS_PER_YEAR, 500),         // 10,000 @ 5% for 1 year = 500
            (100_000, SECONDS_PER_YEAR, 5_000),      // 100,000 @ 5% for 1 year = 5,000
            (10_000, SECONDS_PER_YEAR / 2, 250),     // 10,000 @ 5% for 6 months = 250
            (10_000, SECONDS_PER_YEAR / 4, 125),     // 10,000 @ 5% for 3 months = 125
            (10_000, SECONDS_PER_YEAR / 12, 41),     // 10,000 @ 5% for 1 month ≈ 41
            (1_000_000, SECONDS_PER_YEAR, 50_000),   // 1M @ 5% for 1 year = 50,000
        ];

        for (principal, time, expected_interest) in test_cases {
            let actual = calculate_expected_interest(principal, time, DEFAULT_APR_BPS);
            assert_eq!(
                actual, expected_interest,
                "Expected interest mismatch for principal={}, time={}",
                principal, time
            );
        }
    }

    /// Test 18: Zero principal edge case
    ///
    /// Repaying when there's no debt should handle gracefully.
    #[test]
    #[should_panic]
    fn test_repay_with_zero_principal() {
        let (_env, client, _admin, user) = setup();

        // Try to repay without borrowing
        client.repay(&user, &1_000);
    }

    /// Test 19: Negative repay amount (should fail)
    #[test]
    #[should_panic]
    fn test_negative_repay_amount_fails() {
        let (_env, client, _admin, user) = setup();

        client.borrow(&user, &1_000).unwrap();
        client.repay(&user, &-100);
    }

    /// Test 20: Zero repay amount (should fail)
    #[test]
    #[should_panic]
    fn test_zero_repay_amount_fails() {
        let (_env, client, _admin, user) = setup();

        client.borrow(&user, &1_000).unwrap();
        client.repay(&user, &0);
    }
}
