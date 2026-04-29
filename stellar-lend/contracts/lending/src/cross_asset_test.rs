//! Cross-asset lending module tests.
//!
//! Coverage:
//! - Admin initialisation
//! - Asset parameter configuration (valid and invalid)
//! - Deposit / borrow / repay operations
//! - Basic health-factor checks
//!
//! NOTE: `withdraw_asset` is not tested here because it performs a real token
//! transfer that requires a deployed token contract. Those scenarios live in
//! integration tests that set up a mock token.

use crate::cross_asset::{AssetParams, CrossAssetError};
use crate::{LendingContract, LendingContractClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

// ─────────────────────────────────────────────────────────────────────────────
// Constants that mirror cross_asset.rs internals
// ─────────────────────────────────────────────────────────────────────────────

/// BPS_SCALE = 10_000; health factor ≥ this value is considered healthy.
const HF_HEALTHY: i128 = 10_000;
/// Sentinel health factor when the position carries no debt.
const HF_NO_DEBT: i128 = 1_000_000;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn setup(env: &Env) -> (LendingContractClient<'_>, Address) {
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin, &1_000_000_000, &1000);
    client.initialize_admin(&admin);
    (client, admin)
}

/// Build an `AssetParams` with the given LTV. Liquidation threshold = LTV + 500 bps (capped at 10 000).
fn asset_params(env: &Env, ltv: i128) -> AssetParams {
    let threshold = (ltv + 500).min(10_000);
    AssetParams {
        ltv,
        liquidation_threshold: threshold,
        price_feed: Address::generate(env),
        debt_ceiling: 1_000_000_000_000,
        is_active: true,
    }
}

/// Create a default valid asset config for testing.
fn default_config(env: &Env) -> AssetParams {
    AssetParams {
        asset: None,
        collateral_factor: 7500,        // 75% LTV
        liquidation_threshold: 8000,    // 80%
        reserve_factor: 1000,           // 10%
        max_supply: 10_000_000_000_000, // 1M (7 decimals)
        max_borrow: 5_000_000_000_000,  // 500K
        can_collateralize: true,
        can_borrow: true,
        borrow_factor: 10000,
        price: 10_000_000, // $1.00 (7 decimals)
        price_updated_at: env.ledger().timestamp(),
    }
}

/// Create a token-backed asset config for testing.
fn token_config(env: &Env, addr: &Address) -> AssetParams {
    let price = 20_000_000;
    AssetParams {
        asset: Some(addr.clone()),
        collateral_factor: 6000,     // 60% LTV
        liquidation_threshold: 7000, // 70%
        reserve_factor: 2000,        // 20%
        max_supply: 5_000_000_000_000,
        max_borrow: 2_500_000_000_000,
        can_collateralize: true,
        can_borrow: true,
        borrow_factor: 10000,
        price,
        price_updated_at: env.ledger().timestamp(),
    }
}

/// Set up env + contract + admin, initialize both modules.
fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.initialize_ca(&admin);
    (env, client, admin)
>>>>>>> origin
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Asset parameter configuration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_set_asset_params_success() {
    let env = Env::default();
    env.mock_all_auths();
let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    let params = asset_params(&env, 7500);
    client.set_asset_params(&asset, &params);
}

#[test]
fn test_set_asset_params_stores_and_allows_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    let params = asset_params(&env, 7500);
    client.set_asset_params(&asset, &params);
}

// ============================================================================
// 2. Asset Initialization
// ============================================================================

#[test]
fn test_initialize_asset_success() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    let fetched = client.get_asset_config(&None);
    assert_eq!(fetched.collateral_factor, 7500);
    assert_eq!(fetched.liquidation_threshold, 8000);
    assert_eq!(fetched.price, 10_000_000);
}

#[test]
fn test_initialize_token_asset_success() {
    let (env, client, _admin) = setup();
    let token_addr = Address::generate(&env);
    let config = token_config(&env, &token_addr);
    client.initialize_asset(&Some(token_addr.clone()), &config);

    let fetched = client.get_asset_config(&Some(token_addr));
    assert_eq!(fetched.collateral_factor, 6000);
    assert_eq!(fetched.price, 20_000_000);
}

#[test]
#[should_panic]
fn test_initialize_asset_twice_fails() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);
    // Re-initialization should fail
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_invalid_ltv_above_10000() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.collateral_factor = 10_001; // Out of bounds
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_negative_ltv() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.collateral_factor = -1;
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_ltv_exceeds_liquidation_threshold() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.collateral_factor = 9000;
    config.liquidation_threshold = 8000; // LTV > threshold
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_zero_price() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.price = 0;
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_negative_price() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.price = -5;
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_negative_max_supply() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.max_supply = -100;
    client.initialize_asset(&None, &config);
}

