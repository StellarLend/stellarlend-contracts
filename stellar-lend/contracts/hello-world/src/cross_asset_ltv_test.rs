//! Tests for per-asset collateral-factor (LTV) tiering.
//!
//! `closes #1121` — each `AssetConfig` carries a `collateral_factor_bps`
//! field that weights the asset's contribution to total borrow capacity.
//! Factor is bounded to `[0, 10_000]`, validated at initialization and
//! update time. Mixed-factor portfolios, zero-factor and full-factor
//! edge cases are exercised below.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::cross_asset::{
    cross_asset_borrow, cross_asset_deposit, cross_asset_repay, get_asset_config_by_address,
    get_user_position_summary, initialize_asset, update_asset_config, AssetConfig, CrossAssetError,
    MAX_COLLATERAL_FACTOR_BPS,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    Env::default()
}

/// Soroban storage requires an active contract context.
fn with_contract<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce() -> T,
{
    let contract_id = env.register(crate::cross_asset::NoOpContract {}, ());
    env.as_contract(&contract_id, f)
}

fn asset_config(price: i128, price_decimals: u32, factor_bps: i128) -> AssetConfig {
    AssetConfig {
        collateral_factor_bps: factor_bps,
        liquidation_threshold: 8000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_decimals,
    }
}

fn borrow_only_asset_config(price: i128, price_decimals: u32, factor_bps: i128) -> AssetConfig {
    AssetConfig {
        collateral_factor_bps: factor_bps,
        liquidation_threshold: 8000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: false,
        can_borrow: true,
        price,
        price_decimals,
    }
}

fn collateral_only_asset_config(
    price: i128,
    price_decimals: u32,
    factor_bps: i128,
) -> AssetConfig {
    AssetConfig {
        collateral_factor_bps: factor_bps,
        liquidation_threshold: 8000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: false,
        price,
        price_decimals,
    }
}

// ---------------------------------------------------------------------------
// Initialization / update: factor bounds validation
// ---------------------------------------------------------------------------

/// `collateral_factor_bps` below 0 must be rejected at the init call.
#[test]
fn test_init_rejects_negative_factor() {
    let env = make_env();
    let result = initialize_asset(
        &env,
        None,
        asset_config(1_000_000, 6, -1),
    );
    assert_eq!(result, Err(CrossAssetError::InvalidCollateralFactor));
}

/// `collateral_factor_bps` above 10_000 must be rejected at the init call.
#[test]
fn test_init_rejects_factor_above_max() {
    let env = make_env();
    let result = initialize_asset(
        &env,
        None,
        asset_config(1_000_000, 6, MAX_COLLATERAL_FACTOR_BPS + 1),
    );
    assert_eq!(result, Err(CrossAssetError::InvalidCollateralFactor));
}

/// Boundary: factor = 0 is accepted (zero-capacity tier).
#[test]
fn test_init_accepts_zero_factor_boundary() {
    let env = make_env();
    with_contract(&env, || {
        let result = initialize_asset(&env, None, asset_config(1_000_000, 6, 0));
        assert_eq!(result, Ok(()));
    });
}

/// Boundary: factor = 10_000 is accepted (100 % LTV — full-fraction tier).
#[test]
fn test_init_accepts_max_factor_boundary() {
    let env = make_env();
    with_contract(&env, || {
        let result = initialize_asset(&env, None, asset_config(1_000_000, 6, 10_000));
        assert_eq!(result, Ok(()));
    });
}

/// `update_asset_config` rejects out-of-range factor changes.
#[test]
fn test_update_rejects_out_of_range_factor() {
    let env = make_env();
    with_contract(&env, || {
        initialize_asset(&env, None, asset_config(1_000_000, 6, 7500)).unwrap();

        // 10_001 → reject
        let r = update_asset_config(&env, None, Some(MAX_COLLATERAL_FACTOR_BPS + 1), None, None, None, None, None);
        assert_eq!(r, Err(CrossAssetError::InvalidCollateralFactor));

        // −1 → reject
        let r = update_asset_config(&env, None, Some(-1), None, None, None, None, None);
        assert_eq!(r, Err(CrossAssetError::InvalidCollateralFactor));

        // unchanged on disk
        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.collateral_factor_bps, 7500);
    });
}

