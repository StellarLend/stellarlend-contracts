//! Doc-test verifying the multi-asset health factor worked example from
//! [`CROSS_ASSET_HEALTH.md`](../CROSS_ASSET_HEALTH.md).
//!
//! # What this file tests
//!
//! Each test in this module is a self-contained verification of one entry in
//! the specification document.  The tests are named after the section they
//! correspond to and the expected value is the literal constant from the spec.
//!
//! | Test | Spec section | Expected HF |
//! |------|-------------|-------------|
//! | `test_two_collateral_two_debt_health_factor` | §5 Worked Example | 36 666 |
//! | `test_no_debt_saturated_sentinel` | §3 No-Debt Value | 100 000 000 |
//! | `test_single_collateral_single_debt_boundary` | §6.1 Edge Cases | 10 000 |
//! | `test_floor_rounding_direction` | §4 + §6.3 Rounding | 36 666 (not 36 667) |
//!
//! # Numeric constants used
//!
//! ```text
//! PRICE_DIVISOR          = 10_000_000   ($1.00 in oracle units)
//! HEALTH_FACTOR_SCALE    = 10_000       (1.0 × scale)
//! HEALTH_FACTOR_NO_DEBT  = 100_000_000  (sentinel, no debt)
//! ```
//!
//! All prices below follow the 7-decimal convention:
//! `$0.50 = 5_000_000`, `$1.00 = 10_000_000`, `$2.00 = 20_000_000`.