#[test]
#[should_panic]
fn test_initialize_asset_invalid_reserve_factor() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.reserve_factor = 10_001;
    client.initialize_asset(&None, &config);
}

#[test]
fn test_initialize_asset_zero_caps_unlimited() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.max_supply = 0; // unlimited
    config.max_borrow = 0; // unlimited
    client.initialize_asset(&None, &config);

    let fetched = client.get_asset_config(&None);
    assert_eq!(fetched.max_supply, 0);
    assert_eq!(fetched.max_borrow, 0);
}

#[test]
fn test_initialize_asset_edge_ltv_equals_threshold() {
    let (env, client, _admin) = setup();
    let mut config = default_config(&env);
    config.collateral_factor = 8000;
    config.liquidation_threshold = 8000; // Equal is allowed
    client.initialize_asset(&None, &config);
}

// ============================================================================
// 3. Config Updates
// ============================================================================

#[test]
fn test_update_asset_config_success() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    client.update_asset_config(
        &None,
        &Some(6000), // new LTV
        &Some(7000), // new threshold
        &None,
        &None,
        &None,
        &None,
    );

    let fetched = client.get_asset_config(&None);
    assert_eq!(fetched.collateral_factor, 6000);
    assert_eq!(fetched.liquidation_threshold, 7000);
    // Unchanged fields preserved
    assert_eq!(fetched.reserve_factor, 1000);
    assert!(fetched.can_collateralize);
}

#[test]
fn test_update_asset_config_partial_update() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    // Only update can_borrow
    client.update_asset_config(&None, &None, &None, &None, &None, &None, &Some(false));

    let fetched = client.get_asset_config(&None);
    assert!(!fetched.can_borrow);
    assert_eq!(fetched.collateral_factor, 7500); // Unchanged
}

#[test]
#[should_panic]
fn test_update_asset_config_ltv_above_threshold_fails() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    // Try to set LTV > current threshold (8000)
    client.update_asset_config(
        &None,
        &Some(9000), // LTV 90% > threshold 80%
        &None,       // Keep threshold at 8000
        &None,
        &None,
        &None,
        &None,
    );
}

#[test]
#[should_panic]
fn test_update_asset_config_out_of_bounds_fails() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    client.update_asset_config(
        &None,
        &Some(10_001), // Out of bounds
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );
}

#[test]
#[should_panic]
fn test_update_asset_config_unconfigured_asset_fails() {
    let (_env, client, _admin) = setup();
    // Asset not initialized
    client.update_asset_config(&None, &Some(5000), &None, &None, &None, &None, &None);
}

// ============================================================================
// 4. Price Updates
// ============================================================================

#[test]
fn test_update_asset_price_success() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    client.update_asset_price(&None, &20_000_000); // $2.00

    let fetched = client.get_asset_config(&None);
    assert_eq!(fetched.price, 20_000_000);
}

    let asset = Address::generate(&env);
    let params = asset_params(&env, 7500);
    client.set_asset_params(&asset, &params);
}

#[test]
fn test_set_asset_params_stores_and_allows_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

>>>>>>> origin
    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &1_000);
}

#[test]
fn test_deposit_on_inactive_asset_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    let params = AssetParams {
        ltv: 7500,
        liquidation_threshold: 8000,
        price_feed: Address::generate(&env),
        debt_ceiling: 1_000_000_000,
        is_active: false, // disabled
    };
    client.set_asset_params(&asset, &params);

    let user = Address::generate(&env);
    let result = client.try_deposit_collateral_asset(&user, &asset, &1_000);
    assert_eq!(result, Err(Ok(CrossAssetError::AssetNotSupported)));
}

#[test]
fn test_deposit_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    let result = client.try_deposit_collateral_asset(&user, &asset, &0);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_deposit_negative_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    let result = client.try_deposit_collateral_asset(&user, &asset, &-100);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Borrow operations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_borrow_on_unknown_asset_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    let user = Address::generate(&env);
    let result = client.try_borrow_asset(&user, &asset, &100);
    assert_eq!(result, Err(Ok(CrossAssetError::AssetNotSupported)));
}

#[test]
fn test_borrow_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    let result = client.try_borrow_asset(&user, &asset, &0);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_borrow_without_collateral_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    let result = client.try_borrow_asset(&user, &asset, &500);
    assert_eq!(result, Err(Ok(CrossAssetError::InsufficientCollateral)));
}

