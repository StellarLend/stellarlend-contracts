//! Tests for `update_asset_config` access-control and bounds validation.
//!
//! Covers the hardening added in `closes #<issue>`:
//! - Admin-only enforcement (non-admin callers are rejected).
//! - LTV (`collateral_factor_bps`) must not exceed `liquidation_threshold`.
//! - `collateral_factor_bps` must be in `[0, 10_000]` (reject > 100 %).
//! - `price_decimals` must not be zero.
//! - Valid updates are applied and a `ConfigUpdatedEvent` is emitted.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::cross_asset::{
    get_asset_config_by_address, initialize_asset, set_admin, update_asset_config, AssetConfig,
    CrossAssetError,
};

// ---------------------------------------------------------------------------
// Helpers
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

/// Default valid config: factor 7500 bps (75 %), threshold 8000 bps (80 %).
fn default_config() -> AssetConfig {
    AssetConfig {
        collateral_factor_bps: 7_500,
        liquidation_threshold: 8_000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        price: 1_000_000,
        price_decimals: 6,
    }
}

// ---------------------------------------------------------------------------
// Access-control: non-admin callers are rejected
// ---------------------------------------------------------------------------

/// A caller that has not been registered as admin must be rejected.
#[test]
fn test_update_rejects_non_admin() {
    let env = make_env();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let non_admin = Address::generate(&env);

        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        let r = update_asset_config(
            &env, &non_admin, None, None, None, None, None, None, None, None,
        );
        assert_eq!(r, Err(CrossAssetError::Unauthorized));

        // Config on disk is unchanged.
        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.collateral_factor_bps, 7_500);
    });
}

/// When no admin has been set yet, every caller must be rejected.
#[test]
fn test_update_rejects_when_no_admin_set() {
    let env = make_env();
    with_contract(&env, || {
        initialize_asset(&env, None, default_config()).unwrap();
        let caller = Address::generate(&env);
        let r = update_asset_config(
            &env, &caller, None, None, None, None, None, None, None, None,
        );
        assert_eq!(r, Err(CrossAssetError::Unauthorized));
    });
}

// ---------------------------------------------------------------------------
// Bounds: LTV > liquidation_threshold
// ---------------------------------------------------------------------------

/// Setting `collateral_factor_bps` above the current `liquidation_threshold`
/// must be rejected with `LtvExceedsThreshold`.
#[test]
fn test_update_rejects_ltv_above_threshold() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        // Factor 8001 > threshold 8000 → reject.
        let r = update_asset_config(
            &env,
            &admin,
            None,
            Some(8_001),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(r, Err(CrossAssetError::LtvExceedsThreshold));

        // Config on disk is unchanged.
        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.collateral_factor_bps, 7_500);
    });
}

/// Simultaneously lowering the threshold below the current factor must also
/// be rejected — the invariant check uses the *post-update* config.
#[test]
fn test_update_rejects_threshold_below_current_factor() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        // Drop threshold to 7000 without changing factor (7500 > 7000) → reject.
        let r = update_asset_config(
            &env,
            &admin,
            None,
            None,
            Some(7_000),
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(r, Err(CrossAssetError::LtvExceedsThreshold));
    });
}

/// Edge: factor == threshold is valid (position starts exactly at the liquidation
/// boundary but is not under-water).
#[test]
fn test_update_accepts_ltv_equal_to_threshold() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        // Set factor == threshold (8000 == 8000) → accept.
        let r = update_asset_config(
            &env,
            &admin,
            None,
            Some(8_000),
            Some(8_000),
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(r, Ok(()));

        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.collateral_factor_bps, 8_000);
        assert_eq!(cfg.liquidation_threshold, 8_000);
    });
}

// ---------------------------------------------------------------------------
// Bounds: collateral_factor_bps > 10_000 (LTV > 100 %)
// ---------------------------------------------------------------------------

