//! Tests for the governable close-factor and liquidation-incentive risk
//! parameters introduced for issue #1027.
//!
//! `LendingContract::liquidate` previously hard-coded `CLOSE_FACTOR = 5000`
//! and `INCENTIVE_BPS = 1000` as inline `const` literals, and duplicated the
//! top-level `LIQUIDATION_THRESHOLD_BPS` constant as a local `8000` literal.
//! These tests cover the new `DataKey::CloseFactorBps` /
//! `DataKey::LiquidationIncentiveBps` storage, their admin-gated setters and
//! bounds, the public getters (including default fallthrough when unset),
//! and that `liquidate` actually sources its close-factor cap and incentive
//! from the governed values rather than from recompiled constants.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

use crate::{
    debt::DebtPosition, DataKey, LendingContract, LendingContractClient, LendingError,
    DEFAULT_CLOSE_FACTOR_BPS, DEFAULT_LIQUIDATION_INCENTIVE_BPS, MAX_LIQUIDATION_INCENTIVE_BPS,
};

fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, cid, admin)
}

/// Seed a borrower's `(collateral, debt)` position directly into storage.
///
/// This bypasses `deposit`/`borrow` (which independently enforce their own
/// solvency gate at borrow time) so a liquidatable position can be set up
/// directly, mirroring the approach already used by
/// `liquidate_close_factor_test.rs`.
fn seed_position(env: &Env, cid: &Address, borrower: &Address, collateral: i128, debt: i128) {
    let now = env.ledger().timestamp();
    env.as_contract(cid, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &collateral);
        env.storage().persistent().set(
            &DataKey::Debt(borrower.clone()),
            &DebtPosition {
                principal: debt,
                last_update: now,
            },
        );
    });
}

// ─── Default fallthrough ──────────────────────────────────────────────────

#[test]
fn close_factor_bps_defaults_when_unset() {
    let (_env, client, ..) = setup();
    assert_eq!(client.get_close_factor_bps(), DEFAULT_CLOSE_FACTOR_BPS);
    assert_eq!(DEFAULT_CLOSE_FACTOR_BPS, 5_000);
}

#[test]
fn liquidation_incentive_bps_defaults_when_unset() {
    let (_env, client, ..) = setup();
    assert_eq!(
        client.get_liquidation_incentive_bps(),
        DEFAULT_LIQUIDATION_INCENTIVE_BPS
    );
    assert_eq!(DEFAULT_LIQUIDATION_INCENTIVE_BPS, 1_000);
}

// ─── Setters: boundary acceptance ─────────────────────────────────────────

#[test]
fn set_close_factor_bps_accepts_lower_boundary() {
    let (_env, client, ..) = setup();
    client.set_close_factor_bps(&1);
    assert_eq!(client.get_close_factor_bps(), 1);
}

#[test]
fn set_close_factor_bps_accepts_upper_boundary() {
    let (_env, client, ..) = setup();
    client.set_close_factor_bps(&10_000);
    assert_eq!(client.get_close_factor_bps(), 10_000);
}

#[test]
fn set_liquidation_incentive_bps_accepts_lower_boundary() {
    let (_env, client, ..) = setup();
    client.set_liquidation_incentive_bps(&0);
    assert_eq!(client.get_liquidation_incentive_bps(), 0);
}

#[test]
fn set_liquidation_incentive_bps_accepts_upper_boundary() {
    let (_env, client, ..) = setup();
    client.set_liquidation_incentive_bps(&MAX_LIQUIDATION_INCENTIVE_BPS);
    assert_eq!(
        client.get_liquidation_incentive_bps(),
        MAX_LIQUIDATION_INCENTIVE_BPS
    );
}

// ─── Setters: out-of-range rejection ───────────────────────────────────────

#[test]
fn set_close_factor_bps_rejects_zero() {
    let (_env, client, ..) = setup();
    let res = client.try_set_close_factor_bps(&0);
    assert!(matches!(res, Err(Ok(LendingError::InvalidCloseFactorBps))));
    // Rejected calls must not mutate storage.
    assert_eq!(client.get_close_factor_bps(), DEFAULT_CLOSE_FACTOR_BPS);
}

#[test]
fn set_close_factor_bps_rejects_negative() {
    let (_env, client, ..) = setup();
    let res = client.try_set_close_factor_bps(&-1);
    assert!(matches!(res, Err(Ok(LendingError::InvalidCloseFactorBps))));
}

#[test]
fn set_close_factor_bps_rejects_above_10000() {
    let (_env, client, ..) = setup();
    let res = client.try_set_close_factor_bps(&10_001);
    assert!(matches!(res, Err(Ok(LendingError::InvalidCloseFactorBps))));
    assert_eq!(client.get_close_factor_bps(), DEFAULT_CLOSE_FACTOR_BPS);
}

