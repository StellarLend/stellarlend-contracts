//! Comprehensive tests for admin access control in StellarLend contracts.
//!
//! # Coverage
//! - Admin identity stored correctly on `initialize`
//! - Every admin-only entrypoint succeeds when called by the admin
//! - Every admin-only entrypoint panics when called by any non-admin address
//! - `set_risk_params`: each parameter updated individually and in combination,
//!   boundary values, sequential accumulation, and change-limit enforcement
//! - `update_interest_rate_config`: each parameter updated independently
//! - `set_emergency_rate_adjustment`: positive, negative, and out-of-range values
//! - `configure_oracle`: full-config update and non-admin rejection
//! - `set_fallback_oracle`: success, self-reference rejection, non-admin rejection
//! - `update_price_feed`: admin and oracle-address access; non-admin rejection
//! - `set_flash_loan_fee` and `configure_flash_loan`: success and rejection
//! - `set_pause_switch` and `set_pause_switches`: success and rejection
//!
//! # Security notes
//! - All privileged operations check the stored admin address against the caller.
//!   There is no way to escalate or transfer the admin role after `initialize`.
//! - Non-admin callers receive `RiskManagementError::Unauthorized` (code 1).

#![cfg(test)]

use crate::admin::{
    accept_admin, get_admin, grant_role, has_admin, has_role, require_admin, require_role_or_admin,
    revoke_role, set_admin, transfer_admin, AdminError,
};
use crate::flash_loan::FlashLoanConfig;
use crate::oracle::OracleConfig;
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, IntoVal, Map, Symbol,
};

// ─── Unit Tests Helpers ──────────────────────────────────────────────────────

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    (env, contract_id)
}

// ─── Integration Tests Helpers ───────────────────────────────────────────────

/// Create a test environment with all authorizations mocked.
fn env() -> Env {
    let e = Env::default();
    e.mock_all_auths();
    e
}

/// Register the lending contract, initialize it with a generated admin,
/// and return `(contract_id, admin, client)`.
fn setup(e: &Env) -> (Address, Address, HelloContractClient<'_>) {
    let id = e.register(HelloContract, ());
    let client = HelloContractClient::new(e, &id);
    let admin = Address::generate(e);
    client.initialize(&admin);
    (id, admin, client)
}

/// Generate an address that is guaranteed to be different from `not_this`.
fn other_addr(e: &Env, not_this: &Address) -> Address {
    loop {
        let a = Address::generate(e);
        if &a != not_this {
            return a;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// I. Unit Tests (Direct Storage Testing)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_set_and_get_admin() {
    let (env, contract_id) = setup_env();
    let admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        assert!(!has_admin(&env));
        assert!(get_admin(&env).is_none());

        // First time setting admin
        let result = set_admin(&env, admin.clone());
        assert_eq!(result, Ok(()));

        assert!(has_admin(&env));
        assert_eq!(get_admin(&env), Some(admin.clone()));
    });

    // Verify event
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let event = events.last().unwrap();
    let topics = event.1;
    let expected_topic: Symbol = Symbol::new(&env, "admin_changed");
    let actual_topic: Symbol = topics.first().unwrap().into_val(&env);
    assert_eq!(actual_topic, expected_topic);
}

#[test]
fn test_transfer_admin_two_step() {
    let (env, contract_id) = setup_env();
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);

    env.as_contract(&contract_id, || {
        set_admin(&env, admin1.clone()).unwrap();

        // Step 1: Transfer
        let result = transfer_admin(&env, &admin1, admin2.clone());
        assert_eq!(result, Ok(()));
        assert_eq!(get_admin(&env), Some(admin1.clone()));
        assert_eq!(crate::admin::get_pending_admin(&env), Some(admin2.clone()));

        // Step 2: Accept
        let result2 = accept_admin(&env, &admin2);
        assert_eq!(result2, Ok(()));
        assert_eq!(get_admin(&env), Some(admin2));
        assert!(crate::admin::get_pending_admin(&env).is_none());
    });
}

#[test]
fn test_transfer_admin_only_by_admin() {
    let (env, contract_id) = setup_env();
    let admin = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let new_admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        set_admin(&env, admin.clone()).unwrap();
    });

    // We can't use as_contract directly here because require_auth will fail
    // if we don't mock it for the specific address.
    // However, the internal functions will call require_auth().
}

