// Liquidation branch tests
//
// Pins every arithmetic branch in `liquidate`:
//   1. Close-factor cap   - amount > max_repay -> capped at 50% of debt
//   2. Seizure clamp      - seized_collateral > collateral -> clamped
//   3. Sequential partial - two liquidations on the same borrower
//   4. Healthy rejection  - hf >= 10_000 -> PositionHealthy
//   5. Zero-debt rejection - debt == 0 -> PositionHealthy
//
// Cross-ref: stellar-lend/contracts/hello-world/liquidation_events.md
//
// Assertions break if CLOSE_FACTOR (5000 BPS), INCENTIVE_BPS (1000 BPS),
// or LIQUIDATION_THRESHOLD (8000 BPS) constants change.

#[cfg(test)]
mod liquidation_branch_tests {
    use crate::{LendingContract, LendingContractClient, LendingError};
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{Address, Env};

    fn setup() -> (Env, LendingContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, client)
    }

    fn advance_time(env: &Env, seconds: u64) {
        let mut info = env.ledger().get();
        info.timestamp = info.timestamp.saturating_add(seconds);
        env.ledger().set(info);
    }

    /// Build an undercollateralised position: deposit `col`, borrow `debt`,
    /// then advance time by `elapsed` seconds.
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

    /// col=1_000 debt=1_000 -> hf = 1000*8000/1000 = 8000 < 10000 (liquidatable).
    /// Passing the full debt triggers the close-factor cap: actual_repay = debt/2.
    #[test]
    fn test_close_factor_cap_applied_when_amount_exceeds_half_debt() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        make_undercollateralised(&env, &client, &borrower, 1_000, 1_000, 0);

        let hf = client.get_health_factor(&borrower);
        assert!(hf < 10_000, "expected liquidatable position; hf={hf}");

        let debt_before = client.get_debt_position(&borrower).principal;
        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);
        let actual_repay = client
            .liquidate(
                &liquidator,
                &borrower,
                &debt_asset,
                &collateral_asset,
                &debt_before,
            )
            .unwrap();

        // CLOSE_FACTOR = 5000 BPS
        let expected_repay = debt_before * 5_000 / 10_000;
        assert_eq!(actual_repay, expected_repay);

        // debt storage reduced by actual_repay
        let debt_after = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after, debt_before - actual_repay);

        // collateral reduced by seized = actual_repay * 110% (no clamp here)
        let seized = actual_repay * 11_000 / 10_000;
        let col_after = client.get_position(&borrower).collateral;
        assert_eq!(col_after, 1_000 - seized);
    }

    /// col=100 debt=1_000 -> hf=800 < 10000.
    /// max_repay=500, seized_candidate=550 > 100 -> clamped; col goes to 0.
    #[test]
    fn test_seizure_clamp_when_incentive_exceeds_available_collateral() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        make_undercollateralised(&env, &client, &borrower, 100, 1_000, 0);

        let debt_before = client.get_debt_position(&borrower).principal;
        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);
        let actual_repay = client
            .liquidate(
                &liquidator,
                &borrower,
                &debt_asset,
                &collateral_asset,
                &debt_before,
            )
            .unwrap();

        // repay is still the close-factor-capped amount
        assert_eq!(actual_repay, debt_before * 5_000 / 10_000);

        // all collateral drained
        assert_eq!(client.get_position(&borrower).collateral, 0);

        // debt reduced by actual_repay only
        let debt_after = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after, debt_before - actual_repay);
    }

    /// col=200 debt=1_000. Round 1 seizes all collateral (200 < 550),
    /// Round 2 still succeeds because hf=0 (no collateral, remaining debt).
    #[test]
    fn test_sequential_partial_liquidations_reduce_debt_cumulatively() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        make_undercollateralised(&env, &client, &borrower, 200, 1_000, 0);

        // Round 1
        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);
        let debt_r1 = client.get_debt_position(&borrower).principal;
        let repay_r1 = client
            .liquidate(
                &liquidator,
                &borrower,
                &debt_asset,
                &collateral_asset,
                &debt_r1,
            )
            .unwrap();
        assert_eq!(repay_r1, debt_r1 * 5_000 / 10_000);
        let debt_after_r1 = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after_r1, debt_r1 - repay_r1);

        // still liquidatable (col=0, debt=500 -> hf=0)
        assert!(client.get_health_factor(&borrower) < 10_000);

        // Round 2
        let debt_r2 = client.get_debt_position(&borrower).principal;
        let repay_r2 = client
            .liquidate(
                &liquidator,
                &borrower,
                &debt_asset,
                &collateral_asset,
                &debt_r2,
            )
            .unwrap();
        assert_eq!(repay_r2, debt_r2 * 5_000 / 10_000);
        let debt_after_r2 = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after_r2, debt_r2 - repay_r2);

        // cumulative
        assert_eq!(debt_after_r2, debt_r1 - repay_r1 - repay_r2);
    }

    /// col=10_000 debt=100 -> hf=800_000 >= 10_000: must reject.
    #[test]
    fn test_healthy_position_rejected_with_position_healthy_error() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        client.deposit(&borrower, &10_000).unwrap();
        client.borrow(&borrower, &100).unwrap();

        assert!(client.get_health_factor(&borrower) >= 10_000);

        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);
        let result =
            client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
        assert!(
            matches!(result, Err(Ok(LendingError::PositionHealthy))),
            "expected PositionHealthy error, got {:?}",
            result
        );
    }

    /// No borrow -> debt == 0: must reject.
    #[test]
    fn test_zero_debt_rejected_with_position_healthy_error() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        client.deposit(&borrower, &1_000).unwrap();

        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);
        let result =
            client.try_liquidate(&liquidator, &borrower, &debt_asset, &collateral_asset, &100);
        assert!(
            matches!(result, Err(Ok(LendingError::PositionHealthy))),
            "expected PositionHealthy error, got {:?}",
            result
        );
    }
}
