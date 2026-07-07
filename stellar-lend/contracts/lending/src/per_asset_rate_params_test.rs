//! Tests for the per-asset borrow-rate-params override introduced in issue #1258.
//!
//! The global borrow-rate computation ([`crate::current_borrow_rate`]) is
//! intentionally untouched; these tests prove the standalone override
//! infrastructure (storage key, resolver, validation, admin-gated entrypoint)
//! behaves correctly and isolates assets from one another.

use crate::rate_model::{validate_rate_params, RateParams, RateModelKey};
use crate::{LendingContract, LendingContractClient};
use soroban_sdk::{testutils::Ledger, Address, Env};

/// A deliberately distinct, valid per-asset curve.
fn override_params() -> RateParams {
    RateParams {
        base_rate_bps: 200,
        kink_utilization_bps: 7_000,
        multiplier_bps: 3_000,
        jump_multiplier_bps: 12_000,
        rate_floor_bps: 100,
        rate_ceiling_bps: 15_000,
        max_rate_change_per_ledger_bps: i128::MAX,
        hysteresis_bps: 0,
    }
}

/// Runs `f` inside the lending contract's storage context so instance storage
/// (where [`RateModelKey`] and [`crate::DataKey::RateParams`] live) is
/// addressable without going through the client.
fn with_contract<R>(env: &Env, f: impl FnOnce(Address) -> R) -> R {
    let contract_id = env.register(LendingContract, ());
    let c = contract_id.clone();
    env.as_contract(&c, || f(contract_id))
}

#[test]
fn validate_rate_params_accepts_valid_and_rejects_invalid() {
    let valid = RateParams::default();
    assert!(validate_rate_params(&valid), "default params must validate");

    // floor > ceiling
    let mut bad = valid.clone();
    bad.rate_floor_bps = 500;
    bad.rate_ceiling_bps = 100;
    assert!(!validate_rate_params(&bad), "floor>ceiling must be rejected");

    // kink above the 0..=10000 range
    let mut bad = valid.clone();
    bad.kink_utilization_bps = 12_000;
    assert!(!validate_rate_params(&bad), "kink>10000 must be rejected");

    // negative slope
    let mut bad = valid.clone();
    bad.multiplier_bps = -1;
    assert!(!validate_rate_params(&bad), "negative slope must be rejected");

    // negative bound
    let mut bad = valid.clone();
    bad.rate_floor_bps = -5;
    assert!(!validate_rate_params(&bad), "negative bound must be rejected");
}

#[test]
fn resolver_falls_back_to_global_default_without_override() {
    let env = Env::default();
    with_contract(&env, |_id| {
        let asset = Address::generate(&env);
        let eff = crate::rate_model::get_effective_rate_params(&env, &asset);
        // No override stored -> global default (which equals RateParams::default
        // when the global has never been set).
        assert_eq!(eff, RateParams::default());
    });
}

#[test]
fn resolver_returns_per_asset_override_and_isolates_assets() {
    let env = Env::default();
    with_contract(&env, |_id| {
        let asset_a = Address::generate(&env);
        let asset_b = Address::generate(&env);

        crate::rate_model::set_asset_rate_params(&env, &asset_a, &override_params());

        // asset_a sees its override
        assert_eq!(
            crate::rate_model::get_effective_rate_params(&env, &asset_a),
            override_params()
        );
        // asset_b is untouched and still resolves to the global default
        assert_eq!(
            crate::rate_model::get_effective_rate_params(&env, &asset_b),
            RateParams::default()
        );
        // The override must be persisted under the exact AssetParams(asset_a) key.
        let stored: Option<RateParams> = env
            .storage()
            .instance()
            .get(&RateModelKey::AssetParams(asset_a.clone()));
        assert_eq!(stored, Some(override_params()));
    });
}

#[test]
fn entrypoint_admin_sets_and_resolves_override_for_one_asset_only() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let asset_a = Address::generate(&env);
    let asset_b = Address::generate(&env);
    client.initialize(&admin);

    // Before any override, both assets resolve to the global default.
    assert_eq!(
        client.get_effective_rate_params(&asset_a),
        RateParams::default()
    );

    // Admin installs an override for asset_a only.
    client.set_asset_rate_params(&asset_a, &override_params());

    // asset_a now resolves to the override; asset_b remains default.
    assert_eq!(
        client.get_effective_rate_params(&asset_a),
        override_params()
    );
    assert_eq!(
        client.get_effective_rate_params(&asset_b),
        RateParams::default()
    );
}

#[test]
#[should_panic]
fn entrypoint_rejects_invalid_params_with_panic() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    client.initialize(&admin);

    let mut bad = override_params();
    bad.rate_floor_bps = 900;
    bad.rate_ceiling_bps = 100; // floor > ceiling -> invalid
    client.set_asset_rate_params(&asset, &bad);
}