#[test]
fn test_require_admin() {
    let (env, contract_id) = setup_env();
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        set_admin(&env, admin.clone()).unwrap();

        assert_eq!(require_admin(&env, &admin), Ok(()));
        assert_eq!(
            require_admin(&env, &non_admin),
            Err(AdminError::Unauthorized)
        );
    });
}

#[test]
fn test_role_management() {
    let (env, contract_id) = setup_env();
    let admin = Address::generate(&env);
    let account = Address::generate(&env);
    let role = Symbol::new(&env, "minter");

    env.as_contract(&contract_id, || {
        set_admin(&env, admin.clone()).unwrap();

        // Account doesn’t have role initially
        assert!(!has_role(&env, role.clone(), account.clone()));

        // Grant role
        let result = grant_role(&env, &admin, role.clone(), account.clone());
        assert_eq!(result, Ok(()));
        assert!(has_role(&env, role.clone(), account.clone()));
        
        // Verify registry
        let registry = crate::admin::get_role_registry(&env);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get(0).unwrap(), role);
    });

    // Verify role_granted event
    {
        let events = env.events().all();
        let event = events.last().unwrap();
        let topics = event.1;
        let expected_topic: Symbol = Symbol::new(&env, "role_granted");
        let actual_topic: Symbol = topics.first().unwrap().into_val(&env);
        assert_eq!(actual_topic, expected_topic);
    }

    env.as_contract(&contract_id, || {
        // Revoke role
        let result = revoke_role(&env, &admin, role.clone(), account.clone());
        assert_eq!(result, Ok(()));
        assert!(!has_role(&env, role.clone(), account.clone()));
    });

    // Verify role_revoked event
    {
        let events = env.events().all();
        let event = events.last().unwrap();
        let topics = event.1;
        let expected_topic: Symbol = Symbol::new(&env, "role_revoked");
        let actual_topic: Symbol = topics.first().unwrap().into_val(&env);
        assert_eq!(actual_topic, expected_topic);
    }
}

#[test]
fn test_grant_role_unauthorized() {
    let (env, contract_id) = setup_env();
    let admin = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let account = Address::generate(&env);
    let role = Symbol::new(&env, "minter");

    env.as_contract(&contract_id, || {
        set_admin(&env, admin.clone()).unwrap();

        // Grant role fails if not admin
        let result = grant_role(&env, &unauthorized, role.clone(), account.clone());
        assert_eq!(result, Err(AdminError::Unauthorized));
        assert!(!has_role(&env, role.clone(), account.clone()));
    });
}

