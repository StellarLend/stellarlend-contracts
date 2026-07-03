// ════════════════════════════════════════════════════════════════
// LEDGER-TIME ADVANCEMENT TESTS: Interest Accrual Ordering on Repay
// ════════════════════════════════════════════════════════════════
//
// Verifies that interest is accrued BEFORE the repay amount is subtracted,
// ensuring correct debt calculation across time boundaries.
//
// Security invariant – the order of operations on repay MUST be:
//   1. Accrue interest based on elapsed time.
//   2. Apply repayment to the accrued total.
//
// If the order were reversed (apply-then-accrue), users could repay before
// interest accrues, effectively obtaining interest-free loans.
// ════════════════════════════════════════════════════════════════

#[cfg(test)]
mod interest_ordering_time_tests {
    use crate::debt::{
        borrow_amount, repay_amount, save_debt, DebtPosition, DEFAULT_APR_BPS,
    };
    use crate::rounding_strategy::SECONDS_PER_YEAR;
    use crate::{LendingContract, LendingContractClient};
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::{Address, Env};

    // ────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ────────────────────────────────────────────────────────────────────────

    /// Set up the contract and seed `user` with enough collateral to borrow
    /// up to `collateral` units.  The caller decides the exact collateral
    /// amount; pass something large enough for the borrow being tested.
    fn setup_with_collateral(
        collateral: i128,
    ) -> (Env, LendingContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        client.initialize(&admin);
        client.deposit(&user, &collateral);

        (env, client, user)
    }

    /// Advance ledger timestamp and sequence by `seconds`.
    fn advance_time(env: &Env, seconds: u64) {
        let mut li: LedgerInfo = env.ledger().get();
        li.timestamp = li.timestamp.saturating_add(seconds);
        li.sequence_number = li.sequence_number.saturating_add(1);
        env.ledger().set(li);
    }

    /// Simple-interest formula matching the debt module:
    ///   interest = principal * elapsed * rate_bps / (SECONDS_PER_YEAR * 10_000)
    fn expected_interest(principal: i128, elapsed: u64, rate_bps: i128) -> i128 {
        principal
            .checked_mul(elapsed as i128)
            .and_then(|v| v.checked_mul(rate_bps))
            .unwrap()
            / ((SECONDS_PER_YEAR as i128) * 10_000)
    }

    // ────────────────────────────────────────────────────────────────────────
    // Core ordering tests
    // ────────────────────────────────────────────────────────────────────────

    /// Repay immediately after borrow (same timestamp) — no interest should
    /// accrue and the principal is reduced by the exact repayment amount.
    #[test]
    fn test_repay_immediately_zero_elapsed_time() {
        let (env, client, user) = setup_with_collateral(2_000);
        let _ = env; // keep env alive

        client.borrow(&user, &1_000);
        let remaining = client.repay(&user, &300);

        assert_eq!(remaining, 700, "immediate repay: no interest, principal -= repay");
    }

