//! Per-asset debt ceiling tests for the cross-asset lending module.
//!
//! These tests verify that the `max_borrow` cap on each asset is enforced
//! independently: hitting the ceiling on one asset must not affect borrowing
//! capacity on other assets, and the ceiling is correctly tracked as users
//! borrow and repay.
//!
//! ## Key scenario (issue #519)
//! "Borrow fails when asset ceiling hit while others have room."

#![cfg(test)]

use crate::cross_asset::{AssetConfig, CrossAssetError};
use crate::{HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

// ============================================================================
// Helpers
// ============================================================================

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

/// Set up env + contract with both admin modules initialized.
fn setup_protocol(env: &Env) -> (HelloContractClient<'static>, Address) {
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    client.initialize_ca(&admin);
    (client, admin)
}

/// Native XLM collateral-only config (no borrow cap, unlimited supply).
fn xlm_collateral_config(env: &Env) -> AssetConfig {
    AssetConfig {
        asset: None,
        collateral_factor: 8000,
        liquidation_threshold: 8000,
        reserve_factor: 1000,
        max_supply: 0,      // unlimited
        max_borrow: 0,      // unlimited (XLM is collateral only in these tests)
        can_collateralize: true,
        can_borrow: false,
        price: 1_0000000,   // $1.00
        price_updated_at: env.ledger().timestamp(),
    }
}

/// Token borrow config with an explicit debt ceiling.
fn token_borrow_config(env: &Env, addr: &Address, price: i128, max_borrow: i128) -> AssetConfig {
    AssetConfig {
        asset: Some(addr.clone()),
        collateral_factor: 8000,
        liquidation_threshold: 8000,
        reserve_factor: 1000,
        max_supply: 0,
        max_borrow,
        can_collateralize: false,
        can_borrow: true,
        borrow_factor: 7000,
        price,
        price_updated_at: env.ledger().timestamp(),
    }
}

// ============================================================================
// 1. Basic cap enforcement
// ============================================================================

/// A borrow that would push total borrows past max_borrow must fail.
#[test]
fn test_borrow_cap_enforcement() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    // XLM as collateral (unlimited)
    client.initialize_asset(&None, &xlm_collateral_config(&env));

    // USDC with a 1 000-unit borrow cap
    let usdc_cfg = token_borrow_config(&env, &usdc, 1_0000000, 1000);
    client.initialize_asset(&Some(usdc.clone()), &usdc_cfg);

    // User 1 deposits XLM collateral and borrows 600 USDC
    client.cross_asset_deposit(&user1, &None, &5000);
    client.cross_asset_borrow(&user1, &Some(usdc.clone()), &600);

    // User 2 deposits collateral and tries to borrow 500 USDC
    // Total would be 600 + 500 = 1 100 > cap of 1 000 → must fail
    client.cross_asset_deposit(&user2, &None, &5000);
    let result = client.try_cross_asset_borrow(&user2, &Some(usdc.clone()), &500);
    assert!(
        result.is_err(),
        "borrow should fail when adding to existing borrows would exceed cap"
    );
}

/// Admin can raise the cap after it was hit, unblocking further borrows.
#[test]
fn test_borrow_cap_update_via_admin() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 500));

    client.cross_asset_deposit(&user, &None, &5000);

    // Initialize Native XLM as a cross-asset instrument so health checks work
    let xlm_config = create_asset_config(&env, None, 1_0000000, 0);
    client.initialize_asset(&None, &xlm_config);

    // User 1 deposits collateral (Native XLM) via cross-asset deposit
    client.cross_asset_deposit(&user1, &None, &5000);

    // Admin raises cap to 1 000
    client.update_asset_config(
        &Some(usdc.clone()),
        &None,
        &None,
        &None,
        &Some(1000),
        &None,
        &None,
    );

    // User 2 deposits collateral
    client.cross_asset_deposit(&user2, &None, &5000);

// ============================================================================
// 2. Per-asset isolation: "ceiling hit while others have room" (issue #519)
// ============================================================================