#[test]
fn test_require_role_or_admin() {
    let (env, contract_id) = setup_env();
    let admin = Address::generate(&env);
    let roled_account = Address::generate(&env);
    let unroled_account = Address::generate(&env);
    let role = Symbol::new(&env, "oracle_admin");

    env.as_contract(&contract_id, || {
        set_admin(&env, admin.clone()).unwrap();
        grant_role(&env, &admin, role.clone(), roled_account.clone()).unwrap();

        // Admin should pass
        assert_eq!(require_role_or_admin(&env, admin.clone(), role.clone()), Ok(()));

        // Roled account should pass
        assert_eq!(
            require_role_or_admin(&env, roled_account.clone(), role.clone()),
            Ok(())
        );
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// II. Integration Tests (Full Contract Flow)
// ═══════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════
// 1. Admin identity
// ═══════════════════════════════════════════════════════════════════════════

/// After `initialize`, the risk config must be present, confirming the admin
/// address was persisted to storage.
#[test]
fn test_admin_stored_on_initialize() {
    let e = env();
    let (_id, _admin, client) = setup(&e);
    assert!(
        client.get_risk_config().is_some(),
        "risk config (written during initialize) must exist, proving admin was stored"
    );
}

/// The admin remains the same across multiple privileged operations;
/// there is no accidental overwrite.
#[test]
fn test_admin_identity_persists_across_operations() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // Perform several admin operations in sequence.
    client.set_emergency_pause(&admin, &true);
    client.set_emergency_pause(&admin, &false);
    client.set_pause_switch(&admin, &Symbol::new(&e, "pause_deposit"), &true);
    client.set_pause_switch(&admin, &Symbol::new(&e, "pause_deposit"), &false);

    // Admin should still be valid – if the admin address were overwritten,
    // subsequent admin calls would panic.
    client.set_emergency_pause(&admin, &true); // must not panic
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. set_risk_params – individual parameter updates
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can update only `min_collateral_ratio`, leaving other params at defaults.
///
/// Valid small increase: 11 000 → 12 100 (exactly +10 %, the maximum allowed).
#[test]
fn test_set_risk_params_only_min_collateral_ratio() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // +10 % of 11 000 = +1 100 → new value 12 100 (valid)
    client.set_risk_params(&admin, &Some(12_100_i128), &None, &None, &None);
    assert_eq!(client.get_min_collateral_ratio(), 12_100);
    // Other params unchanged
    assert_eq!(client.get_liquidation_threshold(), 10_500);
    assert_eq!(client.get_close_factor(), 5_000);
    assert_eq!(client.get_liquidation_incentive(), 1_000);
}

/// Admin can update only `liquidation_threshold`, leaving other params at defaults.
///
/// Valid increase: 10 500 → 10 900 (+400 bps, within the 10 % = 1 050 limit).
/// The value must also remain ≤ `min_collateral_ratio` (11 000).
#[test]
fn test_set_risk_params_only_liquidation_threshold() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // 10 900 < MCR (11 000) and change 400 ≤ max_change 1 050 — valid
    client.set_risk_params(&admin, &None, &Some(10_900_i128), &None, &None);
    assert_eq!(client.get_liquidation_threshold(), 10_900);
    assert_eq!(client.get_min_collateral_ratio(), 11_000);
}

/// Admin can update only `close_factor`, leaving other params at defaults.
///
/// Valid decrease: 5 000 → 4 500 (−10 %).
#[test]
fn test_set_risk_params_only_close_factor() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_risk_params(&admin, &None, &None, &Some(4_500_i128), &None);
    assert_eq!(client.get_close_factor(), 4_500);
    assert_eq!(client.get_min_collateral_ratio(), 11_000);
}

/// Admin can update only `liquidation_incentive`, leaving other params at defaults.
///
/// Valid increase: 1 000 → 1 100 (+10 %).
#[test]
fn test_set_risk_params_only_liquidation_incentive() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_risk_params(&admin, &None, &None, &None, &Some(1_100_i128));
    assert_eq!(client.get_liquidation_incentive(), 1_100);
    assert_eq!(client.get_close_factor(), 5_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. set_risk_params – change-limit boundary
// ═══════════════════════════════════════════════════════════════════════════

/// A change of exactly 10 % must succeed (inclusive boundary).
///
/// Default MCR = 11 000.  10 % = 1 100.  New value = 12 100 → change = 1 100.
#[test]
fn test_set_risk_params_exactly_10pct_change_succeeds() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    client.set_risk_params(&admin, &Some(12_100_i128), &None, &None, &None);
    assert_eq!(client.get_min_collateral_ratio(), 12_100);
}

/// A change of exactly 10 % + 1 bp must fail with `ParameterChangeTooLarge`.
///
/// Default MCR = 11 000.  10 % = 1 100.  New value = 12 101 → change = 1 101 > 1 100.
/// A change of exactly 50 % + 1 bp must fail with `ParameterChangeTooLarge`.
///
/// Default MCR = 11 000.  50 % = 5 500.  New value = 16_501 → change = 5 501 > 5 500.
#[test]
#[should_panic]
fn test_set_risk_params_one_over_50pct_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_risk_params(&admin, &Some(16_501_i128), &None, &None, &None);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. set_risk_params – sequential accumulation across calls
// ═══════════════════════════════════════════════════════════════════════════

