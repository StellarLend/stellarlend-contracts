//! Tests for per-asset rate-params overrides ([#1258](https://github.com/StellarLend/stellarlend-contracts/issues/1258)).
//!
//! Covers:
//! - no override → global / default fallback
//! - override present → supersedes global
//! - invalid override rejected
//! - unauthorized setter rejected (auth)
//! - clear restores global curve
//! - `compute_borrow_rate` remains pure and produces byte-identical rates when no override

#![cfg(test)]

use crate::rate_model::{
    compute_borrow_rate, validate_rate_params, RateParams, RateParamsValidationError,
};
use crate::{DataKey, LendingContract, LendingContractClient, LendingError};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn custom_params() -> RateParams {
    RateParams {
        base_rate_bps: 200,
        kink_utilization_bps: 5_000,
        multiplier_bps: 1_000,
        jump_multiplier_bps: 20_000,
        rate_floor_bps: 100,
        rate_ceiling_bps: 50_000,
        max_rate_change_per_ledger_bps: i128::MAX,
        hysteresis_bps: 0,
    }
}

#[test]
fn no_override_falls_back_to_default() {
    let (env, client, _admin) = setup();
    let asset = Address::generate(&env);

    let effective = client.get_effective_rate_params(&asset);
    assert_eq!(
        effective,
        RateParams::default(),
        "without global or override, effective must equal RateParams::default()"
    );
    assert!(
        client.get_asset_rate_params_override(&asset).is_none(),
        "override view must be None when unset"
    );
}

#[test]
fn no_override_falls_back_to_global() {
    let (env, client, _admin) = setup();
    let asset = Address::generate(&env);
    let global = custom_params();

    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::RateParams, &global);
    });

    let effective = client.get_effective_rate_params(&asset);
    assert_eq!(effective, global);
    // Rate for a fixed util must match pure compute_borrow_rate(global)
    let rate = compute_borrow_rate(4_000, &effective).unwrap();
    let expected = compute_borrow_rate(4_000, &global).unwrap();
    assert_eq!(rate, expected);
}

#[test]
fn override_supersedes_global() {
    let (env, client, _admin) = setup();
    let asset = Address::generate(&env);
    let global = RateParams::default();
    let override_params = custom_params();

    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::RateParams, &global);
    });

    client.set_asset_rate_params(&asset, &override_params);

    let effective = client.get_effective_rate_params(&asset);
    assert_eq!(effective, override_params);

    let stored = client
        .get_asset_rate_params_override(&asset)
        .expect("override must be stored");
    assert_eq!(stored, override_params);

    // Rates must follow the override curve, not the global default.
    let rate = compute_borrow_rate(4_000, &effective).unwrap();
    let rate_override = compute_borrow_rate(4_000, &override_params).unwrap();
    let rate_global = compute_borrow_rate(4_000, &global).unwrap();
    assert_eq!(rate, rate_override);
    assert_ne!(rate, rate_global);
}

#[test]
fn other_assets_unaffected_by_override() {
    let (env, client, _admin) = setup();
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);
    let override_params = custom_params();

    client.set_asset_rate_params(&asset_a, &override_params);

    assert_eq!(client.get_effective_rate_params(&asset_a), override_params);
    assert_eq!(
        client.get_effective_rate_params(&asset_b),
        RateParams::default(),
        "asset without override must still use default"
    );
}

#[test]
fn clear_override_restores_global() {
    let (env, client, _admin) = setup();
    let asset = Address::generate(&env);
    let global = custom_params();

    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::RateParams, &global);
    });

    client.set_asset_rate_params(&asset, &RateParams::default());
    assert_eq!(
        client.get_effective_rate_params(&asset),
        RateParams::default()
    );

    client.clear_asset_rate_params(&asset);
    assert!(client.get_asset_rate_params_override(&asset).is_none());
    assert_eq!(
        client.get_effective_rate_params(&asset),
        global,
        "after clear, must fall back to global"
    );
}

#[test]
fn invalid_override_rejected_floor_above_ceiling() {
    let (env, client, _admin) = setup();
    let asset = Address::generate(&env);
    let mut bad = RateParams::default();
    bad.rate_floor_bps = 5_000;
    bad.rate_ceiling_bps = 1_000;

    let res = client.try_set_asset_rate_params(&asset, &bad);
    assert!(res.is_err());
}

#[test]
fn invalid_override_rejected_kink_out_of_range() {
    let (env, client, _admin) = setup();
    let asset = Address::generate(&env);
    let mut bad = RateParams::default();
    bad.kink_utilization_bps = 10_001;

    let res = client.try_set_asset_rate_params(&asset, &bad);
    assert!(res.is_err());
}

#[test]
fn invalid_override_rejected_negative_slope() {
    let (env, client, _admin) = setup();
    let asset = Address::generate(&env);
    let mut bad = RateParams::default();
    bad.multiplier_bps = -1;

    let res = client.try_set_asset_rate_params(&asset, &bad);
    assert!(res.is_err());
}

#[test]
fn validate_rate_params_unit_checks() {
    assert_eq!(validate_rate_params(&RateParams::default()), Ok(()));

    let mut p = RateParams::default();
    p.rate_floor_bps = 100;
    p.rate_ceiling_bps = 50;
    assert_eq!(
        validate_rate_params(&p),
        Err(RateParamsValidationError::FloorAboveCeiling)
    );

    p = RateParams::default();
    p.kink_utilization_bps = -1;
    assert_eq!(
        validate_rate_params(&p),
        Err(RateParamsValidationError::KinkOutOfRange)
    );

    p = RateParams::default();
    p.jump_multiplier_bps = -5;
    assert_eq!(
        validate_rate_params(&p),
        Err(RateParamsValidationError::NegativeSlope)
    );
}

#[test]
fn pure_compute_borrow_rate_unchanged_without_override() {
    // Byte-identical rates for the default curve at several utilizations.
    let params = RateParams::default();
    for util in [0_i128, 1_000, 8_000, 10_000] {
        let rate = compute_borrow_rate(util, &params).unwrap();
        // Re-compute to ensure purity / determinism.
        assert_eq!(rate, compute_borrow_rate(util, &params).unwrap());
    }
    // Known kink value from RATE_MODEL docs / unit tests.
    assert_eq!(compute_borrow_rate(8_000, &params).unwrap(), 1_700);
}

#[test]
fn unauthorized_setter_rejected() {
    let env = Env::default();
    // Do NOT mock all auths — require real admin auth.
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    // initialize requires admin auth; mock only for init.
    env.mock_all_auths();
    client.initialize(&admin);

    // Clear mocks so subsequent calls need real auth.
    env.set_auths(&[]);

    let asset = Address::generate(&env);
    let res = client.try_set_asset_rate_params(&asset, &RateParams::default());
    assert!(
        res.is_err(),
        "set_asset_rate_params without admin auth must fail"
    );
}

// Silence unused-import warning when LendingError is only referenced in docs.
#[allow(dead_code)]
fn _type_check_error() -> LendingError {
    LendingError::InvalidAmount
}