#[test]
fn test_borrow_exceeds_health_factor_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    // LTV 7500 → max borrow = collateral * 0.75
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &10_000);

    // Borrow more than weighted collateral allows (> 7500)
    let result = client.try_borrow_asset(&user, &asset, &8_000);
    assert_eq!(result, Err(Ok(CrossAssetError::InsufficientCollateral)));
}

#[test]
fn test_borrow_at_exact_capacity_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    // LTV 7500 → max borrow for 10_000 collateral = 7500
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &10_000);
    client.borrow_asset(&user, &asset, &7_500);
}

#[test]
fn test_borrow_exceeds_debt_ceiling_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    let params = AssetParams {
        ltv: 9000,
        liquidation_threshold: 9500,
        price_feed: Address::generate(&env),
        debt_ceiling: 100, // very small ceiling
        is_active: true,
    };
    client.set_asset_params(&asset, &params);

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &10_000);

    let result = client.try_borrow_asset(&user, &asset, &101);
    assert_eq!(result, Err(Ok(CrossAssetError::DebtCeilingReached)));
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Repay operations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_repay_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    let result = client.try_repay_asset(&user, &asset, &0);
    assert_eq!(result, Err(Ok(CrossAssetError::InvalidAmount)));
}

#[test]
fn test_repay_reduces_debt() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &10_000);
    client.borrow_asset(&user, &asset, &5_000);

    // Repay 2000; debt should fall to 3000.
    client.repay_asset(&user, &asset, &2_000);

    let summary = client.get_cross_position_summary(&user);
    // debt_value = 3000 (price = $1 scaled)
    assert_eq!(summary.total_debt_usd, 3_000);
}

#[test]
fn test_repay_overpay_capped_at_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &10_000);
    client.borrow_asset(&user, &asset, &5_000);

    // Repay more than the outstanding debt — should be capped at 5000.
    client.repay_asset(&user, &asset, &999_999);

    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_debt_usd, 0);
    assert_eq!(summary.health_factor, HF_NO_DEBT);
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Position summary – basic invariants
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_summary_empty_position_has_zero_totals() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let user = Address::generate(&env);
    let summary = client.get_cross_position_summary(&user);

    assert_eq!(summary.total_collateral_usd, 0);
    assert_eq!(summary.total_debt_usd, 0);
    assert_eq!(summary.health_factor, HF_NO_DEBT);
}

#[test]
fn test_summary_collateral_only_has_max_health_factor() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    client.set_asset_params(&asset, &asset_params(&env, 7500));

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &10_000);

    let summary = client.get_cross_position_summary(&user);
    // No debt → sentinel health factor
    assert_eq!(summary.health_factor, HF_NO_DEBT);
    // Collateral value: 10_000 * 10_000_000 / 10_000_000 = 10_000
    assert_eq!(summary.total_collateral_usd, 10_000);
    assert_eq!(summary.total_debt_usd, 0);
}

#[test]
fn test_summary_debt_only_position_reflects_uncollateralised_state() {
    // A user can end up with debt > 0 but collateral = 0 only through
    // a test shortcut (direct state manipulation). The summary must still
    // be internally consistent: if collateral_usd == 0 the health factor
    // formula yields 0 (weighted_collateral=0 → 0 * scale / debt = 0).
    // We verify the formula works with very small collateral instead.
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let asset = Address::generate(&env);
    // LTV = 10_000 (100%) to maximise borrow capacity
    client.set_asset_params(&asset, &asset_params(&env, 10_000));

    let user = Address::generate(&env);
    client.deposit_collateral_asset(&user, &asset, &1);
    client.borrow_asset(&user, &asset, &1);

    let summary = client.get_cross_position_summary(&user);
    assert_eq!(summary.total_collateral_usd, 1);
    assert_eq!(summary.total_debt_usd, 1);
    // weighted = 1 * 10_000 / 10_000 = 1; HF = 1 * 10_000 / 1 = 10_000
    assert_eq!(summary.health_factor, HF_HEALTHY);
}

#[test]
fn test_config_update_preserves_price() {
    let (env, client, _admin) = setup();
    let config = default_config(&env);
    client.initialize_asset(&None, &config);

    client.update_asset_price(&None, &50_000_000); // $5.00

    client.update_asset_config(&None, &Some(5000), &Some(6000), &None, &None, &None, &None);

    let fetched = client.get_asset_config(&None);
    assert_eq!(fetched.price, 50_000_000); // Price preserved
    assert_eq!(fetched.collateral_factor, 5000);
}
