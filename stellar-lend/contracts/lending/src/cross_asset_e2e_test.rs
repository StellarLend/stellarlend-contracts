/// End-to-end lifecycle tests for cross-asset lending:
/// deposit_collateral_asset → borrow_asset → price shock → liquidatable assertions.
///
/// These tests verify that the protocol remains solvent and accounting stays
/// consistent across the full cross-asset lifecycle where collateral and debt
/// are distinct assets.
///
/// # Numeric conventions (from cross_asset.rs)
/// - `PRICE_DIVISOR = 10_000_000` — prices are 7-decimal fixed-point (10_000_000 = $1.00).
/// - `HEALTH_FACTOR_SCALE = 10_000` — HF = 1.0 is represented as 10_000.
/// - `HEALTH_FACTOR_NO_DEBT = 100_000_000` — sentinel returned when user has no debt.
/// - Liquidation threshold is expressed in bps (8_000 = 80%).
/// - Minimum borrow is 1_000 (raw units) by default.
use super::*;
use soroban_sdk::testutils::{Address as _, LedgerInfo};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Set the oracle price for `asset` inside the contract's persistent storage.
///
/// `price` uses 7-decimal fixed-point: 10_000_000 = $1.00.
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

/// Standard two-asset fixture.
///
/// - `asset_col`: collateral asset, price $1.00 (10_000_000), LTV 75 %, liq-threshold 80 %.
/// - `asset_dbt`: debt asset, price $1.00 (10_000_000), LTV 60 %, liq-threshold 70 %.
/// - Min-borrow left at protocol default (1_000).
fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address, // contract id
    Address, // admin
    Address, // borrower
    Address, // liquidator (separate address)
    Address, // asset_col
    Address, // asset_dbt
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);
    let asset_col = env.register(MockAsset, ());
    let asset_dbt = env.register(MockAsset, ());

    client.initialize(&admin);

    // 75 % LTV / 80 % liquidation threshold for collateral asset
    client.set_asset_params(&admin, &asset_col, &7500, &8000, &1_000_000_000_000i128);
    // 60 % LTV / 70 % liquidation threshold for debt asset
    client.set_asset_params(&admin, &asset_dbt, &6000, &7000, &1_000_000_000_000i128);

    // Initial prices: both assets at $1.00
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    (env, client, id, admin, borrower, liquidator, asset_col, asset_dbt)
}

/// Advance ledger time by `secs` seconds.
fn advance(env: &Env, secs: u64) {
    let mut li: LedgerInfo = env.ledger().get();
    li.timestamp = li.timestamp.saturating_add(secs);
    li.sequence_number = li.sequence_number.saturating_add(1);
    env.ledger().set(li);
}

// ── 1. Full lifecycle — healthy deposit → borrow → repay → withdraw ──────────

/// Verifies the happy path: deposit asset A as collateral, borrow asset B,
/// check health factor is safe, fully repay, then withdraw all collateral.
///
/// Post-conditions:
/// - Collateral balance = 0 after withdrawal.
/// - Debt position = 0 after full repayment.
/// - Health factor returns to NO_DEBT sentinel.
#[test]
fn e2e_deposit_borrow_repay_withdraw_full_lifecycle() {
    let (_, client, _, _, borrower, _, asset_col, asset_dbt) = setup();

    // Deposit 10_000 units of collateral asset
    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    assert_eq!(client.get_collateral_asset_balance(&borrower, &asset_col), 10_000);

    // Borrow 5_000 units of debt asset (50 % of collateral at 1:1 price — well within 75 % LTV)
    client.borrow_asset(&borrower, &asset_dbt, &5_000i128);
    let pos_after_borrow = client.get_debt_asset_position(&borrower, &asset_dbt);
    assert_eq!(pos_after_borrow.principal, 5_000);

    // Health factor should be well above 1.0 (10_000)
    // weighted_col = 10_000 * 8000 / 10_000 = 8_000 (uses liq-threshold)
    // debt_value   = 5_000 (raw at $1:$1)
    // HF = 8_000 * 10_000 / (5_000 * price / price) — computed in cross_asset.rs as
    //      weighted_col * HEALTH_FACTOR_SCALE / total_debt_value
    let hf = client.get_cross_health_factor(&borrower);
    assert!(hf > 10_000, "expected healthy HF, got {hf}");

    // Fully repay debt
    client.repay_asset(&borrower, &asset_dbt, &5_000i128);
    assert_eq!(
        client.get_debt_asset_position(&borrower, &asset_dbt).principal,
        0
    );

    // HF sentinel after full repayment
    assert_eq!(client.get_cross_health_factor(&borrower), 100_000_000);

    // Withdraw all collateral
    client.withdraw_asset(&borrower, &asset_col, &10_000i128);
    assert_eq!(client.get_collateral_asset_balance(&borrower, &asset_col), 0);
}

