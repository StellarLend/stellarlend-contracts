#![cfg(test)]

//! Regression tests for [`crate::config_snapshot::get_config_snapshot`].
//!
//! These pin the observable contract of the read-only configuration snapshot:
//!
//! - returns `None` before the contract has risk params (uninitialized);
//! - aggregates the risk parameters once they have been set through the real
//!   `risk_management` setters;
//! - `emergency_paused` mirrors the global emergency-pause flag;
//! - interest-rate fields fall back to [`InterestRateConfig::default`] when the
//!   interest-rate module has not been initialized; and
//! - the getter is read-only (repeated reads are identical and no underlying
//!   state changes).
//!
//! Note: the fallback used by `get_config_snapshot` is
//! `InterestRateConfig::default()`, which is a *non-zero* curve
//! (`base_rate_bps == 100`, `kink_utilization_bps == 8_000`, ...), not an
//! all-zero snapshot. These tests pin that actual behaviour.

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::admin;
use crate::config_snapshot::{get_config_snapshot, ConfigSnapshot};
use crate::interest_rate::{
    initialize_interest_rate_config, update_interest_rate_config, InterestRateConfig,
};
use crate::risk_management::{
    get_close_factor, get_liquidation_incentive, get_liquidation_threshold,
    get_min_collateral_ratio, initialize_risk_management, is_emergency_paused, set_emergency_pause,
    set_risk_params,
};

/// Run `f` with a fresh contract context that has an admin and the default risk
/// params stored, but deliberately **no** interest-rate config.
fn with_risk_only<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce(&Address) -> T,
{
    env.mock_all_auths();
    let contract_id = env.register(crate::cross_asset::NoOpContract {}, ());
    let admin = Address::generate(env);
    env.as_contract(&contract_id, || {
        admin::set_admin(env, admin.clone(), None).unwrap();
        initialize_risk_management(env, admin.clone()).unwrap();
        f(&admin)
    })
}

#[test]
fn returns_none_before_initialization() {
    let env = Env::default();
    let contract_id = env.register(crate::cross_asset::NoOpContract {}, ());
    let snapshot: Option<ConfigSnapshot> =
        env.as_contract(&contract_id, || get_config_snapshot(&env));
    assert_eq!(snapshot, None);
}

#[test]
fn aggregates_risk_params_once_set() {
    let env = Env::default();
    with_risk_only(&env, |admin| {
        // Values respect `set_risk_params`' 50%-of-current paced-change bound
        // relative to the 15_000 / 12_000 / 5_000 / 500 defaults.
        set_risk_params(
            &env,
            admin.clone(),
            Some(16_000),
            Some(13_000),
            Some(4_000),
            Some(600),
        )
        .unwrap();

        let snapshot =
            get_config_snapshot(&env).expect("snapshot must exist once risk params are set");
        assert_eq!(snapshot.min_collateral_ratio_bps, 16_000);
        assert_eq!(snapshot.liquidation_threshold_bps, 13_000);
        assert_eq!(snapshot.close_factor_bps, 4_000);
        assert_eq!(snapshot.liquidation_incentive_bps, 600);

        // ...and the snapshot mirrors the module getters exactly.
        assert_eq!(
            snapshot.min_collateral_ratio_bps,
            get_min_collateral_ratio(&env).unwrap()
        );
        assert_eq!(
            snapshot.liquidation_threshold_bps,
            get_liquidation_threshold(&env).unwrap()
        );
        assert_eq!(snapshot.close_factor_bps, get_close_factor(&env).unwrap());
        assert_eq!(
            snapshot.liquidation_incentive_bps,
            get_liquidation_incentive(&env).unwrap()
        );
    });
}

