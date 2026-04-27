//! Deployment and initialization tests for StellarLend contracts.
//!
//! These tests validate the full deployment lifecycle:
//! - Successful first-time initialization
//! - Rejection of duplicate initialization (init-twice-must-fail)
//! - Correct default parameter values post-init
//! - Admin-only enforcement on privileged operations
//! - Post-initialization operational readiness

use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a clean Soroban test environment with all auths mocked.
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Register the contract and return (contract_id, client).
fn register(e: &Env) -> (Address, HelloContractClient<'_>) {
    let id = e.register(HelloContract, ());
    let client = HelloContractClient::new(e, &id);
    (id, client)
}

/// Register the contract, initialize it with a generated admin, and return the triple.
fn setup(e: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let (id, client) = register(e);
    let admin = Address::generate(e);
    client.initialize(&admin);
    (id, admin, client)
}

// ---------------------------------------------------------------------------
// 1. Successful initialization
// ---------------------------------------------------------------------------

/// Initializing a freshly deployed contract must succeed without panicking.
#[test]
fn test_initialize_succeeds_on_fresh_contract() {
    let e = env();
    let (_id, client) = register(&e);
    let admin = Address::generate(&e);
    // Must not panic.
    client.initialize(&admin);
}

// ---------------------------------------------------------------------------
// 2. Double-initialization is rejected (init-twice-must-fail)
// ---------------------------------------------------------------------------

/// A second call to `initialize` on an already-initialized contract must panic
/// with an `AlreadyInitialized` contract error.
#[test]
#[should_panic]
fn test_initialize_twice_panics() {
    let e = env();
    let (_id, client) = register(&e);
    let admin = Address::generate(&e);

    client.initialize(&admin); // first call – must succeed
    client.initialize(&admin); // second call – must panic
}

/// Calling `initialize` with a *different* admin address the second time must also
/// fail, preventing an admin-takeover via re-initialization.
#[test]
#[should_panic]
fn test_initialize_twice_different_admin_panics() {
    let e = env();
    let (_id, client) = register(&e);
    let admin1 = Address::generate(&e);
    let admin2 = Address::generate(&e);

    client.initialize(&admin1); // first call – must succeed
    client.initialize(&admin2); // attacker tries to replace admin – must panic
}

// ---------------------------------------------------------------------------
// 3. Default risk parameters after initialization
// ---------------------------------------------------------------------------

/// After initialization the default `RiskConfig` must reflect documented values
/// (all in basis points).
#[test]
fn test_default_risk_params_after_init() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    // The Soroban test client auto-unwraps Result<i128, _> → i128 (panics on Err).
    // min_collateral_ratio = 110 % = 11_000 bps
    let mcr = client.get_min_collateral_ratio();
    assert_eq!(
        mcr, 11_000,
        "min_collateral_ratio should be 11000 bps (110%)"
    );

    // liquidation_threshold = 105 % = 10_500 bps
    let lt = client.get_liquidation_threshold();
    assert_eq!(
        lt, 10_500,
        "liquidation_threshold should be 10500 bps (105%)"
    );

    // close_factor = 50 % = 5_000 bps
    let cf = client.get_close_factor();
    assert_eq!(cf, 5_000, "close_factor should be 5000 bps (50%)");

    // liquidation_incentive = 10 % = 1_000 bps
    let li = client.get_liquidation_incentive();
    assert_eq!(li, 1_000, "liquidation_incentive should be 1000 bps (10%)");
}

/// `get_risk_config` must return `Some` immediately after initialization.
#[test]
fn test_risk_config_exists_after_init() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    let config = client.get_risk_config();
    assert!(
        config.is_some(),
        "RiskConfig must exist after initialization"
    );
}

// ---------------------------------------------------------------------------
// 4. Default interest rate parameters after initialization
// ---------------------------------------------------------------------------

/// At launch there are no deposits, so utilization must be 0 bps.
#[test]
fn test_utilization_zero_at_launch() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    let util = client.get_utilization();
    assert_eq!(util, 0, "utilization should be 0 at launch");
}

/// Borrow and supply rates must be accessible and internally consistent
/// immediately after initialization.
#[test]
fn test_interest_rates_accessible_after_init() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    let borrow_rate = client.get_borrow_rate();
    let supply_rate = client.get_supply_rate();

    assert!(borrow_rate >= 0, "borrow_rate must be non-negative");
    assert!(supply_rate >= 0, "supply_rate must be non-negative");
    assert!(
        supply_rate <= borrow_rate,
        "supply_rate must not exceed borrow_rate"
    );
}

// ---------------------------------------------------------------------------
// 5. Emergency pause starts disabled
// ---------------------------------------------------------------------------

/// Immediately after initialization the emergency pause must be inactive.
#[test]
fn test_emergency_pause_disabled_after_init() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    assert!(
        !client.is_emergency_paused(),
        "emergency pause must be OFF after init"
    );
}

/// Individual operation pause switches must all start unpaused.
#[test]
fn test_operation_pause_switches_disabled_after_init() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    for op in &[
        "pause_deposit",
        "pause_withdraw",
        "pause_borrow",
        "pause_repay",
        "pause_liquidate",
    ] {
        let sym = soroban_sdk::Symbol::new(&e, op);
        assert!(
            !client.is_operation_paused(&sym),
            "operation '{}' should NOT be paused after init",
            op
        );
    }
}

