#![cfg(test)]

//! Executable specification: **collateral parameters × max borrow**, including oracle-priced
//! cross-asset positions.
//!
//! # Formula — single-asset `borrow_asset` (hello-world borrow module)
//!
//! Let `C` = posted collateral (native units), `cf` = collateral factor (basis points),
//! `R` = minimum collateral ratio from risk params (default **15_000** = 150%).
//!
//! ```text
//! collateral_value = C * cf / 10_000
//! max_total_debt   = collateral_value * 10_000 / R
//! max_new_borrow   = max_total_debt - (principal_debt + accrued_interest)
//! ```
//!
//! The borrow asset `Option<Address>` selects **which [`AssetParams`][`crate::deposit::AssetParams`]
//! row** supplies `cf`. Native collateral with `borrow_asset(..., &Some(token), ...)` applies
//! that token’s `collateral_factor` to the **same** global collateral balance.
//!
//! # Formula — cross-asset registry (`cross_asset` module) + oracle price
//!
//! Prices are USD-normalized with **7** decimals (`10_000_000` = \$1.00). Let `P = 10^7`.
//!
//! ```text
//! collateral_value_usd = collateral_amount * price / P
//! ```
//!
//! **Health** uses the **liquidation threshold** (basis points), not `collateral_factor`, when
//! weighting collateral for `get_user_position_summary` / borrow checks:
//!
//! ```text
//! weighted_collateral_usd = collateral_value_usd * liquidation_threshold / 10_000
//! debt_value_usd          = total_debt_amount * price / P   (per-asset leg)
//! health_factor           = weighted_collateral_usd * 10_000 / weighted_debt_usd
//! ```
//!
//! A `cross_asset_borrow` succeeds only if the post-tx `health_factor >= 10_000` (1.0).
//! Invariants require `collateral_factor <= liquidation_threshold`; both are admin-configured
//! with basis-point bounds checks.
//!
//! # Security & trust (summary)
//!
//! - **Oracle / prices**: Cross-asset module stores admin-updated prices and rejects stale
//!   prices for non-zero positions (see `cross_asset::get_user_position_summary`). Production
//!   deployments should assume a trusted updater or verified feeds.
//! - **Authorization**: `borrow_asset` and `cross_asset_borrow` require the user to authorize;
//!   tests use `Env::mock_all_auths()`.
//! - **Reentrancy**: `borrow_asset` takes a reentrancy guard before state changes; cross-asset
//!   borrow performs no external token calls in the current implementation.
//! - **Token transfers**: Under `cfg(test)`, ERC-20-style transfers for `Some(asset)` borrows
//!   are skipped so unit tests do not require deployed token contracts.
//!
//! # References
//!
//! - Implementation: [`crate::borrow::calculate_max_borrowable`], [`crate::cross_asset::get_user_position_summary`].
//! - User-facing notes: `COLLATERAL_FACTOR_MAX_BORROW.md` in this crate.

use crate::cross_asset::AssetConfig;
use crate::deposit::{DepositDataKey, Position};
use crate::{deposit, HelloContract, HelloContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Map, Symbol};

const MIN_COLLATERAL_RATIO_BPS: i128 = 15_000;

fn env_with_auth() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn set_borrow_asset_params(
    env: &Env,
    contract_id: &Address,
    asset: &Address,
    deposit_enabled: bool,
    collateral_factor: i128,
    max_deposit: i128,
) {
    use deposit::AssetParams;
    let params = AssetParams {
        deposit_enabled,
        collateral_factor,
        max_deposit,
        borrow_fee_bps: 0,
    };
    env.as_contract(contract_id, || {
        let key = DepositDataKey::AssetParams(asset.clone());
        env.storage().persistent().set(&key, &params);
    });
}