#[test]
fn emergency_paused_mirrors_pause_state() {
    let env = Env::default();
    with_risk_only(&env, |admin| {
        assert!(!get_config_snapshot(&env).unwrap().emergency_paused);
        assert_eq!(
            get_config_snapshot(&env).unwrap().emergency_paused,
            is_emergency_paused(&env)
        );

        set_emergency_pause(&env, admin.clone(), true).unwrap();
        assert!(get_config_snapshot(&env).unwrap().emergency_paused);
        assert_eq!(
            get_config_snapshot(&env).unwrap().emergency_paused,
            is_emergency_paused(&env)
        );

        set_emergency_pause(&env, admin.clone(), false).unwrap();
        assert!(!get_config_snapshot(&env).unwrap().emergency_paused);
    });
}

#[test]
fn interest_rate_fields_fall_back_to_interest_rate_default() {
    let env = Env::default();
    with_risk_only(&env, |_admin| {
        assert!(
            crate::interest_rate::get_interest_rate_config(&env).is_none(),
            "interest-rate config must be absent for this case"
        );

        let default = InterestRateConfig::default();
        let snapshot = get_config_snapshot(&env).unwrap();
        assert_eq!(snapshot.base_rate_bps, default.base_rate_bps);
        assert_eq!(snapshot.kink_utilization_bps, default.kink_utilization_bps);
        assert_eq!(snapshot.multiplier_bps, default.multiplier_bps);
        assert_eq!(snapshot.jump_multiplier_bps, default.jump_multiplier_bps);
        assert_eq!(snapshot.spread_bps, default.spread_bps);
        assert_eq!(snapshot.min_rate_bps, default.min_rate_bps);
        assert_eq!(snapshot.max_rate_bps, default.max_rate_bps);
    });
}

#[test]
fn mirrors_configured_interest_rate_fields() {
    let env = Env::default();
    with_risk_only(&env, |admin| {
        initialize_interest_rate_config(&env).unwrap();
        update_interest_rate_config(
            &env,
            admin.clone(),
            Some(250),
            Some(7_000),
            Some(3_000),
            Some(12_000),
            Some(50),
            Some(9_000),
            Some(75),
        )
        .unwrap();

        let snapshot = get_config_snapshot(&env).unwrap();
        assert_eq!(snapshot.base_rate_bps, 250);
        assert_eq!(snapshot.kink_utilization_bps, 7_000);
        assert_eq!(snapshot.multiplier_bps, 3_000);
        assert_eq!(snapshot.jump_multiplier_bps, 12_000);
        assert_eq!(snapshot.spread_bps, 75);
        assert_eq!(snapshot.min_rate_bps, 50);
        assert_eq!(snapshot.max_rate_bps, 9_000);
    });
}

#[test]
fn is_read_only_and_does_not_mutate_state() {
    let env = Env::default();
    with_risk_only(&env, |admin| {
        set_risk_params(
            &env,
            admin.clone(),
            Some(16_000),
            Some(13_000),
            Some(4_000),
            Some(600),
        )
        .unwrap();
        initialize_interest_rate_config(&env).unwrap();

        let risk_before = (
            get_min_collateral_ratio(&env).unwrap(),
            get_liquidation_threshold(&env).unwrap(),
            get_close_factor(&env).unwrap(),
            get_liquidation_incentive(&env).unwrap(),
        );
        let ir_before = crate::interest_rate::get_interest_rate_config(&env);
        let paused_before = is_emergency_paused(&env);

        let first = get_config_snapshot(&env).unwrap();
        let second = get_config_snapshot(&env).unwrap();

        // Pure getter: repeated reads are identical...
        assert_eq!(first, second);

        // ...and none of the underlying state changed.
        let risk_after = (
            get_min_collateral_ratio(&env).unwrap(),
            get_liquidation_threshold(&env).unwrap(),
            get_close_factor(&env).unwrap(),
            get_liquidation_incentive(&env).unwrap(),
        );
        assert_eq!(risk_after, risk_before);
        assert_eq!(
            crate::interest_rate::get_interest_rate_config(&env),
            ir_before
        );
        assert_eq!(is_emergency_paused(&env), paused_before);
    });
}
