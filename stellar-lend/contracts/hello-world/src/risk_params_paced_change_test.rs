//! Regression tests for the paced rate-change cap documented in
//! `stellar-lend/risk_params.md` and enforced by
//! `stellar-lend/contracts/hello-world/src/risk_params.rs`.
//!
//! The 10% delta cap must apply uniformly to **all four** parameters —
//! `min_collateral_ratio`, `liquidation_threshold`, `close_factor`, and
//! `liquidation_incentive`. Two of those (`close_factor`, `liquidation_incentive`)
//! historically bypassed the cap, allowing a single transaction to span
//! from the default straight to the upper bound. These tests guard against
//! that regression and pin the documented defaults.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::risk_params::{
    get_close_factor, get_liquidation_incentive, get_liquidation_threshold,
    get_min_collateral_ratio, initialize_risk_params, set_risk_params, RiskParamsError,
};

// ---------------------------------------------------------------------------
// Documented default values (must match initialize_risk_params)
// ---------------------------------------------------------------------------

const DEFAULT_MIN_COLLATERAL_RATIO_BPS: i128 = 15_000; // 150%
const DEFAULT_LIQUIDATION_THRESHOLD_BPS: i128 = 12_000; // 120%
const DEFAULT_CLOSE_FACTOR_BPS: i128 = 5_000; // 50%
const DEFAULT_LIQUIDATION_INCENTIVE_BPS: i128 = 500; // 5%

// ---------------------------------------------------------------------------
// Helpers (mirror the cross_asset_config_bounds_test.rs pattern)
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    Env::default()
}

fn with_contract<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce() -> T,
{
    let contract_id = env.register(crate::cross_asset::NoOpContract {}, ());
    env.as_contract(&contract_id, f)
}

fn init_with_admin() -> (Env, Address) {
    let env = make_env();
    let admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
    });
    (env, admin)
}

// ---------------------------------------------------------------------------
// Documented defaults
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_sets_documented_defaults() {
    let env = make_env();
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();

        assert_eq!(
            get_min_collateral_ratio(&env).unwrap(),
            DEFAULT_MIN_COLLATERAL_RATIO_BPS
        );
        assert_eq!(
            get_liquidation_threshold(&env).unwrap(),
            DEFAULT_LIQUIDATION_THRESHOLD_BPS
        );
        assert_eq!(get_close_factor(&env).unwrap(), DEFAULT_CLOSE_FACTOR_BPS);
        assert_eq!(
            get_liquidation_incentive(&env).unwrap(),
            DEFAULT_LIQUIDATION_INCENTIVE_BPS
        );
    });
}

// ---------------------------------------------------------------------------
// min_collateral_ratio: 10% cap (was already guarded)
// ---------------------------------------------------------------------------

#[test]
fn test_min_cr_accepts_exactly_10pct_up() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 15000 + 10% = 15000 + 1500 = 16500
        let r = set_risk_params(&env, Some(16_500), None, None, None);
        assert_eq!(r, Ok(()));
        assert_eq!(get_min_collateral_ratio(&env).unwrap(), 16_500);
    });
}

#[test]
fn test_min_cr_rejects_more_than_10pct_up() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 15000 + 10% = 16500 — 16601 is just above the cap.
        let r = set_risk_params(&env, Some(16_601), None, None, None);
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
        // Stored value is unchanged.
        assert_eq!(
            get_min_collateral_ratio(&env).unwrap(),
            DEFAULT_MIN_COLLATERAL_RATIO_BPS
        );
    });
}

#[test]
fn test_min_cr_accepts_exactly_10pct_down() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 15000 - 10% = 13500
        let r = set_risk_params(&env, Some(13_500), None, None, None);
        assert_eq!(r, Ok(()));
    });
}

#[test]
fn test_min_cr_rejects_more_than_10pct_down() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 15000 - 10% = 13500 — 13499 is just below.
        let r = set_risk_params(&env, Some(13_499), None, None, None);
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
    });
}

// ---------------------------------------------------------------------------
// liquidation_threshold: 10% cap (was already guarded)
// ---------------------------------------------------------------------------

#[test]
fn test_liquidation_threshold_rejects_more_than_10pct_up() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 12000 + 10% = 13200 — 13201 is just above the cap.
        let r = set_risk_params(&env, None, Some(13_201), None, None);
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
    });
}

// ---------------------------------------------------------------------------
// close_factor: 10% cap (regression guard — previously bypassed!)
// ---------------------------------------------------------------------------

/// Historically `close_factor` could be updated in one call from default (50%)
/// straight to 100% with no mid-step. The 10% cap means a single
/// jump from 5_000 to 10_000 must now be rejected.
#[test]
fn test_close_factor_rejects_more_than_10pct_up() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 5000 + 10% = 5500 — 5501 is just above the cap.
        let r = set_risk_params(&env, None, None, Some(5_501), None);
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
        // Stored value is unchanged.
        assert_eq!(get_close_factor(&env).unwrap(), DEFAULT_CLOSE_FACTOR_BPS);
    });
}

