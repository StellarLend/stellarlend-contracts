//! Tests for per-asset oracle-decimal normalization in cross_asset value aggregation.
//!
//! Issue #1122: assets with different `price_decimals` must be normalised to
//! the shared internal scale before summation so position valuations are
//! correct regardless of oracle feed precision.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::cross_asset::{
    cross_asset_borrow, cross_asset_deposit, cross_asset_repay, get_user_position_summary,
    initialize_asset, normalize_price, normalize_price_ceil, update_asset_price, AssetConfig,
    CrossAssetError,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_env() -> Env {
    Env::default()
}

/// Soroban storage requires an active contract context.
/// We register a minimal contract-id to run storage-backed logic.
fn with_contract<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce() -> T,
{
    let contract_id = env.register(crate::cross_asset::NoOpContract {}, ());
    env.as_contract(&contract_id, f)
}

fn default_config(price: i128, price_decimals: u32) -> AssetConfig {
    AssetConfig {
        collateral_factor: 7500, // 75 %
        liquidation_threshold: 8000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_decimals,
    }
}

// ---------------------------------------------------------------------------
// Unit tests: normalize_price / normalize_price_ceil
// ---------------------------------------------------------------------------

#[test]
fn test_normalize_same_decimals() {
    // No conversion needed when decimals match INTERNAL_DECIMALS.
    let price: i128 = 1_000_000_000_000_000_000; // 1.0 at 18 dp
    assert_eq!(normalize_price(price, 18), Some(price));
    assert_eq!(normalize_price_ceil(price, 18), Some(price));
}

#[test]
fn test_normalize_6_to_18_decimals() {
    // 6-decimal oracle price 1_000_000 == $1.00 → normalised 10^18
    let raw: i128 = 1_000_000; // $1.00 at 6 dp
    let expected: i128 = 1_000_000_000_000_000_000; // $1.00 at 18 dp
    assert_eq!(normalize_price(raw, 6), Some(expected));
    assert_eq!(normalize_price_ceil(raw, 6), Some(expected));
}

#[test]
fn test_normalize_8_to_18_decimals() {
    // 8-decimal (e.g. BTC feed): large prices overflow at 18-dp internal scale.
    // $1.00 at 8 dp = 100_000_000 → 1_000_000_000_000_000_000 at 18 dp.
    let raw_1: i128 = 100_000_000; // $1.00 at 8 dp
    let expected_1: i128 = 1_000_000_000_000_000_000; // $1.00 at 18 dp
    assert_eq!(normalize_price(raw_1, 8), Some(expected_1));
    assert_eq!(normalize_price_ceil(raw_1, 8), Some(expected_1));
}

#[test]
fn test_normalize_18_to_6_floor_vs_ceil() {
    // Downsizing: 18-dp price to 6-dp internal would only happen if
    // INTERNAL_DECIMALS were smaller; here we test asset_decimals > 18.
    // asset_decimals = 20 (hypothetical): raw 123_456_789 at dp=20
    // floor:  123_456_789 / 100 = 1_234_567 (remainder 89 discarded)
    // ceil:   (123_456_789 + 99) / 100 = 1_234_568
    let raw: i128 = 123_456_789;
    assert_eq!(normalize_price(raw, 20), Some(1_234_567));
    assert_eq!(normalize_price_ceil(raw, 20), Some(1_234_568));
}

#[test]
fn test_normalize_exact_multiple_no_rounding_diff() {
    // When the raw value is an exact multiple the floor and ceil agree.
    let raw: i128 = 200; // asset_decimals=20, scale=100 → 200/100 = 2
    assert_eq!(normalize_price(raw, 20), Some(2));
    assert_eq!(normalize_price_ceil(raw, 20), Some(2));
}

#[test]
fn test_normalize_overflow_guard() {
    // Very large raw price + upscaling must return None rather than panic.
    let raw: i128 = i128::MAX;
    // asset_decimals=6 → multiply by 10^12; i128::MAX * 10^12 overflows.
    assert_eq!(normalize_price(raw, 6), None);
    assert_eq!(normalize_price_ceil(raw, 6), None);
}

#[test]
fn test_normalize_zero_price() {
    assert_eq!(normalize_price(0, 6), Some(0));
    assert_eq!(normalize_price_ceil(0, 6), Some(0));
    assert_eq!(normalize_price(0, 18), Some(0));
}

// ---------------------------------------------------------------------------
// Integration tests: position summary with mixed decimals
// ---------------------------------------------------------------------------

