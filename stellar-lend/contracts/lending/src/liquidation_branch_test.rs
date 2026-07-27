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
    use crate::debt::{accrue_interest, DEFAULT_APR_BPS};
    use crate::{LendingContract, LendingContractClient, LendingError};
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::token::StellarAssetClient;
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
        let mut info: LedgerInfo = env.ledger().get();
        info.timestamp = info.timestamp.saturating_add(seconds);
        info.sequence_number = info.sequence_number.saturating_add(1);
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
        client.deposit(borrower, &col);
        client.borrow(borrower, &debt);
        advance_time(env, elapsed);
    }

    /// Compute the settled principal after interest accrual, matching the
    /// contract's internal `settle_and_accrue_insurance` logic.
    fn settled_principal(principal: i128, elapsed: u64) -> i128 {
        let interest = accrue_interest(principal, elapsed, DEFAULT_APR_BPS).unwrap();
        principal + interest
    }

    /// Register Stellar Asset Contracts for debt and collateral, then mint
    /// enough tokens so the token transfers inside `liquidate` succeed.
    fn setup_tokens(
        env: &Env,
        contract_id: &Address,
        liquidator: &Address,
        debt_needed: i128,
        col_needed: i128,
    ) -> (Address, Address) {
        let debt_admin = Address::generate(env);
        let col_admin = Address::generate(env);
        let debt_asset = env.register_stellar_asset_contract(debt_admin);
        let col_asset = env.register_stellar_asset_contract(col_admin);
        let debt_token = StellarAssetClient::new(env, &debt_asset);
        let col_token = StellarAssetClient::new(env, &col_asset);
        if debt_needed > 0 {
            debt_token.mint(liquidator, &debt_needed);
        }
        if col_needed > 0 {
            col_token.mint(contract_id, &col_needed);
        }
        (debt_asset, col_asset)
    }

    /// Deposit 1_000, borrow 790 (79% LTV — passes health check).
    /// Advance ~116 days so interest pushes HF below 10 000.
    /// Passing the full principal triggers the close-factor cap: actual_repay = settled/2.
    #[test]
    fn test_close_factor_cap_applied_when_amount_exceeds_half_debt() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);
        const ELAPSED: u64 = 10_000_000;

        make_undercollateralised(&env, &client, &borrower, 1_000, 790, ELAPSED);

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

        let (debt_asset, collateral_asset) =
            setup_tokens(&env, &client.address, &liquidator, max_repay, seized);

        let actual_repay = client.liquidate(
            &liquidator,
            &borrower,
            &debt_asset,
            &collateral_asset,
            &stored,
        );

        // CLOSE_FACTOR = 5000 BPS applied against settled principal
        assert_eq!(actual_repay, max_repay);

        // debt storage reduced by actual_repay
        let debt_after = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after, settled - actual_repay);

        // collateral reduced by seized = actual_repay * 110% (no clamp here)
        let col_after = client.get_position(&borrower).collateral;
        assert_eq!(col_after, 1_000 - seized);
    }

    /// Deposit 100, borrow 79 (79% LTV).
    /// Advance ~27 years so interest balloons the debt.
    /// Passing the fully-settled debt triggers both the close-factor cap (50%)
    /// and the seizure clamp (incentivized collateral > available).
    #[test]
    fn test_seizure_clamp_when_incentive_exceeds_available_collateral() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);
        const ELAPSED: u64 = 850_000_000;

        make_undercollateralised(&env, &client, &borrower, 100, 79, ELAPSED);

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

        let (debt_asset, collateral_asset) = setup_tokens(
            &env,
            &client.address,
            &liquidator,
            max_repay,
            100, // contract needs at most collateral = 100
        );

        // Pass the full settled amount so the cap binds and seized > collateral.
        let actual_repay = client.liquidate(
            &liquidator,
            &borrower,
            &debt_asset,
            &collateral_asset,
            &settled,
        );

        // repay is capped at 50 % of settled principal
        assert_eq!(actual_repay, max_repay);

        // all collateral drained (seized > available, clamped to zero)
        assert_eq!(client.get_position(&borrower).collateral, 0);

        // debt reduced by actual_repay
        let debt_after = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after, settled - actual_repay);
    }

    /// Deposit 200, borrow 158 (79% LTV).
    /// Advance ~27 years so interest balloons debt enough to trigger the
    /// close-factor cap.  Round 1 drains all collateral (clamped).
    /// Round 2 still succeeds because hf=0 (no collateral, remaining debt).
    #[test]
    fn test_sequential_partial_liquidations_reduce_debt_cumulatively() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);
        const ELAPSED: u64 = 850_000_000;

        make_undercollateralised(&env, &client, &borrower, 200, 158, ELAPSED);

        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);

        // Round 1
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

        let (debt_asset, collateral_asset) = setup_tokens(
            &env,
            &client.address,
            &liquidator,
            max_repay_r1,
            200, // contract needs at most collateral = 200
        );

        let repay_r1 = client.liquidate(
            &liquidator,
            &borrower,
            &debt_asset,
            &collateral_asset,
            &settled_r1,
        );
        assert_eq!(repay_r1, max_repay_r1);
        let debt_after_r1 = client.get_debt_position(&borrower).principal;
        assert_eq!(debt_after_r1, settled_r1 - repay_r1);

        // still liquidatable (no collateral or HF < 10 000)
        assert!(client.get_health_factor(&borrower) < 10_000);

        // Round 2 — no elapsed time, so stored principal equals settled
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
        assert_eq!(debt_after_r2, settled_r1 - repay_r1 - repay_r2);
    }

    /// col=10_000 debt=100 -> hf=800_000 >= 10_000: must reject.
    #[test]
    fn test_healthy_position_rejected_with_position_healthy_error() {
        let (env, client) = setup();
        let borrower = Address::generate(&env);
        let liquidator = Address::generate(&env);

        client.deposit(&borrower, &10_000);
        client.borrow(&borrower, &100);

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

        client.deposit(&borrower, &1_000);

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
