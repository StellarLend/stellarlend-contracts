// ═══════════════════════════════════════════════════════════════════
// LIQUIDATION BRANCH TESTS
//
// Pins arithmetic branches in `liquidate`:
//
//   1. Close-factor cap   – amount > max_repay → capped at 50 % of debt
//   2. Seizure clamp      – seized_collateral > collateral → clamped
//   3. Sequential partial – two liquidations on the same borrower
//   4. Healthy rejection  – hf >= 10 000 → PositionHealthy
//   5. Zero-debt rejection – debt == 0 → PositionHealthy
//
// Cross-ref: stellar-lend/contracts/hello-world/liquidation_events.md
//
// Constants under test (assertions break if these change):
//   CLOSE_FACTOR   = 5 000 BPS (50 %)
//   INCENTIVE_BPS  = 1 000 BPS (10 %)
//   LIQUIDATION_THRESHOLD = 8 000 BPS (80 %)
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod liquidation_branch_tests {
    use crate::{LendingContract, LendingContractClient, LendingError};
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{Address, Env};

    // ── helpers ──────────────────────────────────────────────────────

    fn setup() -> (Env, LendingContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin)
    }

    /// Advance ledger timestamp so that accrued interest makes the
    /// position undercollateralised (hf < 10 000).
    fn advance_time(env: &Env, seconds: u64) {
        let mut info = env.ledger().get();
        info.timestamp = info.timestamp.saturating_add(seconds);
        env.ledger().set(info);
    }

    /// Deposit `col` and borrow `debt` for `borrower`, then advance time
    /// by `elapsed` seconds so interest tips the health factor below 1.
    ///
    /// The caller must ensure the chosen numbers actually produce hf < 10 000
    /// after the time advance (verified inside each test via assertions).
    fn make_undercollateralised(
        env: &Env,
        client: &LendingContractClient<'static>,
        borrower: &Address,
        col: i128,
        debt: i128,
        elapsed: u64,
    ) {
        client.deposit(borrower, &col).unwrap();
        client.borrow(borrower, &debt).unwrap();
        advance_time(env, elapsed);
    }

    // ─────────────────────────────────────────────────────────────────
    // Test 1 – Close-factor cap
    //
    // Setup: collateral=1 000, borrow=1 000, wait long enough that the
    // position is underwater.  Pass amount=1 000 (100 % of debt).
    //
    // Expected:
    //   actual_repay = floor(debt * 5 000 / 10 000)   [50 % cap applied]
    //   new_debt     = debt - actual_repay
    //   new_col      = col  - actual_repay * 11 000 / 10 000   [10 % incentive]
    // ─────────────────────────────────────────────────────────────────
    #[test]
    fn test_close_factor_cap_applied_when_amount_exceeds_half_debt() {
        let (env, client, _admin) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        // col=1 000, debt=1 000 → hf = 1000*8000/1000 = 8000 < 10000 (already liquidatable)
        make_undercollateralised(&env, &client, &borrower, 1_000, 1_000, 0);

        // Confirm undercollateralised before liquidation
        let hf_before = client.get_health_factor(&borrower);
        assert!(hf_before < 10_000, "position must be liquidatable; hf={hf_before}");

        let debt_before = client.get_debt_position(&borrower).principal;

        // Pass full debt as amount – close factor should cap at 50 %
        let actual_repay = client.liquidate(&liquidator, &borrower, &debt_before).unwrap();

        // CLOSE_FACTOR = 5 000 BPS → max_repay = debt * 5000 / 10000
        let expected_repay = debt_before * 5_000 / 10_000;
        assert_eq!(
            actual_repay, expected_repay,
            "close factor cap: expected {expected_repay}, got {actual_repay}"
        );

        // Verify debt storage was reduced by exactly actual_repay
        let debt_after = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after, debt_before - actual_repay);

        // Verify collateral = original - seized (with 10 % incentive, unclamped here)
        let seized = actual_repay * 11_000 / 10_000;
        let col_after = client.get_position(&borrower).collateral;
        assert_eq!(col_after, 1_000 - seized);
    }

    // ─────────────────────────────────────────────────────────────────
    // Test 2 – Seizure clamp
    //
    // Create a position where seized_collateral (repay * 110 %) would
    // exceed available collateral.  Use col=100, debt=1 000; the collateral
    // is tiny relative to debt so the 10 % bonus would overshoot.
    //
    // At col=100, debt=1 000: hf = 100*8000/1000 = 800 < 10000. ✓
    // max_repay = 1000 * 50 % = 500; seized_candidate = 500*110% = 550 > 100.
    // Clamp: final_seized = 100 (all collateral).
    // ─────────────────────────────────────────────────────────────────
    #[test]
    fn test_seizure_clamp_when_incentive_exceeds_available_collateral() {
        let (env, client, _admin) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        // col=100 is far below debt=1 000 → position is immediately liquidatable
        make_undercollateralised(&env, &client, &borrower, 100, 1_000, 0);

        let hf = client.get_health_factor(&borrower);
        assert!(hf < 10_000, "hf={hf}");

        let debt_before = client.get_debt_position(&borrower).principal;
        // Pass full debt; close factor caps to 500, seized candidate = 550 > 100
        let actual_repay = client.liquidate(&liquidator, &borrower, &debt_before).unwrap();

        // actual_repay is still the capped amount (not further reduced by clamp)
        let expected_repay = debt_before * 5_000 / 10_000;
        assert_eq!(actual_repay, expected_repay);

        // Collateral must be fully seized (clamped to 100)
        let col_after = client.get_position(&borrower).collateral;
        assert_eq!(col_after, 0, "all collateral should be seized via clamp");

        // Debt reduced only by actual_repay (500), not more
        let debt_after = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after, debt_before - actual_repay);
    }

    // ─────────────────────────────────────────────────────────────────
    // Test 3 – Sequential partial liquidations on the same borrower
    //
    // First liquidation reduces debt by 50 %; second liquidation (if
    // position is still unhealthy) reduces the remaining debt by 50 %.
    //
    // col=200, debt=1 000 → hf = 200*8000/1000 = 1600 < 10000. ✓
    // Round 1: repay = 500, seized = 550 → new col = -350 → clamped to 0?
    //   Actually seized = 500*1.1 = 550 > 200, so final_seized = 200, col=0.
    // Round 2: col=0, debt=500 → hf=0 < 10000.
    //   repay = 250, seized_candidate = 275 > 0 → clamped to 0.
    // ─────────────────────────────────────────────────────────────────
    #[test]
    fn test_sequential_partial_liquidations_reduce_debt_cumulatively() {
        let (env, client, _admin) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        make_undercollateralised(&env, &client, &borrower, 200, 1_000, 0);

        let hf = client.get_health_factor(&borrower);
        assert!(hf < 10_000, "hf={hf}");

        // ── Round 1 ──
        let debt_r1 = client.get_debt_position(&borrower).principal;
        let repay_r1 = client.liquidate(&liquidator, &borrower, &debt_r1).unwrap();
        assert_eq!(repay_r1, debt_r1 * 5_000 / 10_000);
        let debt_after_r1 = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after_r1, debt_r1 - repay_r1);

        // ── Round 2 ──
        // Position still unhealthy (col=0, debt=500 → hf=0)
        let hf_r2 = client.get_health_factor(&borrower);
        assert!(hf_r2 < 10_000, "still liquidatable after round 1; hf={hf_r2}");

        let debt_r2 = client.get_debt_position(&borrower).principal;
        let repay_r2 = client.liquidate(&liquidator, &borrower, &debt_r2).unwrap();
        assert_eq!(repay_r2, debt_r2 * 5_000 / 10_000);
        let debt_after_r2 = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after_r2, debt_r2 - repay_r2);

        // Cumulative debt reduction
        assert_eq!(
            debt_after_r2,
            debt_r1 - repay_r1 - repay_r2,
            "cumulative debt mismatch"
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Test 4 – Healthy position is rejected
    //
    // col=10 000, debt=100 → hf = 10000*8000/100 = 800 000 >> 10000.
    // liquidate must return Err(PositionHealthy).
    // ─────────────────────────────────────────────────────────────────
    #[test]
    fn test_healthy_position_rejected_with_position_healthy_error() {
        let (env, client, _admin) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        client.deposit(&borrower, &10_000).unwrap();
        client.borrow(&borrower, &100).unwrap();

        let hf = client.get_health_factor(&borrower);
        assert!(hf >= 10_000, "must be healthy; hf={hf}");

        let result = client.try_liquidate(&liquidator, &borrower, &100);
        assert_eq!(result, Ok(Err(LendingError::PositionHealthy)));
    }

    // ─────────────────────────────────────────────────────────────────
    // Test 5 – Zero-debt rejection
    //
    // A borrower who never borrowed (debt == 0) must also be rejected.
    // ─────────────────────────────────────────────────────────────────
    #[test]
    fn test_zero_debt_rejected_with_position_healthy_error() {
        let (env, client, _admin) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        // Deposit only, no borrow → debt == 0
        client.deposit(&borrower, &1_000).unwrap();

        let result = client.try_liquidate(&liquidator, &borrower, &100);
        assert_eq!(result, Ok(Err(LendingError::PositionHealthy)));
    }
}