fn set_pause_borrow(env: &Env, contract_id: &Address, paused: bool) {
    env.as_contract(contract_id, || {
        let pause_key = DepositDataKey::PauseSwitches;
        let mut pause_map = Map::new(env);
        pause_map.set(Symbol::new(env, "pause_borrow"), paused);
        env.storage().persistent().set(&pause_key, &pause_map);
    });
}

fn position(env: &Env, contract_id: &Address, user: &Address) -> Position {
    env.as_contract(contract_id, || {
        let key = DepositDataKey::Position(user.clone());
        env.storage()
            .persistent()
            .get::<DepositDataKey, Position>(&key)
            .unwrap()
    })
}

/// `borrow_asset` max borrow: `floor(C * cf / 10_000 * 10_000 / MIN_COLLATERAL_RATIO_BPS)`.
fn expected_max_borrow_single_asset(collateral: i128, collateral_factor_bps: i128) -> i128 {
    collateral
        .checked_mul(collateral_factor_bps)
        .and_then(|v| v.checked_div(10_000))
        .and_then(|v| v.checked_mul(10_000))
        .and_then(|v| v.checked_div(MIN_COLLATERAL_RATIO_BPS))
        .unwrap_or(0)
}

fn default_ca_config(env: &Env) -> AssetConfig {
    AssetConfig {
        asset: None,
        collateral_factor: 7500,
        liquidation_threshold: 8000,
        reserve_factor: 1000,
        max_supply: 1_000_000_0000000,
        max_borrow: 500_000_0000000,
        can_collateralize: true,
        can_borrow: true,
        price: 10_000_000,
        price_updated_at: env.ledger().timestamp(),
    }
}

// --- Single-asset `borrow_asset` + `AssetParams.collateral_factor` ---

#[test]
fn spec_borrow_with_token_asset_applies_collateral_factor_to_max() {
    let env = env_with_auth();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let borrow_asset = Address::generate(&env);

    set_borrow_asset_params(&env, &contract_id, &borrow_asset, true, 7500, 0);

    let collateral = 2000;
    client.deposit_collateral(&user, &None, &collateral);

    let max = expected_max_borrow_single_asset(collateral, 7500);
    assert_eq!(max, 1000, "2000 * 0.75 * 10_000 / 15_000 = 1000");

    client.borrow_asset(&user, &Some(borrow_asset.clone()), &max);
    assert_eq!(position(&env, &contract_id, &user).debt, max);
}

#[test]
fn spec_borrow_boundary_factors_50_75_100_percent() {
    let env = env_with_auth();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);

    let collateral = 3000;

    for (cf_bps, expected_max) in [(5000, 1000), (7500, 1500), (10000, 2000)] {
        let token = Address::generate(&env);
        set_borrow_asset_params(&env, &contract_id, &token, true, cf_bps, 0);

        let u = Address::generate(&env);
        client.deposit_collateral(&u, &None, &collateral);
        let max = expected_max_borrow_single_asset(collateral, cf_bps);
        assert_eq!(
            max, expected_max,
            "cf={cf_bps}: max borrow should match formula"
        );
        client.borrow_asset(&u, &Some(token), &max);
        assert_eq!(position(&env, &contract_id, &u).debt, max);
    }
}

#[test]
fn spec_borrow_one_over_max_fails_single_asset() {
    let env = env_with_auth();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let token = Address::generate(&env);

    set_borrow_asset_params(&env, &contract_id, &token, true, 7500, 0);
    client.deposit_collateral(&user, &None, &2000);

    let max = expected_max_borrow_single_asset(2000, 7500);
    assert!(client
        .try_borrow_asset(&user, &Some(token), &(max + 1))
        .is_err());
}

#[test]
fn spec_borrow_paused_rejects_even_if_under_max() {
    let env = env_with_auth();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let token = Address::generate(&env);

    set_borrow_asset_params(&env, &contract_id, &token, true, 10_000, 0);
    set_pause_borrow(&env, &contract_id, true);

    client.deposit_collateral(&user, &None, &2000);
    assert!(client.try_borrow_asset(&user, &Some(token), &100).is_err());
}