// ---------------------------------------------------------------------------
// 6. Admin CAN perform privileged operations post-init
// ---------------------------------------------------------------------------

/// The admin must be able to enable and then disable the emergency pause.
#[test]
fn test_admin_can_set_emergency_pause() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_emergency_pause(&admin, &true);
    assert!(client.is_emergency_paused(), "emergency pause should be ON");

    client.set_emergency_pause(&admin, &false);
    assert!(
        !client.is_emergency_paused(),
        "emergency pause should be OFF"
    );
}

/// The admin must be able to update the interest rate spread without error.
/// A small adjustment (200 → 210 bps, within the 10% change limit) must succeed.
#[test]
fn test_admin_can_update_interest_rate_config() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // Adjust spread from 200 to 210 bps (within ≤10% change limit).
    // If this panics the test fails automatically.
    client.update_interest_rate_config(
        &admin,
        &None,           // base_rate_bps
        &None,           // kink_utilization_bps
        &None,           // multiplier_bps
        &None,           // jump_multiplier_bps
        &None,           // rate_floor_bps
        &None,           // rate_ceiling_bps
        &Some(210_i128), // spread_bps
    );
}

// ---------------------------------------------------------------------------
// 7. Admin-only operations are enforced (non-admin must fail)
// ---------------------------------------------------------------------------

/// A non-admin caller must NOT be able to modify risk parameters; must panic.
#[test]
#[should_panic]
fn test_set_risk_params_unauthorized_caller_panics() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    let attacker = Address::generate(&e);
    client.set_risk_params(&attacker, &None, &None, &None, &None);
}

/// A non-admin caller must NOT be able to trigger emergency pause; must panic.
#[test]
#[should_panic]
fn test_set_emergency_pause_unauthorized_caller_panics() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    let attacker = Address::generate(&e);
    client.set_emergency_pause(&attacker, &true);
}

// ---------------------------------------------------------------------------
// 8. Collateral ratio helpers are functional post-init
// ---------------------------------------------------------------------------

/// With zero debt, `require_min_collateral_ratio` must succeed (infinite ratio).
#[test]
fn test_collateral_ratio_with_zero_debt_succeeds() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    // Must not panic – Result<(), _> unwrapped to () on success.
    client.require_min_collateral_ratio(&1_000_000_i128, &0_i128);
}

/// A position with collateral ≥ min_collateral_ratio (110%) must succeed.
#[test]
fn test_collateral_ratio_sufficient_succeeds() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    // 120 % collateral ratio: collateral = 120, debt = 100 – above 110 % minimum.
    client.require_min_collateral_ratio(&120_i128, &100_i128);
}

/// A position with collateral below the minimum (110%) must panic.
#[test]
#[should_panic]
fn test_collateral_ratio_insufficient_panics() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    // 105 % collateral ratio – below the 110 % minimum; must panic.
    client.require_min_collateral_ratio(&105_i128, &100_i128);
}

// ---------------------------------------------------------------------------
// 9. Liquidation eligibility helpers are functional post-init
// ---------------------------------------------------------------------------

/// A well-collateralized position (120%) must NOT be liquidatable.
#[test]
fn test_well_collateralized_position_cannot_be_liquidated() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    // 120 % – above the 105 % liquidation threshold.
    let can_liq = client.can_be_liquidated(&120_i128, &100_i128);
    assert!(
        !can_liq,
        "well-collateralized position must NOT be liquidatable"
    );
}

/// A position at exactly the liquidation threshold (100% ≤ 105%) must be liquidatable.
#[test]
fn test_undercollateralized_position_can_be_liquidated() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    // 100 % – below the 105 % liquidation threshold.
    let can_liq = client.can_be_liquidated(&100_i128, &100_i128);
    assert!(
        can_liq,
        "undercollateralized position (100%) must be liquidatable"
    );
}

/// A position with zero debt must never be eligible for liquidation.
#[test]
fn test_zero_debt_position_cannot_be_liquidated() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    let can_liq = client.can_be_liquidated(&1_000_i128, &0_i128);
    assert!(!can_liq, "zero-debt position must never be liquidatable");
}

// ---------------------------------------------------------------------------
// 10. Max liquidatable amount respects close factor (50 %)
// ---------------------------------------------------------------------------

/// With a 50% close factor, max liquidatable = debt × 5000 / 10000 = 50%.
#[test]
fn test_max_liquidatable_amount_respects_close_factor() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    let debt = 1_000_i128;
    let max = client.get_max_liquidatable_amount(&debt);
    // 50 % close factor → 1000 × 5000 / 10000 = 500
    assert_eq!(max, 500, "max liquidatable amount should be 50% of debt");
}

// ---------------------------------------------------------------------------
// 11. Liquidation incentive amount respects incentive rate (10 %)
// ---------------------------------------------------------------------------

/// With a 10% incentive, the liquidation bonus = liquidated × 1000 / 10000 = 10%.
#[test]
fn test_liquidation_incentive_amount() {
    let e = env();
    let (_id, _admin, client) = setup(&e);

    let liquidated = 1_000_i128;
    let incentive = client.get_liquidation_incentive_amount(&liquidated);
    // 10 % incentive → 1000 × 1000 / 10000 = 100
    assert_eq!(
        incentive, 100,
        "liquidation incentive should be 10% of liquidated amount"
    );
}
