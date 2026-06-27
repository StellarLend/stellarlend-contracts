//! Tests that every division in the liquidation path rounds in the protocol's
//! favour (floor / truncate-toward-zero).  Sub-unit remainders must always
//! benefit the protocol / remaining borrowers, never the liquidator.
//!
//! # Rounding audit
//!
//! | Division                     | Direction | Protocol-favoured? |
//! |------------------------------|-----------|--------------------|
//! | `hf = col × 8000 ÷ debt`     | floor     | Yes (lower HF → more liquidatable) |
//! | `max_repay = debt × 5000 ÷ 10000` | floor | Yes (smaller cap) |
//! | `seized = repay × 11000 ÷ 10000`  | floor | Yes (less collateral to liquidator) |
//!
//! Every test probes sub-unit boundaries where truncation matters.

use super::*;
use soroban_sdk::testutils::Address as _;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin, user, debt_asset, collateral_asset)
}

/// Create a position with `collateral` deposited and `debt` borrowed such that
/// HF < 1.0 (liquidatable).  Uses `mock_all_auths`.
fn make_unhealthy_position(
    env: &Env,
    client: &LendingContractClient<'static>,
    user: &Address,
    collateral: i128,
    debt: i128,
) {
    client.deposit(user, &collateral);
    client.borrow(user, &debt);
}

// ---------------------------------------------------------------------------
// 1-unit repay boundary
// ---------------------------------------------------------------------------

/// Repaying 1 unit of debt when the bonus formula produces a fraction:
/// seized = 1 × 11000 / 10000 = 1 (floor).  The 0.1 remainder stays with the
/// borrower / protocol — the liquidator must NOT receive 2.
#[test]
fn one_unit_repay_seizes_one_collateral() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // Collateral = 2, debt = 3 → HF = 2 * 8000 / 3 = 5333 < 10000
    make_unhealthy_position(&env, &client, &user, 2, 3);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
    assert!(result.is_ok(), "liquidation of 1 unit should succeed");

    let pos = client.get_position(&user);
    // debt 3 → max_repay = 3 * 5000 / 10000 = 1 → actual_repay = 1
    // seized = 1 * 11000 / 10000 = 1 (floor)
    // new_debt = 3 - 1 = 2
    // new_col = 2 - 1 = 1
    assert_eq!(pos.debt, 2, "debt should decrease by 1 (floor of 1/1)");
    assert_eq!(
        pos.collateral, 1,
        "collateral should decrease by 1, not by 2 (floor protects protocol)"
    );
}

/// Exact same scenario verifying the returned `actual_repay` is 1.
#[test]
fn one_unit_liquidate_returns_actual_repay() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    make_unhealthy_position(&env, &client, &user, 2, 3);

    let liquidator = Address::generate(&env);
    let repay = client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
    assert_eq!(repay, 1, "liquidate must return actual_repay = 1");
}

// ---------------------------------------------------------------------------
// Fractional seizure boundary (where truncation matters most)
// ---------------------------------------------------------------------------

/// When seized_collateral would be fractional, the floor must give the smaller
/// amount to the liquidator.  This test pins the exact boundary.
#[test]
fn fractional_seizure_rounds_down_for_liquidator() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // Collateral = 2, debt = 9 → HF = 2 * 8000 / 9 = 1777 < 10000
    // max_repay = 9 * 5000 / 10000 = 4
    make_unhealthy_position(&env, &client, &user, 2, 9);

    // Liquidate with amount = 2
    // actual_repay = min(2, 4) = 2
    // seized = 2 * 11000 / 10000 = 22000 / 10000 = 2 (floor, exact would be 2.2)
    // final_seized = min(2, 2) = 2
    // new_debt = 9 - 2 = 7
    // new_col = 2 - 2 = 0
    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &2);
    assert!(
        result.is_ok(),
        "fractional-seizure liquidation should succeed"
    );

    let pos = client.get_position(&user);
    assert_eq!(pos.debt, 7, "debt 9 - 2 = 7");
    assert_eq!(pos.collateral, 0, "collateral 2 - 2 = 0 (floor saved 0.2)");
}

/// Repaying 1 unit when seized would be 1.1 → liquidator gets 1, not 2.
#[test]
fn fractional_seizure_at_one_unit_repay() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // Collateral = 3, debt = 3 → HF = 3 * 8000 / 3 = 8000 < 10000
    // max_repay = 3 * 5000 / 10000 = 1
    make_unhealthy_position(&env, &client, &user, 3, 3);

    let liquidator = Address::generate(&env);
    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
    // seized = 1 * 11000 / 10000 = 1 (floor of 1.1)
    // new_col = 3 - 1 = 2
    let pos = client.get_position(&user);
    assert_eq!(
        pos.collateral, 2,
        "floor: liquidator gets 1 collateral, not 2"
    );
}