/// `update_asset_config` applies a valid factor change on the next summary
/// computation.
#[test]
fn test_update_factor_takes_effect_immediately() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);

    with_contract(&env, || {
        // Collateral at 50 % LTV, $1 per unit. Borrow asset also $1 per unit.
        initialize_asset(&env, None, asset_config(1_000_000, 6, 5_000)).unwrap();
        initialize_asset(&env, Some(borrow_asset.clone()), borrow_only_asset_config(1_000_000, 6, 5_000))
            .unwrap();

        cross_asset_deposit(&env, user.clone(), None, 100).unwrap();

        // 100 × 50 % = 50 capacity.
        let s = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s.total_collateral_value, 100);
        assert_eq!(s.borrow_capacity, 50);

        // Cut factor to 25 % — capacity should drop to 25.
        update_asset_config(&env, None, Some(2_500), None, None, None, None, None).unwrap();
        let s2 = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s2.borrow_capacity, 25);
        assert_eq!(s2.total_collateral_value, 100);

        // Raise factor to 80 % — capacity rises to 80.
        update_asset_config(&env, None, Some(8_000), None, None, None, None, None).unwrap();
        let s3 = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s3.borrow_capacity, 80);
    });
}

// ---------------------------------------------------------------------------
// Borrow capacity math
// ---------------------------------------------------------------------------

/// Full-factor asset (10_000 bps) contributes the entire collateral value.
/// Regression property: the existing "asset at full factor" math must be
/// preserved bit-for-bit.
#[test]
fn test_full_factor_no_regression() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);

    with_contract(&env, || {
        initialize_asset(
            &env,
            None,
            asset_config(1_000_000, 6, 10_000), // 100 %
        )
        .unwrap();
        initialize_asset(
            &env,
            Some(borrow_asset.clone()),
            borrow_only_asset_config(1_000_000, 6, 10_000),
        )
        .unwrap();

        cross_asset_deposit(&env, user.clone(), None, 100).unwrap();

        let summary = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(summary.total_collateral_value, 100);
        assert_eq!(summary.borrow_capacity, 100); // 100 × 100 % = 100
        assert_eq!(summary.is_healthy, 1);

        // Borrowing up to capacity succeeds; one above fails.
        cross_asset_borrow(&env, user.clone(), Some(borrow_asset.clone()), 100).unwrap();
        let s = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s.is_healthy, 1);

        cross_asset_repay(&env, user.clone(), Some(borrow_asset.clone()), 100).unwrap();
        let r = cross_asset_borrow(&env, user.clone(), Some(borrow_asset.clone()), 101);
        assert_eq!(r, Err(CrossAssetError::InsufficientCollateral));
    });
}

/// Zero-factor asset: deposit 1000 units, capacity remains 0.
#[test]
fn test_zero_factor_asset_no_capacity() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);

    with_contract(&env, || {
        initialize_asset(
            &env,
            None,
            collateral_only_asset_config(1_000_000, 6, 0), // 0 %
        )
        .unwrap();
        initialize_asset(
            &env,
            Some(borrow_asset.clone()),
            borrow_only_asset_config(1_000_000, 6, 10_000),
        )
        .unwrap();

        cross_asset_deposit(&env, user.clone(), None, 1000).unwrap();

        let s = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s.total_collateral_value, 1000);
        assert_eq!(s.borrow_capacity, 0); // weighted by 0 %

        // Any borrow attempt → InsufficientCollateral.
        let r = cross_asset_borrow(&env, user.clone(), Some(borrow_asset.clone()), 1);
        assert_eq!(r, Err(CrossAssetError::InsufficientCollateral));
    });
}

/// Mixed portfolio: two collateral assets at different tiers. The capacity
/// is the sum of per-asset factor-weighted contributions.
#[test]
fn test_mixed_factor_portfolio() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let blue_chip = Address::generate(&env); // 90 % LTV
    let long_tail = Address::generate(&env); // 40 % LTV
    let borrow_asset = Address::generate(&env);

    with_contract(&env, || {
        // Blue-chip: $1 / unit, 90 %
        initialize_asset(
            &env,
            None,
            asset_config(1_000_000, 6, 9_000),
        )
        .unwrap();
        // Long-tail: $1 / unit, 40 %
        initialize_asset(
            &env,
            Some(blue_chip.clone()),
            asset_config(1_000_000, 6, 4_000),
        )
        .unwrap();
        // Borrow asset (configured to allow borrow)
        initialize_asset(
            &env,
            Some(borrow_asset.clone()),
            borrow_only_asset_config(1_000_000, 6, 0),
        )
        .unwrap();

        // Note: we deposit using the `None` (Native) slot for the first
        // collateral, and a Token slot for the second. Both treat collateral
        // symmetrically.
        cross_asset_deposit(&env, user.clone(), None, 100).unwrap();
        cross_asset_deposit(&env, user.clone(), Some(blue_chip.clone()), 100).unwrap();

        let s = get_user_position_summary(&env, &user).unwrap();
        // total_collateral_value = 100 + 100 = 200
        assert_eq!(s.total_collateral_value, 200);
        // capacity = (100 × 9000 / 10_000) + (100 × 4000 / 10_000) = 90 + 40 = 130
        assert_eq!(s.borrow_capacity, 130);

        // Borrow 130 → exactly at capacity → healthy.
        cross_asset_borrow(&env, user.clone(), Some(borrow_asset.clone()), 130).unwrap();
        let sb = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(sb.is_healthy, 1);

        // Repay and attempt 131 → one over capacity → rejected.
        cross_asset_repay(&env, user.clone(), Some(borrow_asset.clone()), 130).unwrap();
        let r = cross_asset_borrow(&env, user.clone(), Some(borrow_asset.clone()), 131);
        assert_eq!(r, Err(CrossAssetError::InsufficientCollateral));
    });
}