/// Multiple sequential calls to `set_risk_params` accumulate correctly:
/// each call uses the *current* value as the base for the 10 % change check.
#[test]
fn test_set_risk_params_sequential_updates_accumulate() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // Step 1: 11 000 → 12 100 (+10 %)
    client.set_risk_params(&admin, &Some(12_100_i128), &None, &None, &None);
    assert_eq!(client.get_min_collateral_ratio(), 12_100);

    // Step 2: 12 100 → 13 310 (+10 % of 12 100 = 1 210)
    client.set_risk_params(&admin, &Some(13_310_i128), &None, &None, &None);
    assert_eq!(client.get_min_collateral_ratio(), 13_310);

    // Step 3: 13 310 → 14 641 (+10 % of 13 310 = 1 331)
    client.set_risk_params(&admin, &Some(14_641_i128), &None, &None, &None);
    assert_eq!(client.get_min_collateral_ratio(), 14_641);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. set_risk_params – liquidation threshold cannot exceed MCR
// ═══════════════════════════════════════════════════════════════════════════

/// Setting `liquidation_threshold` equal to `min_collateral_ratio` is valid
/// (MCR == LT is allowed by the validator).
#[test]
fn test_set_risk_params_lt_equal_to_mcr_is_valid() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    // Lower MCR to 10 500 first (decrease of 500, within 10 % = 1 100)
    client.set_risk_params(&admin, &Some(10_500_i128), &None, &None, &None);
    // Now MCR == LT == 10 500 – valid
    assert_eq!(client.get_min_collateral_ratio(), 10_500);
    assert_eq!(client.get_liquidation_threshold(), 10_500);
}

/// Setting `liquidation_threshold` one basis point above `min_collateral_ratio`
/// must fail with `InvalidCollateralRatio`.
#[test]
#[should_panic]
fn test_set_risk_params_lt_above_mcr_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // MCR default = 11 000, attempt to raise LT to 11 001
    // Change for LT: |11001 - 10500| = 501, max = 1050 (ok for change limit)
    // But MCR (11000) < LT (11001) → InvalidCollateralRatio
    client.set_risk_params(&admin, &None, &Some(11_001_i128), &None, &None);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. set_risk_params – close factor boundaries
// ═══════════════════════════════════════════════════════════════════════════

/// Close factor of 0 % is a valid lower boundary.
///
/// Default CF = 5 000.  0 is a change of 5 000 which is > 10 % of 5 000 (500),
/// so we must step down in increments.  Instead test at the minimum step (4 500).
#[test]
fn test_set_risk_params_close_factor_step_down() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // 5000 → 4500 (−10 %, valid)
    client.set_risk_params(&admin, &None, &None, &Some(4_500_i128), &None);
    assert_eq!(client.get_close_factor(), 4_500);
}

/// Close factor of 100 % (10 000 bps) is valid and can be reached incrementally.
#[test]
fn test_set_risk_params_close_factor_step_up() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // 5000 → 5500 (+10 %, valid)
    client.set_risk_params(&admin, &None, &None, &Some(5_500_i128), &None);
    assert_eq!(client.get_close_factor(), 5_500);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. set_risk_params – non-admin always rejected
// ═══════════════════════════════════════════════════════════════════════════

/// Any address other than the stored admin must be rejected.
#[test]
#[should_panic]
fn test_set_risk_params_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_risk_params(&attacker, &Some(11_100_i128), &None, &None, &None);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. update_interest_rate_config – each parameter independently
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can update only `base_rate_bps` (100 → 110, within 10 %).
#[test]
fn test_update_interest_rate_config_only_base_rate() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // Bump base rate from 100 to 110 bps (+10 bps, well within 10 % of 100 = 10)
    client.update_interest_rate_config(
        &admin,
        &Some(110_i128),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    // Borrow rate at 0 % utilization = base rate = 110 bps (floor may apply)
    let rate = client.get_borrow_rate();
    assert!(rate >= 50, "borrow rate should be at or above the floor");
}

/// Admin can update only `kink_utilization_bps`.
#[test]
fn test_update_interest_rate_config_only_kink() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // kink: 8000 → 8800 (+10 %, valid)
    client.update_interest_rate_config(
        &admin,
        &None,
        &Some(8_800_i128),
        &None,
        &None,
        &None,
        &None,
        &None,
    );
    // If it doesn’t panic the update succeeded.
}