/// Register two assets (6-dp and 18-dp) each priced at $1.00, deposit equal
/// amounts, and verify the position summary is the same as if both used the
/// same decimal scale.
#[test]
fn test_position_summary_equal_usd_different_decimals() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let token_b = Address::generate(&env);

    with_contract(&env, || {
        // Asset A: 6-decimal feed, $1.00 → price = 1_000_000
        initialize_asset(
            &env,
            None, // Native slot for asset A
            default_config(1_000_000, 6),
        )
        .unwrap();

        // Asset B: 18-decimal feed, $1.00 → price = 1_000_000_000_000_000_000
        initialize_asset(
            &env,
            Some(token_b.clone()),
            default_config(1_000_000_000_000_000_000, 18),
        )
        .unwrap();

        // Deposit 1 unit of each (raw token units = 1).
        cross_asset_deposit(&env, user.clone(), None, 1).unwrap();
        cross_asset_deposit(&env, user.clone(), Some(token_b.clone()), 1).unwrap();

        let summary = get_user_position_summary(&env, &user).unwrap();

        // Each $1 deposit should contribute 1 * price_normalised / 10^18 = 1 unit of collateral value.
        // total_collateral_value = 1 + 1 = 2  (in 18-dp internal units ÷ 10^18 = 2 dollars)
        assert_eq!(summary.total_collateral_value, 2);
        assert_eq!(summary.total_debt_value, 0);
        assert_eq!(summary.is_healthy, 1);
    });
}

/// Mixed decimals: deposit collateral at 6-dp oracle, borrow at 18-dp oracle.
/// Position must be correctly assessed as healthy or underwater.
#[test]
fn test_borrow_health_check_mixed_decimals() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let token_b = Address::generate(&env);

    with_contract(&env, || {
        // Collateral asset: 6-dp, $2.00 per unit, collateral_factor = 7500 (75 %)
        initialize_asset(
            &env,
            None,
            AssetConfig {
                collateral_factor: 7500,
                liquidation_threshold: 8000,
                max_supply: 0,
                max_borrow: 0,
                can_collateralize: true,
                can_borrow: false,
                price: 2_000_000, // $2.00 at 6 dp
                price_decimals: 6,
            },
        )
        .unwrap();

        // Borrow asset: 18-dp, $1.00 per unit
        initialize_asset(
            &env,
            Some(token_b.clone()),
            AssetConfig {
                collateral_factor: 7500,
                liquidation_threshold: 8000,
                max_supply: 0,
                max_borrow: 0,
                can_collateralize: false,
                can_borrow: true,
                price: 1_000_000_000_000_000_000, // $1.00 at 18 dp
                price_decimals: 18,
            },
        )
        .unwrap();

        // Deposit 10 units of collateral → $20 collateral value.
        // borrow_capacity = 20 * 75% = 15.
        cross_asset_deposit(&env, user.clone(), None, 10).unwrap();

        // Borrow 14 units of debt asset → $14 debt value. Should be healthy.
        cross_asset_borrow(&env, user.clone(), Some(token_b.clone()), 14).unwrap();
        let summary = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(
            summary.is_healthy, 1,
            "14 < 15 borrow capacity, should be healthy"
        );

        // Repay everything and try to borrow 16 — should fail (exceeds capacity).
        cross_asset_repay(&env, user.clone(), Some(token_b.clone()), 14).unwrap();
        let result = cross_asset_borrow(&env, user.clone(), Some(token_b.clone()), 16);
        assert_eq!(result, Err(CrossAssetError::InsufficientCollateral));
    });
}

/// Regression: when all assets share the same price_decimals the behaviour is
/// identical to the previous (un-normalised) semantics.
#[test]
fn test_no_regression_same_decimals() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    with_contract(&env, || {
        // Both assets use 18-decimal feeds, $1.00 price.
        for tok in [token_a.clone(), token_b.clone()] {
            initialize_asset(
                &env,
                Some(tok),
                default_config(1_000_000_000_000_000_000, 18),
            )
            .unwrap();
        }

        cross_asset_deposit(&env, user.clone(), Some(token_a.clone()), 5).unwrap();
        cross_asset_deposit(&env, user.clone(), Some(token_b.clone()), 5).unwrap();

        let summary = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(summary.total_collateral_value, 10);
        assert_eq!(summary.is_healthy, 1);
    });
}

/// Edge case: initialize_asset rejects price_decimals > 38.
#[test]
fn test_invalid_decimals_rejected() {
    let env = make_env();
    let result = initialize_asset(
        &env,
        None,
        AssetConfig {
            price_decimals: 39,
            ..default_config(1, 18)
        },
    );
    assert_eq!(result, Err(CrossAssetError::InvalidDecimals));
}

/// Price update works and the new price is reflected in position summary.
#[test]
fn test_price_update_reflected_in_summary() {
    let env = make_env();
    env.mock_all_auths();
    let user = Address::generate(&env);

    with_contract(&env, || {
        initialize_asset(&env, None, default_config(1_000_000, 6)).unwrap();
        cross_asset_deposit(&env, user.clone(), None, 10).unwrap();

        // Initially $1.00 each → collateral = 10.
        let s1 = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s1.total_collateral_value, 10);

        // Double the price to $2.00.
        update_asset_price(&env, None, 2_000_000).unwrap();
        let s2 = get_user_position_summary(&env, &user).unwrap();
        assert_eq!(s2.total_collateral_value, 20);
    });
}