/// Hitting asset A's ceiling must not affect asset B's borrowing.
#[test]
fn test_borrow_cap_asset_a_full_asset_b_still_available() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let dai = Address::generate(&env);
    let user = Address::generate(&env);

    // XLM collateral
    client.initialize_asset(&None, &xlm_collateral_config(&env));
    // USDC: cap 1 000
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 1000));
    // DAI: cap 2 000 (has room)
    client.initialize_asset(&Some(dai.clone()), &token_borrow_config(&env, &dai, 1_0000000, 2000));

    // Large collateral so the health factor is not the bottleneck
    client.cross_asset_deposit(&user, &None, &100_000);

    // Fill USDC cap exactly
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &1000);

    // Borrow of any more USDC must fail (cap exhausted)
    let usdc_result = client.try_cross_asset_borrow(&user, &Some(usdc.clone()), &1);
    assert!(
        usdc_result.is_err(),
        "USDC borrow should fail: cap is exhausted"
    );

    // Borrow of DAI must still succeed (different asset, different cap)
    let dai_result = client.try_cross_asset_borrow(&user, &Some(dai.clone()), &500);
    assert!(
        dai_result.is_ok(),
        "DAI borrow should succeed: DAI cap still has room"
    );
}

/// Two separate users both borrowing against the same asset ceiling share the cap.
#[test]
fn test_borrow_cap_shared_across_users() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 1000));

    // Three users each deposit collateral
    for u in [&user1, &user2, &user3] {
        client.cross_asset_deposit(u, &None, &5000);
    }

    // User 1 borrows 400, user 2 borrows 400 → total 800
    client.cross_asset_borrow(&user1, &Some(usdc.clone()), &400);
    client.cross_asset_borrow(&user2, &Some(usdc.clone()), &400);

    // Remaining cap: 200. User 3 borrows 200 → total hits 1 000 exactly
    client.cross_asset_borrow(&user3, &Some(usdc.clone()), &200);

    // Now cap is full; any further borrow must fail
    let result = client.try_cross_asset_borrow(&user1, &Some(usdc.clone()), &1);
    assert!(result.is_err(), "cap is exhausted; no more borrows allowed");

    let total = client.get_total_borrow_for(&Some(usdc.clone()));
    assert_eq!(total, 1000, "total borrows must equal the cap");
}

/// Repaying reduces total borrows, opening cap space for new borrows.
#[test]
fn test_borrow_cap_repay_frees_capacity() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 1000));

    client.cross_asset_deposit(&user, &None, &5000);

    // Fill cap
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &1000);

    // Cannot borrow more
    assert!(
        client
            .try_cross_asset_borrow(&user, &Some(usdc.clone()), &1)
            .is_err()
    );

    // Repay 300 → total drops to 700
    client.cross_asset_repay(&user, &Some(usdc.clone()), &300);

    let after_repay = client.get_total_borrow_for(&Some(usdc.clone()));
    assert_eq!(after_repay, 700);

    // Now 300 of cap is free; borrow 300 should succeed
    assert!(
        client
            .try_cross_asset_borrow(&user, &Some(usdc.clone()), &300)
            .is_ok(),
        "cap space freed by repayment must allow new borrows"
    );
}

/// max_borrow = 0 means unlimited; any amount should succeed (health factor permitting).
#[test]
fn test_borrow_cap_zero_means_unlimited() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    // max_borrow = 0 → no cap
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 0));

    client.cross_asset_deposit(&user, &None, &100_000);

    // Borrow a large amount — only health factor should limit this
    let result = client.try_cross_asset_borrow(&user, &Some(usdc.clone()), &50_000);
    assert!(result.is_ok(), "unlimited cap: borrow should succeed");
}

