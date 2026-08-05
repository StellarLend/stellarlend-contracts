//! Tests for `update_asset_price` access-control and authorization.
//!
//! Verifies the security fix for issue #1686:
//! - Admin-only enforcement (non-admin callers are rejected).
//! - Authorization check happens before any state modification.
//! - Valid price updates are applied when authorized.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::cross_asset::{
    get_asset_config_by_address, initialize, initialize_asset, update_asset_price, AssetConfig,
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

/// Default valid config for testing.
fn default_config(price: i128, price_decimals: u32) -> AssetConfig {
    AssetConfig {
        collateral_factor_bps: 7_500,
        liquidation_threshold: 8_000,
        max_supply: 0,
        max_borrow: 0,
        can_collateralize: true,
        can_borrow: true,
        price,
        price_decimals,
        last_update_ts: 0,
    }
}

// ---------------------------------------------------------------------------
// Access-control: non-admin callers are rejected
// ---------------------------------------------------------------------------

/// A caller that is not the admin must be rejected.
#[test]
fn test_price_update_rejects_non_admin() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let non_admin = Address::generate(&env);

        initialize(&env, admin.clone()).unwrap();
        initialize_asset(&env, &admin, None, default_config(1_000_000, 6)).unwrap();

        // Non-admin cannot update price
        let r = update_asset_price(&env, &non_admin, None, 2_000_000);
        assert_eq!(r, Err(CrossAssetError::Unauthorized));

        // Price on disk is unchanged
        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.price, 1_000_000);
    });
}

/// When no admin has been set, every caller must be rejected.
#[test]
fn test_price_update_rejects_when_no_admin_set() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        // No admin set — any update_asset_price call must fail.
        let caller = Address::generate(&env);
        let r = update_asset_price(&env, &caller, None, 2_000_000);
        assert_eq!(r, Err(CrossAssetError::Unauthorized));
    });
}

// ---------------------------------------------------------------------------
// Valid updates: authorized admin can update price
// ---------------------------------------------------------------------------

/// Admin can successfully update price.
#[test]
fn test_price_update_succeeds_with_admin() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let initial_price = 1_000_000;
        let new_price = 2_000_000;

        initialize(&env, admin.clone()).unwrap();
        initialize_asset(&env, &admin, None, default_config(initial_price, 6)).unwrap();

        // Admin updates price
        let r = update_asset_price(&env, &admin, None, new_price);
        assert!(r.is_ok());

        // Price on disk is updated
        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.price, new_price);
    });
}

/// Admin can update price for multiple assets independently.
#[test]
fn test_price_update_multiple_assets() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let token_a = Address::generate(&env);
        let token_b = Address::generate(&env);

        initialize(&env, admin.clone()).unwrap();
        initialize_asset(&env, &admin, Some(token_a.clone()), default_config(100, 6)).unwrap();
        initialize_asset(&env, &admin, Some(token_b.clone()), default_config(50, 6)).unwrap();

        // Update token A price
        update_asset_price(&env, &admin, Some(token_a.clone()), 200).unwrap();

        // Verify token A updated, token B unchanged
        let cfg_a = get_asset_config_by_address(&env, Some(token_a)).unwrap();
        let cfg_b = get_asset_config_by_address(&env, Some(token_b)).unwrap();
        assert_eq!(cfg_a.price, 200);
        assert_eq!(cfg_b.price, 50);
    });
}

// ---------------------------------------------------------------------------
// Validation: price bounds are enforced
// ---------------------------------------------------------------------------

/// Price must be positive (non-zero and non-negative).
#[test]
fn test_price_update_rejects_zero() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        initialize(&env, admin.clone()).unwrap();
        initialize_asset(&env, &admin, None, default_config(1_000_000, 6)).unwrap();

        let r = update_asset_price(&env, &admin, None, 0);
        assert_eq!(r, Err(CrossAssetError::InvalidAmount));

        // Price on disk is unchanged
        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.price, 1_000_000);
    });
}

/// Price must be positive (negative is not allowed).
#[test]
fn test_price_update_rejects_negative() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        initialize(&env, admin.clone()).unwrap();
        initialize_asset(&env, &admin, None, default_config(1_000_000, 6)).unwrap();

        let r = update_asset_price(&env, &admin, None, -1_000_000);
        assert_eq!(r, Err(CrossAssetError::InvalidAmount));

        // Price on disk is unchanged
        let cfg = get_asset_config_by_address(&env, None).unwrap();
        assert_eq!(cfg.price, 1_000_000);
    });
}

/// Authorization check happens before price validation.
/// This ensures attackers cannot learn about the state by trying different prices.
#[test]
fn test_unauthorized_rejected_before_validation() {
    let env = make_env();
    env.mock_all_auths();
    with_contract(&env, || {
        let admin = Address::generate(&env);
        let non_admin = Address::generate(&env);

        initialize(&env, admin).unwrap();
        initialize_asset(&env, &admin, None, default_config(1_000_000, 6)).unwrap();

        // Non-admin tries to set invalid price (zero)
        // Should get Unauthorized, not InvalidAmount
        let r = update_asset_price(&env, &non_admin, None, 0);
        assert_eq!(r, Err(CrossAssetError::Unauthorized));
    });
}