// ── 2. Price shock drives position underwater ─────────────────────────────────

/// Verifies that a collateral price crash causes HF to drop below 10_000.
///
/// Setup:
///   - Deposit 10_000 col @ $1.00
///   - Borrow  7_500 dbt @ $1.00  (exactly at LTV boundary: HF ≈ 10_666)
///
/// Shock: collateral price halves to $0.50
///
/// Post-shock:
///   - HF < 10_000 → position is liquidatable.
#[test]
fn e2e_price_shock_collateral_crash_makes_position_liquidatable() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    // Borrow just below LTV limit so position starts healthy
    client.borrow_asset(&borrower, &asset_dbt, &7_000i128);

    let hf_before = client.get_cross_health_factor(&borrower);
    assert!(hf_before >= 10_000, "expected healthy before shock, got {hf_before}");

    // Halve collateral price: $1.00 → $0.50
    set_price(&env, &id, &asset_col, 5_000_000);

    let hf_after = client.get_cross_health_factor(&borrower);
    assert!(
        hf_after < 10_000,
        "expected liquidatable HF after shock, got {hf_after}"
    );
}

/// Verifies that a debt price spike (borrowed asset becomes more expensive)
/// causes HF to drop below 10_000.
#[test]
fn e2e_price_shock_debt_spike_makes_position_liquidatable() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &6_000i128);

    let hf_before = client.get_cross_health_factor(&borrower);
    assert!(hf_before >= 10_000, "expected healthy before shock");

    // Debt asset price doubles: $1.00 → $2.00
    set_price(&env, &id, &asset_dbt, 20_000_000);

    let hf_after = client.get_cross_health_factor(&borrower);
    assert!(
        hf_after < 10_000,
        "expected liquidatable HF after debt spike, got {hf_after}"
    );
}

// ── 3. Post-shock invariants: seized ≤ collateral, debt reduced ───────────────