/// 5_500 is exactly 10% above the default and must be accepted, allowing an
/// operator to legitimately step the close factor upward.
#[test]
fn test_close_factor_accepts_exactly_10pct_up() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        let r = set_risk_params(&env, None, None, Some(5_500), None);
        assert_eq!(r, Ok(()));
        assert_eq!(get_close_factor(&env).unwrap(), 5_500);
    });
}

/// Same change cap applies on the way down. Going from 50% to 1 bp (the
/// lower bound) must pass through intermediate values of at most 10% each.
#[test]
fn test_close_factor_rejects_more_than_10pct_down() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 5000 - 10% = 4500 — 4499 is just below.
        let r = set_risk_params(&env, None, None, Some(4_499), None);
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
    });
}

/// The absolute bound check still fires: `close_factor = 0` continues to
/// be rejected (just as it was before the cap was tightened), but is now
/// reported via the paced-change path *after* crossing the lower bound
/// first. Either error mapping is acceptable — both block the operation
/// before any state mutation.
#[test]
fn test_close_factor_zero_still_rejected() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        let r = set_risk_params(&env, None, None, Some(0), None);
        // 0 is below the absolute MIN_CLOSE_FACTOR (1), so the bounds check
        // surfaces as InvalidCloseFactor.
        assert_eq!(r, Err(RiskParamsError::InvalidCloseFactor));
    });
}

// ---------------------------------------------------------------------------
// liquidation_incentive: 10% cap (regression guard — previously bypassed!)
// ---------------------------------------------------------------------------

/// Historically `liquidation_incentive` could be updated in one call from
/// default (5%) straight to the maximum 50% with no mid-step. The 10% cap
/// means that immediate full-range spike is now rejected.
#[test]
fn test_liquidation_incentive_rejects_more_than_10pct_up() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // 500 + 10% = 550 — 551 is just above the cap.
        let r = set_risk_params(&env, None, None, None, Some(551));
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
        assert_eq!(
            get_liquidation_incentive(&env).unwrap(),
            DEFAULT_LIQUIDATION_INCENTIVE_BPS
        );
    });
}

#[test]
fn test_liquidation_incentive_accepts_exactly_10pct_up() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        let r = set_risk_params(&env, None, None, None, Some(550));
        assert_eq!(r, Ok(()));
        assert_eq!(get_liquidation_incentive(&env).unwrap(), 550);
    });
}

/// Previously a jump from default 500 to 0 was *accepted* because 0 falls in
/// the legal `[0, 5_000]` range and the paced-change check did not apply.
/// With the 10% cap now enforced uniformly, the change is rejected with
/// `ParameterChangeTooLarge` (delta = 500 > 10% of 500 = 50).
#[test]
fn test_liquidation_incentive_jump_to_zero_blocked_by_paced_change() {
    let env = make_env();
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        let r = set_risk_params(&env, None, None, None, Some(0));
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
    });
}

// ---------------------------------------------------------------------------
// Incremental stepping reaches the max — proves the cap is the *only*
// obstacle, not a missed absolute-bound check.
// ---------------------------------------------------------------------------

/// Starting from default 5_000 (50%) the operator can increment by exactly
/// 10% per call; the next attempt to skip ahead is rejected.  This proves
/// the cap is enforced on the *incremented* value, not just on the first
/// call from initialization.
#[test]
fn test_close_factor_third_step_blocked_after_two_paced_steps() {
    let env = make_env();
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();

        // Step 1: 5_000 -> 5_500 (exactly +10%).
        set_risk_params(&env, None, None, Some(5_500), None).unwrap();
        assert_eq!(get_close_factor(&env).unwrap(), 5_500);

        // Step 2: 5_500 -> 6_050 (exactly +10% of 5_500).
        set_risk_params(&env, None, None, Some(6_050), None).unwrap();
        assert_eq!(get_close_factor(&env).unwrap(), 6_050);

        // Attempt to skip ahead to 10_000 from 6_050:
        // delta = 3_950 > 10% of 6_050 = 605.
        let r = set_risk_params(&env, None, None, Some(10_000), None);
        assert_eq!(r, Err(RiskParamsError::ParameterChangeTooLarge));
    });
}

// ---------------------------------------------------------------------------
// Out-of-range absolute values still surface as the documented error
// ---------------------------------------------------------------------------

#[test]
fn test_min_cr_below_absolute_floor_still_rejected() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // MIN_COLLATERAL_RATIO_FLOOR = 10_000; 9_999 fails the bounds before
        // any paced-change logic runs.
        let r = set_risk_params(&env, Some(9_999), None, None, None);
        assert_eq!(r, Err(RiskParamsError::InvalidCollateralRatio));
    });
}

#[test]
fn test_close_factor_above_absolute_ceiling_still_rejected() {
    let env = make_env();
    env.mock_all_auths();
    let _admin = Address::generate(&env);
    with_contract(&env, || {
        initialize_risk_params(&env).unwrap();
        // MAX_CLOSE_FACTOR = 10_000; 10_001 fails the bounds before any
        // paced-change logic runs.
        let r = set_risk_params(&env, None, None, Some(10_001), None);
        assert_eq!(r, Err(RiskParamsError::InvalidCloseFactor));
    });
}