// ---------------------------------------------------------------------------
// Close factor division: max_repay must truncate toward zero.
// ---------------------------------------------------------------------------

/// When debt is 1, max_repay = 1 * 5000 / 10000 = 0 (floor).
/// Liquidator cannot repay debt of 1 because the close-factor cap rounds to 0.
#[test]
fn close_factor_floor_at_one_unit_debt() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // Collateral = 1, debt = 1 → HF = 1 * 8000 / 1 = 8000 < 10000
    make_unhealthy_position(&env, &client, &user, 1, 1);

    let liquidator = Address::generate(&env);
    // amount = 1, max_repay = 0 → actual_repay = 0 → dust guard kicks in
    let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
    assert!(
        result.is_err(),
        "liquidating 1 unit of 1 debt should fail: max_repay = 0"
    );
}

/// When debt = 2, max_repay = 2 * 5000 / 10000 = 1 (floor of 1.0 = exact).
#[test]
fn close_factor_exact_at_two_units_debt() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // HF = 2 * 8000 / 2 = 8000 < 10000
    make_unhealthy_position(&env, &client, &user, 2, 2);

    let liquidator = Address::generate(&env);
    let repay = client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &2);
    // max_repay = 2 * 5000 / 10000 = 1
    assert_eq!(repay, 1, "max_repay caps at 1 for debt=2");
}

/// When debt = 3, max_repay = 3 * 5000 / 10000 = 1 (floor of 1.5).
#[test]
fn close_factor_floor_at_three_units_debt() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // HF = 2 * 8000 / 3 = 5333 < 10000
    make_unhealthy_position(&env, &client, &user, 2, 3);

    let liquidator = Address::generate(&env);
    let repay = client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &2);
    // max_repay = 3 * 5000 / 10000 = 1
    assert_eq!(repay, 1, "max_repay = 1 (floor of 1.5)");
}

// ---------------------------------------------------------------------------
// Health-factor division: must floor so the position looks more underwater.
// ---------------------------------------------------------------------------

/// HF = 1 * 8000 / 1 = 8000 — exact (no rounding needed).
/// Position is liquidatable.
#[test]
fn health_factor_exact_at_boundary() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    make_unhealthy_position(&env, &client, &user, 1, 1);
    let hf = client.get_health_factor(&user);
    assert_eq!(hf, 8000, "HF = 8000 (exact)");
    // Should be liquidatable since 8000 < 10000
    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
    assert!(
        result.is_err(),
        "liquidation of debt=1 should fail: max_repay=0"
    );
}

/// When collateral = 2, debt = 3: HF = 2 * 8000 / 3 = 16000/3 = 5333 (floor).
/// If we had ceil, it would be 5334.  Floor is more conservative (lower HF).
#[test]
fn health_factor_floor_is_conservative() {
    let (_env, client, _admin, user, _debt_asset, _collateral_asset) = setup();
    make_unhealthy_position(&_env, &client, &user, 2, 3);
    let hf = client.get_health_factor(&user);
    // 2 * 8000 / 3 = 5333.333... → floor = 5333
    assert_eq!(hf, 5333, "HF floors at 5333, not 5334");
}

// ---------------------------------------------------------------------------
// Clamp boundary: seized > collateral.
// ---------------------------------------------------------------------------

/// When seized_collateral exceeds actual collateral, the clamp must cap at
/// collateral.  The clamp itself is not a division but interacts with the
/// floor rounding of seized_collateral.
#[test]
fn clamp_caps_seized_at_available_collateral() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // Collateral = 2, debt = 10 → HF = 2 * 8000 / 10 = 1600 < 10000
    // max_repay = 10 * 5000 / 10000 = 5
    make_unhealthy_position(&env, &client, &user, 2, 10);

    let liquidator = Address::generate(&env);
    // amount = 5 → actual_repay = min(5, 5) = 5
    // seized = 5 * 11000 / 10000 = 5 (floor of 5.5)
    // final_seized = min(5, 2) = 2 (clamp)
    // new_debt = 10 - 5 = 5
    // new_col = 2 - 2 = 0
    let repay = client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &5);
    assert_eq!(repay, 5, "liquidator repays 5");

    let pos = client.get_position(&user);
    assert_eq!(pos.collateral, 0, "all collateral seized (clamp at 2)");
    assert_eq!(pos.debt, 5, "debt reduced by 5");
}