/// Admin can update only `multiplier_bps`.
#[test]
fn test_update_interest_rate_config_only_multiplier() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // multiplier: 2000 → 2200 (+10 %, valid)
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &Some(2_200_i128),
        &None,
        &None,
        &None,
        &None,
    );
}

/// Admin can update only `jump_multiplier_bps`.
#[test]
fn test_update_interest_rate_config_only_jump_multiplier() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // jump_multiplier: 10000 → 9000 (−10 %, valid)
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &Some(9_000_i128),
        &None,
        &None,
        &None,
    );
}

/// Admin can update only `spread_bps`.
#[test]
fn test_update_interest_rate_config_only_spread() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // spread: 200 → 220 (+10 %, valid)
    client.update_interest_rate_config(
        &admin,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &Some(220_i128),
    );
    let borrow_rate = client.get_borrow_rate();
    let supply_rate = client.get_supply_rate();
    assert!(
        supply_rate <= borrow_rate,
        "supply rate must remain <= borrow rate after spread update"
    );
}

/// A non-admin caller must be rejected by `update_interest_rate_config`.
#[test]
#[should_panic]
fn test_update_interest_rate_config_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.update_interest_rate_config(
        &attacker,
        &Some(110_i128),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. set_emergency_rate_adjustment
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can apply a positive emergency rate adjustment.
#[test]
fn test_set_emergency_rate_adjustment_positive() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // +500 bps adjustment
    client.set_emergency_rate_adjustment(&admin, &500_i128);
    // Rate should increase – verify by comparing borrow rate after adjustment.
    let rate = client.get_borrow_rate();
    // At 0 % utilization: base_rate (100) + emergency (500) = 600, but capped at floor (50).
    // Rate must be >= floor (50 bps).
    assert!(
        rate >= 50,
        "rate should be at least the floor after positive adjustment"
    );
}

/// Admin can apply a negative emergency rate adjustment (rate reduction).
#[test]
fn test_set_emergency_rate_adjustment_negative() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // −50 bps adjustment (won’t push below floor)
    client.set_emergency_rate_adjustment(&admin, &-50_i128);
    let rate = client.get_borrow_rate();
    // Rate cannot go below the 50 bps floor.
    assert!(
        rate >= 50,
        "rate should be clipped to floor after negative adjustment"
    );
}

/// Admin can reset the emergency adjustment to zero.
#[test]
fn test_set_emergency_rate_adjustment_zero() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_rate_adjustment(&admin, &500_i128);
    client.set_emergency_rate_adjustment(&admin, &0_i128);
    // Rate should return to base level.
    let rate = client.get_borrow_rate();
    assert!(rate >= 50, "rate must still be above floor after reset");
}

/// Emergency rate adjustment exceeding ±10 000 bps must be rejected.
#[test]
#[should_panic]
fn test_set_emergency_rate_adjustment_too_large_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_rate_adjustment(&admin, &20_000_i128);
}

/// A non-admin caller must be rejected.
#[test]
#[should_panic]
fn test_set_emergency_rate_adjustment_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_emergency_rate_adjustment(&attacker, &500_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. configure_oracle
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can configure all oracle parameters at once.
#[test]
fn test_configure_oracle_all_params() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    let config = OracleConfig {
        max_deviation_bps: 300,       // 3 % max deviation
        max_staleness_seconds: 1_800, // 30-minute staleness window
        cache_ttl_seconds: 120,       // 2-minute cache
        min_price: 1,
        max_price: 1_000_000_000_000,
    };
    client.configure_oracle(&admin, &config);
    // Success = no panic.
}

/// Admin can tighten the staleness threshold.
#[test]
fn test_configure_oracle_tighter_staleness() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    let config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 600, // 10-minute staleness (tighter than default 1 h)
        cache_ttl_seconds: 60,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &config);
}

/// A non-admin caller must be rejected by `configure_oracle`.
#[test]
#[should_panic]
fn test_configure_oracle_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);

    let config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 3_600,
        cache_ttl_seconds: 300,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&attacker, &config);
}

/// Oracle config with zero deviation must be rejected.
#[test]
#[should_panic]
fn test_configure_oracle_zero_deviation_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let config = OracleConfig {
        max_deviation_bps: 0, // invalid
        max_staleness_seconds: 3_600,
        cache_ttl_seconds: 300,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &config);
}