/// Borrowing exactly at the ceiling must succeed; one unit above must fail.
#[test]
fn test_borrow_cap_exact_boundary() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 1000));

    client.cross_asset_deposit(&user, &None, &5000);
    client.cross_asset_deposit(&user2, &None, &5000);

    // Borrow exactly at cap
    let at_cap = client.try_cross_asset_borrow(&user, &Some(usdc.clone()), &1000);
    assert!(at_cap.is_ok(), "borrow equal to cap must succeed");

    // One unit above cap
    let above_cap = client.try_cross_asset_borrow(&user2, &Some(usdc.clone()), &1);
    assert!(above_cap.is_err(), "borrow above cap must fail");
}

// ============================================================================
// 3. Total borrow accounting correctness
// ============================================================================

/// `get_total_borrow_for` must track borrows accurately across multiple users.
#[test]
fn test_total_borrow_tracking() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 5000));

    client.cross_asset_deposit(&user1, &None, &5000);
    client.cross_asset_deposit(&user2, &None, &5000);

    assert_eq!(client.get_total_borrow_for(&Some(usdc.clone())), 0);

    client.cross_asset_borrow(&user1, &Some(usdc.clone()), &300);
    assert_eq!(client.get_total_borrow_for(&Some(usdc.clone())), 300);

    client.cross_asset_borrow(&user2, &Some(usdc.clone()), &200);
    assert_eq!(client.get_total_borrow_for(&Some(usdc.clone())), 500);

    client.cross_asset_repay(&user1, &Some(usdc.clone()), &100);
    assert_eq!(client.get_total_borrow_for(&Some(usdc.clone())), 400);
}

/// Total borrow on asset A is unaffected by borrow/repay activity on asset B.
#[test]
fn test_total_borrow_isolation_between_assets() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let dai = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 5000));
    client.initialize_asset(&Some(dai.clone()), &token_borrow_config(&env, &dai, 1_0000000, 5000));

    client.cross_asset_deposit(&user, &None, &20_000);

    // Initialize Native XLM as a cross-asset instrument so health checks work
    let xlm_config = create_asset_config(&env, None, 1_0000000, 0);
    client.initialize_asset(&None, &xlm_config);

    client.cross_asset_deposit(&user, &None, &5000);

    // Repay DAI should not change USDC total
    client.cross_asset_repay(&user, &Some(dai.clone()), &300);
    assert_eq!(
        client.get_total_borrow_for(&Some(usdc.clone())),
        500,
        "USDC total must not change when DAI is repaid"
    );
    assert_eq!(
        client.get_total_borrow_for(&Some(dai.clone())),
        0,
        "DAI total must reflect repayment"
    );
}

// ============================================================================
// 4. Admin can lower cap below current outstanding debt (existing debt stands)
// ============================================================================

/// When admin lowers cap below outstanding borrows, existing positions are
/// unaffected but new borrows are blocked.
#[test]
fn test_borrow_cap_lowered_below_current_debt_blocks_new_borrows() {
    let env = create_test_env();
    let (client, _admin) = setup_protocol(&env);

    let usdc = Address::generate(&env);
    let user = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.initialize_asset(&None, &xlm_collateral_config(&env));
    client.initialize_asset(&Some(usdc.clone()), &token_borrow_config(&env, &usdc, 1_0000000, 2000));

    client.cross_asset_deposit(&user, &None, &5000);
    client.cross_asset_deposit(&user2, &None, &5000);

    // Borrow 800 (well under cap)
    client.cross_asset_borrow(&user, &Some(usdc.clone()), &800);

    // Admin lowers cap to 500 (below current outstanding of 800)
    client.update_asset_config(
        &Some(usdc.clone()),
        &None,       // cf
        &None,       // lt
        &None,       // max_supply
        &Some(1000), // max_borrow
        &None,       // can_collateralize
        &None,       // can_borrow
        &None,       // borrow_factor
    );

    // Existing user can still repay (that reduces total, eventually re-opening capacity)
    client.cross_asset_repay(&user, &Some(usdc.clone()), &400);
    // total is now 400 < cap 500, so small new borrow should pass
    let after_repay = client.try_cross_asset_borrow(&user2, &Some(usdc.clone()), &50);
    assert!(
        after_repay.is_ok(),
        "borrow should succeed after repayment brings total under new cap"
    );
}