/// Clamping when seized_collateral exactly equals collateral (no truncation).
#[test]
fn clamp_exact_match() {
    let (_env, client, _admin, user, debt_asset, collateral_asset) = setup();
    make_unhealthy_position(&_env, &client, &user, 2, 10);
    let liquidator = Address::generate(&_env);
    client.liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &5);
    let pos = client.get_position(&user);
    assert_eq!(pos.collateral, 0, "clamp at 2 released all collateral");
}

// ---------------------------------------------------------------------------
// Repeated dust liquidations: every rounding must favour protocol.
// ---------------------------------------------------------------------------

/// The protocol should remain solvent after many dust liquidations on the
/// same position.  Each liquidation floors its divisions, so the cumulative
/// rounding error should favour the protocol (not the liquidator).
#[test]
fn repeated_dust_liquidations_dont_leak_value() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    // Position: collateral = 10, debt = 10 → HF = 8000 < 10000
    make_unhealthy_position(&env, &client, &user, 10, 10);

    let liquidator = Address::generate(&env);

    for i in 0..8 {
        let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
        assert!(result.is_ok(), "dust liquidation {} should succeed", i + 1);
    }

    let pos = client.get_position(&user);
    assert_eq!(pos.debt, 2, "debt after 8 dust liquidations: 10 - 8 = 2");
    assert_eq!(
        pos.collateral, 2,
        "collateral after 8 dust liquidations: 10 - 8 = 2"
    );

    // The 9th liquidation should work (actual_repay = min(1, 1) = 1 > 0)
    let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
    assert!(result.is_ok(), "9th dust liquidation should succeed");

    let pos = client.get_position(&user);
    assert_eq!(pos.debt, 1, "debt after 9 dust liquidations: 2 - 1 = 1");
    assert_eq!(
        pos.collateral, 1,
        "collateral after 9 dust liquidations: 2 - 1 = 1"
    );
}

// ---------------------------------------------------------------------------
// Rounding consistency: floor is idempotent for exact multiples.
// ---------------------------------------------------------------------------

/// When divisions are exact (no remainder), floor and ceil produce the same
/// result.  This pins that the helpers don't introduce spurious rounding.
#[test]
fn exact_division_rounding_is_idempotent() {
    // 100 * 11000 / 10000 = 1100 → exact
    let floor_result = math::checked_mul_div_floor(100, 11000, 10000).unwrap();
    let ceil_result = math::checked_mul_div_ceil(100, 11000, 10000).unwrap();
    assert_eq!(floor_result, 1100, "floor exact");
    assert_eq!(ceil_result, 1100, "ceil exact");
}

// ---------------------------------------------------------------------------
// Dust guard: actual_repay <= 0 must be rejected.
// ---------------------------------------------------------------------------

/// When max_repay is 0 (debt = 1), requesting liquidation must fail with
/// InvalidAmount rather than silently doing nothing.
#[test]
fn dust_guard_rejects_zero_actual_repay() {
    let (env, client, _admin, user, debt_asset, collateral_asset) = setup();
    make_unhealthy_position(&env, &client, &user, 1, 1);

    let liquidator = Address::generate(&env);
    let result = client.try_liquidate(&liquidator, &user, &debt_asset, &collateral_asset, &1);
    assert!(
        result.is_err(),
        "dust guard should reject liquidation when max_repay = 0"
    );
    // Verify the position was not mutated
    let pos = client.get_position(&user);
    assert_eq!(pos.debt, 1, "debt unchanged");
    assert_eq!(pos.collateral, 1, "collateral unchanged");
}

// ---------------------------------------------------------------------------
// checked_mul_div_floor consistency with raw integer division.
// ---------------------------------------------------------------------------

/// For small positive values, checked_mul_div_floor must match `/`.
#[test]
fn floor_matches_raw_division_for_exact_values() {
    for a in [1, 10, 100, 1000, 10000, 100000] {
        for b in [1, 10, 100, 1000] {
            for c in [1, 10, 100, 1000, 10000] {
                let expected = a * b / c;
                let got = math::checked_mul_div_floor(a, b, c).unwrap();
                assert_eq!(
                    got, expected,
                    "checked_mul_div_floor({}, {}, {}) = {}, expected {}",
                    a, b, c, got, expected
                );
            }
        }
    }
}