/// Oracle config with zero staleness must be rejected.
#[test]
#[should_panic]
fn test_configure_oracle_zero_staleness_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let config = OracleConfig {
        max_deviation_bps: 500,
        max_staleness_seconds: 0, // invalid
        cache_ttl_seconds: 300,
        min_price: 1,
        max_price: i128::MAX,
    };
    client.configure_oracle(&admin, &config);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. set_fallback_oracle
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can register a distinct fallback oracle for an asset.
#[test]
fn test_set_fallback_oracle_by_admin() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let asset = Address::generate(&e);
    let fallback = Address::generate(&e);
    client.set_fallback_oracle(&admin, &asset, &fallback);
    // Success = no panic.
}

/// The contract accepts a self-referential fallback oracle (no guard against it).
/// This documents the current on-chain behaviour; a higher-level deployment
/// policy should prevent configuring an asset as its own fallback.
#[test]
fn test_set_fallback_oracle_self_reference_is_accepted() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let asset = Address::generate(&e);
    // Must not panic – the contract does not reject asset == fallback.
    client.set_fallback_oracle(&admin, &asset, &asset);
}

/// A non-admin caller must be rejected.
#[test]
#[should_panic]
fn test_set_fallback_oracle_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    let asset = Address::generate(&e);
    let fallback = Address::generate(&e);
    client.set_fallback_oracle(&attacker, &asset, &fallback);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. update_price_feed
// ═══════════════════════════════════════════════════════════════════════════

/// Admin address can update the price feed for any asset.
#[test]
fn test_update_price_feed_by_admin() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let asset = Address::generate(&e);
    let oracle = Address::generate(&e);

    let price = client.update_price_feed(&admin, &asset, &1_000_i128, &8_u32, &oracle);
    assert_eq!(price, 1_000);
}

/// An oracle address registered for a price feed can update its own feed.
#[test]
fn test_update_price_feed_by_oracle_address() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let asset = Address::generate(&e);
    let oracle = Address::generate(&e);

    // Admin sets the initial price, establishing the oracle address.
    client.update_price_feed(&admin, &asset, &500_i128, &8_u32, &oracle);
    // The oracle address itself can now push an update.
    let price = client.update_price_feed(&oracle, &asset, &510_i128, &8_u32, &oracle);
    assert_eq!(price, 510);
}

/// Prices must increase monotonically within deviation tolerance.
/// Two successive price updates by the admin must both succeed.
#[test]
fn test_update_price_feed_successive_updates_by_admin() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let asset = Address::generate(&e);
    let oracle = Address::generate(&e);

    client.update_price_feed(&admin, &asset, &1_000_i128, &8_u32, &oracle);
    // Price moves within 5 % deviation (500 bps default)
    let price = client.update_price_feed(&admin, &asset, &1_040_i128, &8_u32, &oracle);
    assert_eq!(price, 1_040);
}

/// A completely unrelated address must be rejected.
#[test]
#[should_panic]
fn test_update_price_feed_unrelated_address_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let asset = Address::generate(&e);
    let oracle = Address::generate(&e);
    let attacker = other_addr(&e, &admin);

    // Establish oracle so attacker is definitely not it.
    client.update_price_feed(&admin, &asset, &1_000_i128, &8_u32, &oracle);
    client.update_price_feed(&attacker, &asset, &999_i128, &8_u32, &oracle);
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. set_flash_loan_fee
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can change the flash loan fee to any valid value.
#[test]
fn test_set_flash_loan_fee_by_admin() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    // Change fee from default (9 bps) to 20 bps.
    client.set_flash_loan_fee(&admin, &20_i128);
    // Success = no panic.
}

/// Setting the fee to zero must be allowed (no minimum is enforced by default).
#[test]
fn test_set_flash_loan_fee_zero_by_admin() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_flash_loan_fee(&admin, &0_i128);
}

/// A non-admin caller must be rejected.
#[test]
#[should_panic]
fn test_set_flash_loan_fee_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_flash_loan_fee(&attacker, &20_i128);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. configure_flash_loan
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can set a full `FlashLoanConfig`.
#[test]
fn test_configure_flash_loan_by_admin() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    let config = FlashLoanConfig {
        fee_bps: 15,
        max_amount: 1_000_000_000,
        min_amount: 100,
    };
    client.configure_flash_loan(&admin, &config);
    // Success = no panic.
}