/// Mixed factors demonstrate that riskier collateral does *not* dilute a
/// blue-chip asset's contribution: we compute exactly the linear weighted
/// sum and verify it against an arithmetic reference.
#[test]
fn test_factor_weighting_arithmetic_reference() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);

    with_contract(&env, || {
        // Three collateral assets with prices $1, $2, $5 at 80 %, 60 %, 20 %.
        initialize_asset(
            &env,
            None,
            collateral_only_asset_config(1_000_000, 6, 8_000), // $1, 80 %
        )
        .unwrap();
        let token_b = Address::generate(&env);
        initialize_asset(
            &env,
            Some(token_b.clone()),
            collateral_only_asset_config(2_000_000, 6, 6_000), // $2, 60 %
        )
        .unwrap();
        let token_c = Address::generate(&env);
        initialize_asset(
            &env,
            Some(token_c.clone()),
            collateral_only_asset_config(5_000_000, 6, 2_000), // $5, 20 %
        )
        .unwrap();
        // Borrow side.
        initialize_asset(
            &env,
            Some(borrow_asset.clone()),
            borrow_only_asset_config(1_000_000, 6, 0),
        )
        .unwrap();

        // Deposit 100 units each → values: 100×$1=100, 50×$2=100, 20×$5=100.
        cross_asset_deposit(&env, user.clone(), None, 100).unwrap();
        cross_asset_deposit(&env, user.clone(), Some(token_b.clone()), 50).unwrap();
        cross_asset_deposit(&env, user.clone(), Some(token_c.clone()), 20).unwrap();

        let s = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s.total_collateral_value, 300);

        // capacity = (100 × 8000 + 100 × 6000 + 100 × 2000) / 10_000
        //          = (800_000 + 600_000 + 200_000) / 10_000
        //          = 1_600_000 / 10_000
        //          = 160
        assert_eq!(s.borrow_capacity, 160);

        // Borrow exactly 160: healthy.
        cross_asset_borrow(&env, user.clone(), Some(borrow_asset.clone()), 160).unwrap();
        let sb = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(sb.is_healthy, 1);

        // Repay, try 161 → over capacity.
        cross_asset_repay(&env, user.clone(), Some(borrow_asset.clone()), 160).unwrap();
        let r = cross_asset_borrow(&env, user.clone(), Some(borrow_asset.clone()), 161);
        assert_eq!(r, Err(CrossAssetError::InsufficientCollateral));
    });
}

/// Factor affects only capacity, not raw `total_collateral_value`.
/// This isolates the two aggregation quantities for reviewer clarity.
#[test]
fn test_factor_does_not_change_total_collateral_value() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);

    with_contract(&env, || {
        // Same collateral at 100 % vs 0 %.
        initialize_asset(&env, None, asset_config(1_000_000, 6, 10_000)).unwrap();
        cross_asset_deposit(&env, user.clone(), None, 7).unwrap();

        let s_full = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s_full.total_collateral_value, 7);
        assert_eq!(s_full.borrow_capacity, 7);

        // Flip to 0 %.
        update_asset_config(&env, None, Some(0), None, None, None, None, None).unwrap();
        let s_zero = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s_zero.total_collateral_value, 7); // unchanged
        assert_eq!(s_zero.borrow_capacity, 0); // weighted to zero
    });
}

/// Boundary precision: 1 bp asset contributes floor(collateral×1/10_000)
/// capacity. With collateral value 100 and factor 50 bps → floor(5/1) = 5.
#[test]
fn test_factor_50bps_small_collateral() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);

    with_contract(&env, || {
        initialize_asset(&env, None, asset_config(1_000_000, 6, 50)).unwrap();
        cross_asset_deposit(&env, user.clone(), None, 100).unwrap();
        let s = get_user_position_summary(&env, &user).unwrap();
        // 100 × 50 / 10_000 = 0  (integer division)
        assert_eq!(s.borrow_capacity, 0);
    });
}