// --- Cross-asset + oracle (price on `AssetConfig`) ---

#[test]
fn spec_cross_asset_max_borrow_matches_liquidation_threshold_weighting() {
    let env = env_with_auth();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    client.initialize_ca(&admin);

    let mut cfg = default_ca_config(&env);
    // Make math obvious: LT 80%, $1 price, 10_000 units collateral → max debt value $8_000 → 8_000 units.
    cfg.liquidation_threshold = 8000;
    cfg.collateral_factor = 7500;
    client.initialize_asset(&None, &cfg);

    client.cross_asset_deposit(&user, &None, &10_000_0000000);
    client.cross_asset_borrow(&user, &None, &8000_0000000);

    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.health_factor, 10_000);
}

#[test]
fn spec_cross_asset_collateral_usd_doubles_when_oracle_price_doubles() {
    let env = env_with_auth();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    client.initialize_ca(&admin);

    let mut cfg = default_ca_config(&env);
    cfg.liquidation_threshold = 8000;
    cfg.collateral_factor = 7500;
    cfg.price = 10_000_000;
    client.initialize_asset(&None, &cfg);

    client.cross_asset_deposit(&user, &None, &10_000_0000000);
    let before = client.get_user_position_summary(&user);

    client.update_asset_price(&None, &20_000_000);
    let after = client.get_user_position_summary(&user);

    assert_eq!(
        after.total_collateral_value,
        before.total_collateral_value * 2,
        "oracle price feeds linearly into USD collateral value"
    );
    assert_eq!(
        after.weighted_collateral_value,
        before.weighted_collateral_value * 2
    );

    // At 2× price, weighted collateral is 2×; borrow up to prior max debt USD (now 2× in notional capacity).
    client.cross_asset_borrow(&user, &None, &8000_0000000);
    assert_eq!(client.get_user_position_summary(&user).health_factor, 10_000);
}

#[test]
fn spec_cross_asset_two_assets_independent_factors_and_prices() {
    let env = env_with_auth();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);
    client.initialize_ca(&admin);

    let mut native = default_ca_config(&env);
    native.collateral_factor = 7500;
    native.liquidation_threshold = 8000;
    native.price = 10_000_000;
    client.initialize_asset(&None, &native);

    let token = Address::generate(&env);
    let mut token_cfg = default_ca_config(&env);
    token_cfg.asset = Some(token.clone());
    token_cfg.collateral_factor = 6000;
    token_cfg.liquidation_threshold = 7000;
    token_cfg.price = 20_000_000;
    client.initialize_asset(&Some(token.clone()), &token_cfg);

    const PRICE_PRECISION: i128 = 10_000_000;
    let native_amt = 10_000_0000000_i128;
    let token_amt = 5_000_0000000_i128;
    let native_cv = native_amt
        .saturating_mul(native.price)
        .saturating_div(PRICE_PRECISION);
    let token_cv = token_amt
        .saturating_mul(token_cfg.price)
        .saturating_div(PRICE_PRECISION);
    let expected_weighted = native_cv.saturating_mul(8000).saturating_div(10_000)
        + token_cv.saturating_mul(7000).saturating_div(10_000);

    client.cross_asset_deposit(&user, &None, &native_amt);
    client.cross_asset_deposit(&user, &Some(token.clone()), &token_amt);

    let summary = client.get_user_position_summary(&user);
    assert_eq!(summary.total_collateral_value, native_cv + token_cv);
    assert_eq!(summary.weighted_collateral_value, expected_weighted);

    // At health = 1.0: weighted_debt == weighted_collateral (single debt leg, unweighted debt).
    let max_borrow_native = expected_weighted
        .saturating_mul(PRICE_PRECISION)
        .saturating_div(native.price);
    client.cross_asset_borrow(&user, &None, &max_borrow_native);
    assert_eq!(
        client.get_user_position_summary(&user).health_factor,
        10_000
    );
}