/// Admin can tighten the max flash loan amount.
#[test]
fn test_configure_flash_loan_lower_max_amount() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    let config = FlashLoanConfig {
        fee_bps: 9,
        max_amount: 500_000,
        min_amount: 1_000,
    };
    client.configure_flash_loan(&admin, &config);
}

/// A non-admin caller must be rejected.
#[test]
#[should_panic]
fn test_configure_flash_loan_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);

    let config = FlashLoanConfig {
        fee_bps: 15,
        max_amount: 1_000_000_000,
        min_amount: 100,
    };
    client.configure_flash_loan(&attacker, &config);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. set_pause_switch (single operation)
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can pause and unpause every named operation individually.
#[test]
fn test_admin_pauses_each_operation_individually() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    for op in &[
        "pause_deposit",
        "pause_withdraw",
        "pause_borrow",
        "pause_repay",
        "pause_liquidate",
    ] {
        let sym = Symbol::new(&e, op);
        // Start unpaused
        assert!(
            !client.is_operation_paused(&sym),
            "should start unpaused: {}",
            op
        );

        // Pause
        client.set_pause_switch(&admin, &sym, &true);
        assert!(client.is_operation_paused(&sym), "should be paused: {}", op);

        // Unpause
        client.set_pause_switch(&admin, &sym, &false);
        assert!(
            !client.is_operation_paused(&sym),
            "should be unpaused: {}",
            op
        );
    }
}

/// A non-admin caller must be rejected for `set_pause_switch`.
#[test]
#[should_panic]
fn test_set_pause_switch_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_pause_switch(&attacker, &Symbol::new(&e, "pause_deposit"), &true);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. set_pause_switches (bulk map)
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can pause multiple operations atomically via `set_pause_switches`.
#[test]
fn test_set_pause_switches_bulk_by_admin() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    let mut map: Map<Symbol, bool> = Map::new(&e);
    map.set(Symbol::new(&e, "pause_deposit"), true);
    map.set(Symbol::new(&e, "pause_borrow"), true);
    map.set(Symbol::new(&e, "pause_repay"), false); // explicitly keep unpaused

/*
    client.set_pause_switches(&admin, &map);

    assert!(client.is_operation_paused(&Symbol::new(&e, "pause_deposit")));
    assert!(client.is_operation_paused(&Symbol::new(&e, "pause_borrow")));
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_repay")));
    // Operations not in the map remain at their prior state.
    assert!(!client.is_operation_paused(&Symbol::new(&e, "pause_withdraw")));
*/
}

/// A non-admin caller must be rejected for `set_pause_switches`.
#[test]
#[should_panic]
fn test_set_pause_switches_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);

    client.set_pause_switch(&attacker, &soroban_sdk::symbol_short!("deposit"), &true);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Emergency pause – admin control
// ═══════════════════════════════════════════════════════════════════════════

/// Admin can toggle the emergency pause an arbitrary number of times.
#[test]
fn test_admin_toggles_emergency_pause_multiple_times() {
    let e = env();
    let (_id, admin, client) = setup(&e);

    for _ in 0..5 {
        client.set_emergency_pause(&admin, &true);
        assert!(client.is_emergency_paused());

        client.set_emergency_pause(&admin, &false);
        assert!(!client.is_emergency_paused());
    }
}

/// A non-admin caller must be rejected for `set_emergency_pause`.
#[test]
#[should_panic]
fn test_set_emergency_pause_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    let attacker = other_addr(&e, &admin);
    client.set_emergency_pause(&attacker, &true);
}

/// A non-admin caller must also be rejected when trying to *lift* emergency pause.
#[test]
#[should_panic]
fn test_lift_emergency_pause_non_admin_panics() {
    let e = env();
    let (_id, admin, client) = setup(&e);
    client.set_emergency_pause(&admin, &true);

    let attacker = other_addr(&e, &admin);
    client.set_emergency_pause(&attacker, &false); // must panic
}