#[test]
fn set_liquidation_incentive_bps_rejects_negative() {
    let (_env, client, ..) = setup();
    let res = client.try_set_liquidation_incentive_bps(&-1);
    assert!(matches!(
        res,
        Err(Ok(LendingError::InvalidLiquidationIncentiveBps))
    ));
    assert_eq!(
        client.get_liquidation_incentive_bps(),
        DEFAULT_LIQUIDATION_INCENTIVE_BPS
    );
}

#[test]
fn set_liquidation_incentive_bps_rejects_above_max() {
    let (_env, client, ..) = setup();
    let above_max = MAX_LIQUIDATION_INCENTIVE_BPS + 1;
    let res = client.try_set_liquidation_incentive_bps(&above_max);
    assert!(matches!(
        res,
        Err(Ok(LendingError::InvalidLiquidationIncentiveBps))
    ));
}

// ─── Setters: unauthorised caller rejection ────────────────────────────────

#[test]
#[should_panic(expected = "Unauthorized")]
fn set_close_factor_bps_rejects_unauthorised_caller() {
    let env = Env::default();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &cid,
            fn_name: "set_close_factor_bps",
            args: (7_000i128,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_close_factor_bps(&7_000);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn set_liquidation_incentive_bps_rejects_unauthorised_caller() {
    let env = Env::default();
    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &cid,
            fn_name: "set_liquidation_incentive_bps",
            args: (2_000i128,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_liquidation_incentive_bps(&2_000);
}

// ─── `liquidate` sources the governed values ───────────────────────────────

/// Drive `liquidate` and unwrap to the repaid amount, failing with a
/// descriptive message on any other branch (mirrors the matcher in
/// `liquidate_close_factor_test.rs::run_case`).
fn expect_repaid(
    liquidator: &Address,
    borrower: &Address,
    debt_asset: &Address,
    collateral_asset: &Address,
    amount: i128,
    client: &LendingContractClient<'static>,
) -> i128 {
    match client.try_liquidate(liquidator, borrower, debt_asset, collateral_asset, &amount) {
        Ok(Ok(repaid)) => repaid,
        Ok(Err(conv)) => panic!("return-value conversion error: {conv:?}"),
        Err(Ok(err)) => panic!("liquidate returned typed error: {err:?}"),
        Err(Err(invoke)) => panic!("liquidate trapped (host error): {invoke:?}"),
    }
}

/// With no admin override, `liquidate` must behave exactly as it did when
/// the close factor / incentive were inline `const` literals — pinning the
/// same scenario already covered by `liquidate_close_factor_test.rs`.
#[test]
fn liquidate_uses_default_close_factor_and_incentive_when_unset() {
    let (env, client, cid, _admin) = setup();
    assert_eq!(client.get_close_factor_bps(), 5_000);
    assert_eq!(client.get_liquidation_incentive_bps(), 1_000);

    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    seed_position(&env, &cid, &borrower, 100, 200);

    // max_repay = 200 * 5000 / 10000 = 100 (default close factor)
    let repaid = expect_repaid(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        100,
        &client,
    );
    assert_eq!(repaid, 100);
}

/// Raising the close-factor cap to 100% allows a single `liquidate` call to
/// extinguish the *entire* debt, instead of being capped at 50%.
#[test]
fn liquidate_honours_governed_close_factor_override() {
    let (env, client, cid, _admin) = setup();
    client.set_close_factor_bps(&10_000);

    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    // hf = 900 * 8000 / 800 = 9000 < 10000 -> unhealthy.
    seed_position(&env, &cid, &borrower, 900, 800);

    // max_repay = 800 * 10000 / 10000 = 800 (100% close factor) -> full repay.
    let repaid = expect_repaid(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        800,
        &client,
    );
    assert_eq!(repaid, 800);

    let pos = client.get_position(&borrower);
    assert_eq!(pos.debt, 0);
    // seized = 800 * (10000 + 1000) / 10000 = 880 (default incentive, no clamp).
    assert_eq!(pos.collateral, 900 - 880);
}

/// Zeroing out the liquidation incentive means the liquidator receives
/// exactly the repaid amount in collateral, with no bonus on top.
#[test]
fn liquidate_honours_governed_incentive_override() {
    let (env, client, cid, _admin) = setup();
    client.set_liquidation_incentive_bps(&0);

    let liquidator = Address::generate(&env);
    let borrower = Address::generate(&env);
    let debt_asset = Address::generate(&env);
    let collateral_asset = Address::generate(&env);
    // hf = 200 * 8000 / 200 = 8000 < 10000 -> unhealthy.
    seed_position(&env, &cid, &borrower, 200, 200);

    // max_repay = 200 * 5000 / 10000 = 100 (default close factor).
    let repaid = expect_repaid(
        &liquidator,
        &borrower,
        &debt_asset,
        &collateral_asset,
        100,
        &client,
    );
    assert_eq!(repaid, 100);

    let pos = client.get_position(&borrower);
    // seized = 100 * (10000 + 0) / 10000 = 100 -> no bonus collateral seized.
    assert_eq!(pos.collateral, 200 - 100);
}