#[cfg(test)]
mod cross_asset_health_doctest {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, Env};

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Inject an oracle price directly into persistent storage.
    ///
    /// `price` is 7-decimal fixed-point: `10_000_000` = $1.00.
    /// The timestamp is set to the current ledger time so the staleness
    /// check (`DEFAULT_ORACLE_MAX_AGE_SECS = 3600`) always passes.
    fn set_price(env: &Env, contract_id: &Address, asset: &Address, price: i128) {
        env.as_contract(contract_id, || {
            env.storage().persistent().set(
                &DataKey::OraclePrice(asset.clone()),
                &PriceRecord {
                    price,
                    timestamp: env.ledger().timestamp(),
                },
            );
        });
    }

    /// Build a minimal contract environment with two configured assets.
    ///
    /// Asset A  — XLM proxy:  price $0.50 (5_000_000),  LT 7 500 bps (75 %)
    /// Asset B  — USDC proxy: price $1.00 (10_000_000), LT 9 000 bps (90 %)
    ///
    /// Returns `(env, client, contract_id, admin, user, asset_a, asset_b)`.
    fn setup_two_asset() -> (
        Env,
        LendingContractClient<'static>,
        Address, // contract id
        Address, // admin
        Address, // user
        Address, // asset_a (XLM proxy, $0.50, LT 75%)
        Address, // asset_b (USDC proxy, $1.00, LT 90%)
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let asset_a = env.register(MockAsset, ());
        let asset_b = env.register(MockAsset, ());

        client.initialize(&admin);

        // Asset A (XLM proxy): 75% LTV, 75% liquidation threshold (7_500 bps)
        client.set_asset_params(
            &admin,
            &asset_a,
            &7500i128,             // ltv_bps
            &7500i128,             // liquidation_threshold_bps
            &1_000_000_000_000i128, // debt_ceiling (unconstrained)
            &0i128,                // borrow_cap   (0 = uncapped)
        );

        // Asset B (USDC proxy): 90% LTV, 90% liquidation threshold (9_000 bps)
        client.set_asset_params(
            &admin,
            &asset_b,
            &9000i128,             // ltv_bps
            &9000i128,             // liquidation_threshold_bps
            &1_000_000_000_000i128, // debt_ceiling (unconstrained)
            &0i128,                // borrow_cap   (0 = uncapped)
        );

        // Asset A price: $0.50 → 5_000_000 raw
        set_price(&env, &id, &asset_a, 5_000_000);
        // Asset B price: $1.00 → 10_000_000 raw
        set_price(&env, &id, &asset_b, 10_000_000);

        (env, client, id, admin, user, asset_a, asset_b)
    }

    // ── §5 Worked Example ─────────────────────────────────────────────────────

    /// Verify the two-collateral, two-debt scenario from §5 of CROSS_ASSET_HEALTH.md.
    ///
    /// # Position
    ///
    /// | Role | Asset | Amount | Price (raw) | LT (bps) |
    /// |------|-------|--------|-------------|----------|
    /// | Collateral | A (XLM)  | 2 000 | 5 000 000  | 7 500 |
    /// | Collateral | B (USDC) | 1 000 | 10 000 000 | 9 000 |
    /// | Debt       | A (XLM)  |   500 | 5 000 000  | —     |
    /// | Debt       | B (USDC) |   200 | 10 000 000 | —     |
    ///
    /// # Expected arithmetic (from spec §5)
    ///
    /// ```text
    /// weighted_collateral  =  2_000 × 5_000_000 × 7_500
    ///                      +  1_000 × 10_000_000 × 9_000
    ///                      =  75_000_000_000_000
    ///                      +  90_000_000_000_000
    ///                      = 165_000_000_000_000
    ///
    /// total_debt_value     =    500 × 5_000_000
    ///                      +    200 × 10_000_000
    ///                      =  2_500_000_000
    ///                      +  2_000_000_000
    ///                      =  4_500_000_000
    ///
    /// health_factor        = 165_000_000_000_000 / 4_500_000_000
    ///                      = 36_666                 (integer floor)
    /// ```
    ///
    /// `36_666 > HEALTH_FACTOR_SCALE (10_000)` → position is healthy ✓
    #[test]
    fn test_two_collateral_two_debt_health_factor() {
        let (_env, client, _id, _admin, user, asset_a, asset_b) = setup_two_asset();

        // ── Deposit collateral ──────────────────────────────────────────────
        // Asset A: 2 000 units @ $0.50, LT = 75%
        client.deposit_collateral_asset(&user, &asset_a, &2_000i128);
        // Asset B: 1 000 units @ $1.00, LT = 90%
        client.deposit_collateral_asset(&user, &asset_b, &1_000i128);

        // ── Borrow debt ─────────────────────────────────────────────────────
        // Asset A: 500 units (same-timestamp borrow → effective_debt == principal)
        client.borrow_asset(&user, &asset_a, &500i128);
        // Asset B: 200 units
        client.borrow_asset(&user, &asset_b, &200i128);

        // ── Verify health factor ────────────────────────────────────────────
        // Expected: floor(165_000_000_000_000 / 4_500_000_000) = 36_666
        let hf = client.get_cross_health_factor(&user);
        assert_eq!(
            hf, 36_666,
            "Two-collateral two-debt HF should be 36_666 \
             (= 165_000_000_000_000 / 4_500_000_000, floor)"
        );
    }

    // ── §3 No-Debt Saturated Value ────────────────────────────────────────────

    /// When a user has no outstanding debt the sentinel value
    /// `HEALTH_FACTOR_NO_DEBT = 100_000_000` is returned.
    ///
    /// This applies to:
    /// - Fast path: `debt_assets.is_empty()`.
    /// - Late path: all debts accrued to zero (`total_debt_value == 0`).
    #[test]
    fn test_no_debt_saturated_sentinel() {
        let (_env, client, _id, _admin, user, asset_a, asset_b) = setup_two_asset();

        // Deposit collateral but borrow nothing.
        client.deposit_collateral_asset(&user, &asset_a, &1_000i128);
        client.deposit_collateral_asset(&user, &asset_b, &500i128);

        let hf = client.get_cross_health_factor(&user);
        assert_eq!(
            hf,
            cross_asset::HEALTH_FACTOR_NO_DEBT,
            "No-debt position must return HEALTH_FACTOR_NO_DEBT sentinel (100_000_000)"
        );
    }

    /// A completely empty position (no collateral, no debt) also returns the
    /// `HEALTH_FACTOR_NO_DEBT` sentinel because the debt list is empty.
    #[test]
    fn test_empty_position_returns_sentinel() {
        let (_env, client, _id, _admin, user, _asset_a, _asset_b) = setup_two_asset();

        let hf = client.get_cross_health_factor(&user);
        assert_eq!(
            hf,
            cross_asset::HEALTH_FACTOR_NO_DEBT,
            "Empty position must return HEALTH_FACTOR_NO_DEBT sentinel"
        );
    }

    // ── §6.1 Single Collateral, Single Debt (boundary) ───────────────────────

    /// At exactly the liquidation boundary the health factor equals
    /// `HEALTH_FACTOR_SCALE = 10_000`.
    ///
    /// # Setup
    ///
    /// ```text
    /// Collateral B: 1 000 units, price = 10_000_000 ($1.00), LT = 9_000 bps (90%)
    /// Debt       B:   900 units, price = 10_000_000 ($1.00)
    ///
    /// weighted_collateral = 1_000 × 10_000_000 × 9_000 = 90_000_000_000_000
    /// total_debt_value    =   900 × 10_000_000           =  9_000_000_000
    ///
    /// health_factor = 90_000_000_000_000 / 9_000_000_000 = 10_000  (exact)
    /// ```
    #[test]
    fn test_single_collateral_single_debt_boundary() {
        let (_env, client, _id, _admin, user, _asset_a, asset_b) = setup_two_asset();

        // Deposit 1 000 USDC (asset_b, LT = 90%)
        client.deposit_collateral_asset(&user, &asset_b, &1_000i128);
        // Borrow 900 USDC — exactly at the 90% boundary
        client.borrow_asset(&user, &asset_b, &900i128);

        let hf = client.get_cross_health_factor(&user);
        assert_eq!(
            hf,
            cross_asset::HEALTH_FACTOR_SCALE,
            "At the liquidation boundary HF must equal HEALTH_FACTOR_SCALE (10_000)"
        );
    }

    // ── §4 + §6.3 Floor Rounding Direction ───────────────────────────────────

    /// Division in the health factor path truncates toward zero (floor for
    /// positive values).  Adding a single unit of debt beyond the §5 example
    /// must keep the result at 36_666 rather than rounding up to 36_667.
    ///
    /// # Arithmetic
    ///
    /// ```text
    /// weighted_collateral = 165_000_000_000_000          (same as §5)
    /// total_debt_value    =   4_500_000_001              (+1 extra unit)
    ///
    /// exact    = 165_000_000_000_000 / 4_500_000_001 ≈ 36_666.999…
    /// floor    = 36_666                                (not 36_667)
    /// ```
    ///
    /// This test borrows 1 extra unit of asset A on top of the §5 amounts
    /// (500 + 1 = 501 units of asset A debt), which adds
    /// `1 × 5_000_000 = 5_000_000` to `total_debt_value`:
    ///
    /// ```text
    /// total_debt_value = 2_505_000_000 + 2_000_000_000 = 4_505_000_000
    ///
    /// exact    = 165_000_000_000_000 / 4_505_000_000 ≈ 36_625.970…
    /// floor    = 36_625
    /// ```
    ///
    /// The floor result (36_625) must be strictly less than what a ceiling
    /// division would produce (36_626), demonstrating that the protocol always
    /// rounds in its own favour (lower HF → more conservative).
    #[test]
    fn test_floor_rounding_direction() {
        let (_env, client, _id, _admin, user, asset_a, asset_b) = setup_two_asset();

        // Same collateral as §5 worked example
        client.deposit_collateral_asset(&user, &asset_a, &2_000i128);
        client.deposit_collateral_asset(&user, &asset_b, &1_000i128);

        // Borrow 501 units of asset_a (+1 vs §5 to introduce a non-integer quotient)
        client.borrow_asset(&user, &asset_a, &501i128);
        client.borrow_asset(&user, &asset_b, &200i128);

        // weighted_collateral = 165_000_000_000_000
        // total_debt_value    = 501×5_000_000 + 200×10_000_000
        //                     = 2_505_000_000 + 2_000_000_000 = 4_505_000_000
        // exact               = 36_625.970…
        // floor               = 36_625
        let hf = client.get_cross_health_factor(&user);
        assert_eq!(
            hf, 36_625,
            "HF must use floor division; expected 36_625 not 36_626"
        );
        // Ceiling would yield 36_626 — floor must be strictly less.
        assert!(
            hf < 36_626,
            "Floor division should produce a value < ceiling; got {hf}"
        );
    }

    // ── §9 Consistency: HF vs USD view functions ──────────────────────────────

    /// Verify that `get_cross_health_factor` and `get_cross_position_summary`
    /// report the same health factor value for the §5 scenario, and that
    /// `get_cross_position_value` and `get_cross_debt_value` match the
    /// human-readable USD totals derived in spec §9.
    ///
    /// USD totals (after dividing by PRICE_DIVISOR = 10_000_000):
    ///
    /// ```text
    /// total_collateral_usd = 2_000×5_000_000/10_000_000 + 1_000×10_000_000/10_000_000
    ///                      = 1_000 + 1_000 = 2_000
    ///
    /// total_debt_usd       = 500×5_000_000/10_000_000 + 200×10_000_000/10_000_000
    ///                      = 250 + 200 = 450
    /// ```
    #[test]
    fn test_usd_view_functions_match_spec() {
        let (_env, client, _id, _admin, user, asset_a, asset_b) = setup_two_asset();

        client.deposit_collateral_asset(&user, &asset_a, &2_000i128);
        client.deposit_collateral_asset(&user, &asset_b, &1_000i128);
        client.borrow_asset(&user, &asset_a, &500i128);
        client.borrow_asset(&user, &asset_b, &200i128);

        // Direct health factor must equal summary health factor.
        let hf_direct = client.get_cross_health_factor(&user);
        let summary = client.get_cross_position_summary(&user);
        assert_eq!(
            hf_direct, summary.health_factor,
            "get_cross_health_factor and get_cross_position_summary must agree"
        );
        assert_eq!(hf_direct, 36_666, "Expected HF = 36_666 (see spec §5)");

        // USD-denominated totals (PRICE_DIVISOR applied in view functions).
        assert_eq!(
            summary.total_collateral_usd, 2_000,
            "total_collateral_usd should be 2_000 (= 1_000 XLM + 1_000 USDC)"
        );
        assert_eq!(
            summary.total_debt_usd, 450,
            "total_debt_usd should be 450 (= 250 XLM-debt + 200 USDC-debt)"
        );
    }
}