    /// After one full year the debt module accrues interest before applying
    /// the repayment.
    ///
    ///   principal = 10_000, rate = DEFAULT_APR_BPS (500 = 5 %)
    ///   interest  = 10_000 * 5 % = 500
    ///   debt before repay = 10_500; after repaying 1_000 → 9_500
    #[test]
    fn test_repay_after_one_year_accrues_first() {
        let (env, client, user) = setup_with_collateral(25_000);

        client.borrow(&user, &10_000);
        advance_time(&env, SECONDS_PER_YEAR);

        let interest = expected_interest(10_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(interest, 500);

        let remaining = client.repay(&user, &1_000);
        assert_eq!(
            remaining,
            10_000 + interest - 1_000,
            "repay must apply to accrued debt"
        );
    }

    /// Repaying less than accrued interest still reduces total debt correctly.
    #[test]
    fn test_repay_smaller_than_accrued_interest() {
        let (env, client, user) = setup_with_collateral(250_000);

        client.borrow(&user, &100_000);
        advance_time(&env, SECONDS_PER_YEAR);

        let interest = expected_interest(100_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(interest, 5_000);

        let remaining = client.repay(&user, &2_000);
        assert_eq!(remaining, 100_000 + interest - 2_000);
    }

    /// Two borrows separated by six months; interest accrues on each borrow
    /// separately then compounds on the second half.
    #[test]
    fn test_multiple_borrows_and_repays_with_time() {
        let (env, client, user) = setup_with_collateral(50_000);

        client.borrow(&user, &10_000);

        let six_months = SECONDS_PER_YEAR / 2;
        advance_time(&env, six_months);

        // Interest after 6 months: 10,000 * 2.5 % = 250
        let int_6m = expected_interest(10_000, six_months, DEFAULT_APR_BPS);
        assert_eq!(int_6m, 250);

        // Second borrow accrues interest first → principal = 10,250 + 5,000
        client.borrow(&user, &5_000);
        let pos = client.get_debt_position(&user);
        assert_eq!(pos.principal, 15_250);

        advance_time(&env, six_months);

        let int_2nd = expected_interest(15_250, six_months, DEFAULT_APR_BPS);
        assert_eq!(int_2nd, 381);

        let remaining = client.repay(&user, &5_000);
        assert_eq!(remaining, 15_250 + int_2nd - 5_000);
    }

    /// Repaying the exact total (principal + interest) zeroes the debt.
    #[test]
    fn test_repay_exact_debt_including_interest() {
        let (env, client, user) = setup_with_collateral(5_000);

        client.borrow(&user, &1_000);
        advance_time(&env, SECONDS_PER_YEAR);

        let interest = expected_interest(1_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(interest, 50);

        let remaining = client.repay(&user, &(1_000 + interest));
        assert_eq!(remaining, 0, "exact repay must zero the debt");
    }

    /// Very long time period (10 years) — simple interest accumulates linearly.
    #[test]
    fn test_repay_after_ten_years() {
        let (env, client, user) = setup_with_collateral(50_000);

        client.borrow(&user, &10_000);

        let ten_years = SECONDS_PER_YEAR * 10;
        advance_time(&env, ten_years);

        let interest = expected_interest(10_000, ten_years, DEFAULT_APR_BPS);
        assert_eq!(interest, 5_000);

        let remaining = client.repay(&user, &3_000);
        assert_eq!(remaining, 10_000 + interest - 3_000);
    }

    /// Repaying more than owed is rejected.
    #[test]
    #[should_panic]
    fn test_repay_more_than_owed_panics() {
        let (_env, client, user) = setup_with_collateral(5_000);
        client.borrow(&user, &1_000);
        client.repay(&user, &2_000);
    }

    /// Sequential repays with time gaps — each repay operates on the
    /// debt accrued since the last operation.
    #[test]
    fn test_sequential_repays_with_time_gaps() {
        let (env, client, user) = setup_with_collateral(50_000);

        client.borrow(&user, &10_000);

        let three_months = SECONDS_PER_YEAR / 4;
        advance_time(&env, three_months);

        let int1 = expected_interest(10_000, three_months, DEFAULT_APR_BPS);
        assert_eq!(int1, 125);

        let rem1 = client.repay(&user, &1_000);
        assert_eq!(rem1, 10_000 + int1 - 1_000);

        advance_time(&env, three_months);

        let int2 = expected_interest(rem1, three_months, DEFAULT_APR_BPS);
        let rem2 = client.repay(&user, &1_000);
        assert_eq!(rem2, rem1 + int2 - 1_000);
    }

    // ────────────────────────────────────────────────────────────────────────
    // Adversarial tests
    // ────────────────────────────────────────────────────────────────────────

    /// Immediate repay with zero elapsed time yields exactly zero interest.
    #[test]
    fn test_adversarial_rapid_repay_no_interest() {
        let (_env, client, user) = setup_with_collateral(5_000_000);
        client.borrow(&user, &1_000_000);
        let remaining = client.repay(&user, &1_000_000);
        assert_eq!(remaining, 0, "immediate repay must leave zero debt");
    }

    /// Interest accrues even 1 second before a year boundary.
    #[test]
    fn test_adversarial_timing_cannot_avoid_interest() {
        let (env, client, user) = setup_with_collateral(50_000);

        client.borrow(&user, &10_000);
        let almost_year = SECONDS_PER_YEAR - 1;
        advance_time(&env, almost_year);

        let interest = expected_interest(10_000, almost_year, DEFAULT_APR_BPS);
        let remaining = client.repay(&user, &1_000);
        assert_eq!(remaining, 10_000 + interest - 1_000);
    }

    /// Large principal with small repay over one year.
    #[test]
    fn test_adversarial_large_debt_minimal_repay() {
        let (env, client, user) = setup_with_collateral(5_000_000_000i128);

        client.borrow(&user, &1_000_000_000);
        advance_time(&env, SECONDS_PER_YEAR);

        let interest = expected_interest(1_000_000_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(interest, 50_000_000);

        let remaining = client.repay(&user, &1_000);
        assert_eq!(remaining, 1_000_000_000 + interest - 1_000);
    }

    // ────────────────────────────────────────────────────────────────────────
    // Low-level debt-module tests (no contract registration needed)
    // ────────────────────────────────────────────────────────────────────────

    /// `repay_amount` in the debt module accrues interest before subtracting
    /// the repayment.
    #[test]
    fn test_debt_module_repay_amount_accrues_first() {
        let env = Env::default();
        let user = Address::generate(&env);

        let initial = DebtPosition { principal: 10_000, last_update: 1_000 };
        save_debt(&env, &user, &initial);

        let now = 1_000 + SECONDS_PER_YEAR;
        let updated = repay_amount(initial, now, 1_000, DEFAULT_APR_BPS)
            .expect("repay should succeed");

        let interest = expected_interest(10_000, SECONDS_PER_YEAR, DEFAULT_APR_BPS);
        assert_eq!(interest, 500);
        assert_eq!(updated.principal, 10_000 + interest - 1_000);
        assert_eq!(updated.last_update, now);
    }

    /// `borrow_amount` followed by `repay_amount` with a six-month gap.
    #[test]
    fn test_debt_module_borrow_then_repay_with_time() {
        let env = Env::default();
        let user = Address::generate(&env);

        let initial = DebtPosition { principal: 0, last_update: 1_000 };
        save_debt(&env, &user, &initial);

        let after_borrow = borrow_amount(initial, 1_000, 5_000, DEFAULT_APR_BPS)
            .expect("borrow should succeed");
        assert_eq!(after_borrow.principal, 5_000);

        let six_months = SECONDS_PER_YEAR / 2;
        let repay_time = 1_000 + six_months;

        let after_repay = repay_amount(after_borrow, repay_time, 1_000, DEFAULT_APR_BPS)
            .expect("repay should succeed");

        let interest = expected_interest(5_000, six_months, DEFAULT_APR_BPS);
        assert_eq!(interest, 125);
        assert_eq!(after_repay.principal, 5_000 + interest - 1_000);
    }

    // ────────────────────────────────────────────────────────────────────────
    // Documented expected values
    // ────────────────────────────────────────────────────────────────────────

    /// Reference table of (principal, seconds, expected_interest) triples.
    #[test]
    fn test_documented_expected_values() {
        let cases: &[(i128, u64, i128)] = &[
            (1_000,   SECONDS_PER_YEAR,      50),
            (10_000,  SECONDS_PER_YEAR,     500),
            (100_000, SECONDS_PER_YEAR,   5_000),
            (10_000,  SECONDS_PER_YEAR / 2,  250),
            (10_000,  SECONDS_PER_YEAR / 4,  125),
            (10_000,  SECONDS_PER_YEAR / 12,  41),
            (1_000_000, SECONDS_PER_YEAR, 50_000),
        ];
        for &(principal, time, exp) in cases {
            let actual = expected_interest(principal, time, DEFAULT_APR_BPS);
            assert_eq!(
                actual, exp,
                "principal={principal} time={time}: expected {exp} got {actual}"
            );
        }
    }

    /// Repaying with no prior debt must panic.
    #[test]
    #[should_panic]
    fn test_repay_with_no_debt_panics() {
        let (_env, client, user) = setup_with_collateral(1_000);
        client.repay(&user, &1_000);
    }

    /// Negative repay amount must be rejected.
    #[test]
    #[should_panic]
    fn test_negative_repay_amount_panics() {
        let (_env, client, user) = setup_with_collateral(5_000);
        client.borrow(&user, &1_000);
        client.repay(&user, &-100);
    }

    /// Zero repay amount must be rejected.
    #[test]
    #[should_panic]
    fn test_zero_repay_amount_panics() {
        let (_env, client, user) = setup_with_collateral(5_000);
        client.borrow(&user, &1_000);
        client.repay(&user, &0);
    }
}