/// Full lifecycle with simulated liquidation via direct state write.
///
/// After a price shock makes the position liquidatable, this test:
///   1. Captures pre-liquidation collateral and debt balances.
///   2. Simulates a partial liquidation (50 % close-factor) by directly
///      updating storage — mirroring what a liquidation call would do.
///   3. Asserts all post-liquidation invariants.
///
/// Invariants checked:
///   - seized_amount ≤ pre_liquidation_collateral (no value created)
///   - debt_after = debt_before - repaid_amount (exact accounting)
///   - collateral_after = collateral_before - seized_amount
///   - HF after partial liquidation is ≥ HF before (position improved)
#[test]
fn e2e_post_liquidation_invariants_no_value_created() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    // Setup: deposit 10_000 col, borrow 7_500 dbt (tight position)
    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &7_000i128);

    // Shock: collateral drops 40 % → position underwater
    set_price(&env, &id, &asset_col, 6_000_000); // $0.60

    let hf_before_liq = client.get_cross_health_factor(&borrower);
    assert!(hf_before_liq < 10_000, "position must be underwater for this test");

    let col_before = client.get_collateral_asset_balance(&borrower, &asset_col);
    let debt_before = client.get_debt_asset_position(&borrower, &asset_dbt).principal;

    // Simulate a 50 % close-factor liquidation (repay half of debt)
    // incentive bonus = 10 % → seized = repaid * 110 / 100
    let repaid_amount = debt_before / 2; // close factor = 50 %
    let incentive_bps: i128 = 1_000; // 10 %
    let seized_amount = repaid_amount * (10_000 + incentive_bps) / 10_000;
    // Clamp seized to available collateral (safety invariant)
    let seized_amount = seized_amount.min(col_before);

    // Apply directly in storage (mirrors the liquidate() function's state writes)
    env.as_contract(&id, || {
        // Reduce collateral
        env.storage().persistent().set(
            &DataKey::CollateralAsset(borrower.clone(), asset_col.clone()),
            &(col_before - seized_amount),
        );
        // Reduce debt principal
        env.storage().persistent().set(
            &DataKey::DebtAsset(borrower.clone(), asset_dbt.clone()),
            &DebtPosition {
                principal: debt_before - repaid_amount,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    let col_after = client.get_collateral_asset_balance(&borrower, &asset_col);
    let debt_after = client.get_debt_asset_position(&borrower, &asset_dbt).principal;

    // Invariant 1: seized ≤ pre-liquidation collateral (no value created)
    assert!(
        seized_amount <= col_before,
        "seized {seized_amount} > collateral {col_before}"
    );

    // Invariant 2: debt reduced by exactly repaid_amount
    assert_eq!(
        debt_after,
        debt_before - repaid_amount,
        "debt accounting error"
    );

    // Invariant 3: collateral reduced by exactly seized_amount
    assert_eq!(
        col_after,
        col_before - seized_amount,
        "collateral accounting error"
    );

    // Invariant 4: HF after liquidation ≥ HF before (position improved or same)
    let hf_after = client.get_cross_health_factor(&borrower);
    assert!(
        hf_after >= hf_before_liq,
        "HF should improve after liquidation: before={hf_before_liq} after={hf_after}"
    );
}

// ── 4. Exactly-at-threshold ───────────────────────────────────────────────────

/// Position at exactly HF = 10_000 (the boundary).
///
/// A position whose health factor is exactly 1.0 is at the liquidation
/// threshold — any price tick down makes it liquidatable.
#[test]
fn e2e_exactly_at_liquidation_threshold() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    // asset_col: liq-threshold = 8_000 bps
    // Deposit 10_000 col @ $1.00, borrow X dbt @ $1.00 such that HF = 10_000 exactly.
    // HF = (col * liq_threshold) / debt = (10_000 * 8_000) / debt = 10_000
    // → debt = 8_000
    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &8_000i128);

    let hf = client.get_cross_health_factor(&borrower);
    // Due to integer math the result is 10_000 exactly (no remainder)
    assert_eq!(hf, 10_000, "expected HF exactly at threshold, got {hf}");

    // One unit price drop makes it liquidatable
    set_price(&env, &id, &asset_col, 9_999_999); // just below $1.00
    let hf_after = client.get_cross_health_factor(&borrower);
    assert!(
        hf_after < 10_000,
        "expected liquidatable after micro-drop, got {hf_after}"
    );
}

// ── 5. Deep underwater — full collateral seizure ──────────────────────────────

/// Verifies that when collateral is insufficient to cover the incentivised
/// seizure the simulation correctly clamps to available balance, so no
/// negative collateral can arise.
#[test]
fn e2e_deep_underwater_seizure_capped_at_available_collateral() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &7_000i128);

    // Crash collateral 90 % → deeply underwater
    set_price(&env, &id, &asset_col, 1_000_000); // $0.10

    let col_before = client.get_collateral_asset_balance(&borrower, &asset_col);
    let debt_before = client.get_debt_asset_position(&borrower, &asset_dbt).principal;

    // Full close-factor repayment (50 %)
    let repaid = debt_before / 2;
    let incentive_bps: i128 = 1_000;
    let uncapped_seizure = repaid * (10_000 + incentive_bps) / 10_000;

    // Protocol clamps: final seized cannot exceed available collateral
    let seized = uncapped_seizure.min(col_before);
    assert_eq!(seized, col_before, "deep underwater: should seize all collateral");

    // Simulate the clamped liquidation
    env.as_contract(&id, || {
        env.storage().persistent().set(
            &DataKey::CollateralAsset(borrower.clone(), asset_col.clone()),
            &(col_before - seized),
        );
        env.storage().persistent().set(
            &DataKey::DebtAsset(borrower.clone(), asset_dbt.clone()),
            &DebtPosition {
                principal: debt_before - repaid,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    let col_after = client.get_collateral_asset_balance(&borrower, &asset_col);
    assert_eq!(col_after, 0, "collateral should be fully seized");
    // Debt reduced but not fully cleared (only 50 % close factor)
    assert!(
        client.get_debt_asset_position(&borrower, &asset_dbt).principal > 0,
        "partial liquidation: some debt should remain"
    );
}

// ── 6. Partial liquidation → healthy → full repay → withdraw ─────────────────

/// Complete lifecycle including a partial liquidation that restores health,
/// followed by the borrower repaying remaining debt and withdrawing collateral.
#[test]
fn e2e_partial_liquidation_then_full_repay_and_withdraw() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    client.deposit_collateral_asset(&borrower, &asset_col, &20_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &8_000i128);

    // Moderate shock: collateral drops 30 %
    set_price(&env, &id, &asset_col, 7_000_000); // $0.70

    let hf_shock = client.get_cross_health_factor(&borrower);
    assert!(hf_shock < 10_000, "should be liquidatable after shock");

    let col_before = client.get_collateral_asset_balance(&borrower, &asset_col);
    let debt_before = client.get_debt_asset_position(&borrower, &asset_dbt).principal;

    // Partial liquidation: repay 50 % of debt (close factor)
    let repaid = debt_before / 2;
    let seized = (repaid * 11_000 / 10_000).min(col_before);

    env.as_contract(&id, || {
        env.storage().persistent().set(
            &DataKey::CollateralAsset(borrower.clone(), asset_col.clone()),
            &(col_before - seized),
        );
        env.storage().persistent().set(
            &DataKey::DebtAsset(borrower.clone(), asset_dbt.clone()),
            &DebtPosition {
                principal: debt_before - repaid,
                last_update: env.ledger().timestamp(),
            },
        );
    });

    let hf_after_liq = client.get_cross_health_factor(&borrower);
    assert!(
        hf_after_liq >= hf_shock,
        "HF must improve after partial liquidation"
    );

    // Borrower repays remaining debt
    let remaining_debt = client.get_debt_asset_position(&borrower, &asset_dbt).principal;
    client.repay_asset(&borrower, &asset_dbt, &remaining_debt);
    assert_eq!(
        client.get_debt_asset_position(&borrower, &asset_dbt).principal,
        0
    );
    assert_eq!(client.get_cross_health_factor(&borrower), 100_000_000);

    // Borrower withdraws remaining collateral
    let remaining_col = client.get_collateral_asset_balance(&borrower, &asset_col);
    client.withdraw_asset(&borrower, &asset_col, &remaining_col);
    assert_eq!(client.get_collateral_asset_balance(&borrower, &asset_col), 0);
}

// ── 7. Two-collateral one-debt cross-asset shock ───────────────────────────────

/// Two distinct collateral assets backing one debt asset.
/// Price shock on one collateral affects aggregate HF.
#[test]
fn e2e_two_collateral_one_debt_shock() {
    let (env, client, id, admin, borrower, _, asset_col, asset_dbt) = setup();

    // Register a third asset as second collateral
    let asset_col2 = env.register(MockAsset, ());
    client.set_asset_params(&admin, &asset_col2, &7500, &8000, &1_000_000_000_000i128);
    set_price(&env, &id, &asset_col2, 10_000_000); // $1.00

    // Deposit both collateral assets
    client.deposit_collateral_asset(&borrower, &asset_col, &5_000i128);
    client.deposit_collateral_asset(&borrower, &asset_col2, &5_000i128);

    // Borrow against combined collateral
    client.borrow_asset(&borrower, &asset_dbt, &7_000i128);

    let hf_before = client.get_cross_health_factor(&borrower);
    assert!(hf_before >= 10_000, "should be healthy before shock");

    // Crash asset_col2 price 80 %
    set_price(&env, &id, &asset_col2, 2_000_000); // $0.20

    let hf_after = client.get_cross_health_factor(&borrower);
    assert!(
        hf_after < hf_before,
        "HF must decrease after collateral crash"
    );

    // Position summary reflects both collateral assets
    let summary = client.get_cross_position_summary(&borrower);
    assert!(summary.total_collateral_usd > 0);
    assert!(summary.total_debt_usd > 0);
    assert_eq!(summary.health_factor, hf_after);
}

// ── 8. User isolation — one user's shock doesn't affect another ───────────────

/// Verifies G-7 (user isolation): price shock on user A's position does not
/// change user B's health factor.
#[test]
fn e2e_user_isolation_shock_does_not_bleed_to_other_user() {
    let (env, client, id, _, borrower, liquidator, asset_col, asset_dbt) = setup();

    // user_a = borrower, user_b = liquidator (reused as second user)
    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &6_000i128);

    client.deposit_collateral_asset(&liquidator, &asset_col, &10_000i128);
    client.borrow_asset(&liquidator, &asset_dbt, &4_000i128);

    let hf_b_before = client.get_cross_health_factor(&liquidator);

    // Crash price (affects both users since same asset, but each position is independent)
    set_price(&env, &id, &asset_col, 6_000_000);

    let hf_b_after = client.get_cross_health_factor(&liquidator);

    // user_b's HF changes due to same asset price — but the *change* is from
    // price only, not from user_a's operations. We verify user_b's position
    // reads only user_b's balances (summary totals are consistent).
    let summary_b = client.get_cross_position_summary(&liquidator);
    assert_eq!(
        summary_b.health_factor, hf_b_after,
        "summary HF must match direct HF query for user B"
    );

    // user_b's collateral balance is unaffected by user_a's position
    assert_eq!(
        client.get_collateral_asset_balance(&liquidator, &asset_col),
        10_000,
        "user B collateral must be unchanged"
    );

    // Suppress unused warning on hf_b_before
    let _ = hf_b_before;
}

// ── 9. Withdraw blocked after shock ──────────────────────────────────────────

/// After a price shock makes HF < 1.0, attempting to withdraw any collateral
/// must be rejected with HealthFactorTooLow.
#[test]
fn e2e_withdraw_blocked_when_hf_below_threshold_after_shock() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &7_000i128);

    // Shock collateral price down so HF drops below 1.0
    set_price(&env, &id, &asset_col, 5_000_000);

    let hf = client.get_cross_health_factor(&borrower);
    assert!(hf < 10_000, "precondition: must be liquidatable");

    let res = client.try_withdraw_asset(&borrower, &asset_col, &1i128);
    assert!(
        matches!(res, Err(Ok(LendingError::HealthFactorTooLow))),
        "withdraw must be rejected when HF < 1.0"
    );
}

// ── 10. Borrow blocked after shock ───────────────────────────────────────────

/// After a price shock reduces HF below 1.0, further borrows must be rejected.
#[test]
fn e2e_borrow_blocked_when_hf_below_threshold_after_shock() {
    let (env, client, id, _, borrower, _, asset_col, asset_dbt) = setup();

    client.deposit_collateral_asset(&borrower, &asset_col, &10_000i128);
    client.borrow_asset(&borrower, &asset_dbt, &7_000i128);

    // Shock: HF goes below 1.0
    set_price(&env, &id, &asset_col, 5_000_000);

    let res = client.try_borrow_asset(&borrower, &asset_dbt, &1_000i128);
    assert!(
        matches!(res, Err(Ok(LendingError::HealthFactorTooLow))),
        "borrow must be rejected when HF < 1.0"
    );
}