/// `collateral_factor_bps` above 10 000 must be rejected with
/// `InvalidCollateralFactor` before the LTV-vs-threshold check.
#[test]
fn test_update_rejects_ltv_above_100_pct() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(
            &env,
            None,
            AssetConfig {
                liquidation_threshold: 10_000,
                ..default_config()
            },
        )
        .unwrap();

        let r = update_asset_config(
            &env,
            &admin,
            None,
            Some(10_001),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(r, Err(CrossAssetError::InvalidCollateralFactor));
    });
}

/// Negative `collateral_factor_bps` is also `InvalidCollateralFactor`.
#[test]
fn test_update_rejects_negative_factor() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        let r = update_asset_config(
            &env,
            &admin,
            None,
            Some(-1),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_eq!(r, Err(CrossAssetError::InvalidCollateralFactor));
    });
}

// ---------------------------------------------------------------------------
// Bounds: price_decimals == 0
// ---------------------------------------------------------------------------

/// `price_decimals` of zero is a misconfiguration.
#[test]
fn test_update_rejects_zero_decimals() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        let r = update_asset_config(
            &env,
            &admin,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(0),
        );
        assert_eq!(r, Err(CrossAssetError::ZeroDecimals));
    });
}

/// `price_decimals` above 38 must be rejected with `InvalidDecimals`.
#[test]
fn test_update_rejects_decimals_above_38() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        let r = update_asset_config(
            &env,
            &admin,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(39),
        );
        assert_eq!(r, Err(CrossAssetError::InvalidDecimals));
    });
}

// ---------------------------------------------------------------------------
// Valid update: config is persisted
// ---------------------------------------------------------------------------

/// A fully valid update must be accepted and persisted.
#[test]
fn test_update_valid_persists_changes() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        let r = update_asset_config(
            &env,
            &admin,
            None,
            Some(6_000),
            Some(9_000),
            Some(1_000_000),
            Some(500_000),
            Some(false),
            Some(true),
            Some(8),
        );
        assert_eq!(r, Ok(()));

        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.collateral_factor_bps, 6_000);
        assert_eq!(cfg.liquidation_threshold, 9_000);
        assert_eq!(cfg.max_supply, 1_000_000);
        assert_eq!(cfg.max_borrow, 500_000);
        assert!(!cfg.can_collateralize);
        assert!(cfg.can_borrow);
        assert_eq!(cfg.price_decimals, 8);
    });
}

/// A `None`-only update must succeed and leave the config unchanged.
#[test]
fn test_update_all_none_is_noop() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        update_asset_config(&env, &admin, None, None, None, None, None, None, None, None).unwrap();

        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.collateral_factor_bps, 7_500);
        assert_eq!(cfg.liquidation_threshold, 8_000);
        assert_eq!(cfg.price_decimals, 6);
    });
}

// ---------------------------------------------------------------------------
// Event emission on success
// ---------------------------------------------------------------------------

/// A successful update must emit exactly one `ConfigUpdatedEvent`.
#[test]
fn test_update_emits_config_updated_event() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        update_asset_config(
            &env,
            &admin,
            None,
            Some(6_000),
            Some(9_000),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(env.events().all().len(), 1);
    });
}

/// A rejected update must not emit any event.
#[test]
fn test_failed_update_emits_no_event() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);
        initialize_asset(&env, None, default_config()).unwrap();

        let _ = update_asset_config(
            &env,
            &admin,
            None,
            Some(9_001),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(env.events().all().len(), 0);
    });
}

/// Existing assets not touched by `update_asset_config` retain their config.
#[test]
fn test_update_preserves_other_assets() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        set_admin(&env, &admin);

        let other_asset = Address::generate(&env);
        initialize_asset(&env, None, default_config()).unwrap();
        initialize_asset(&env, Some(other_asset.clone()), default_config()).unwrap();

        update_asset_config(
            &env,
            &admin,
            None,
            Some(5_000),
            Some(8_000),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let other_cfg = get_asset_config_by_address(&env, Some(other_asset)).unwrap();
        assert_eq!(other_cfg.collateral_factor_bps, 7_500);
    });
}
