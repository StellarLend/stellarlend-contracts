#![cfg(test)]

//! Unit tests for the configurable liquidation grace period feature.
//!
//! Coverage includes:
//! - Default grace period of 0 (immediate liquidation, no-op)
//! - Configured grace period rejects liquidation before elapsed time
//! - Configured grace period allows liquidation at or after boundary
//! - Timestamp clearing and resetting when health factor recovers and drops again
//! - Unauthorized setting of grace period rejects with Unauthorized
//! - Bounded maximum validation of grace period setter

use super::*;
use crate::liquidate_transfer_test::{MockToken, MockTokenClient};
use crate::debt::DebtPosition;
use soroban_sdk::testutils::{Address as _, Ledger};

/// Set up a test environment with a registered LendingContract, admin, user,
/// and a pair of mock tokens (collateral and debt).
fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let asset_col = env.register(MockToken, ());
    let asset_dbt = env.register(MockToken, ());
    client.initialize(&admin);

    // Configure asset params
    client.set_asset_params(
        &admin,
        &asset_col,
        &7500,                  // 75% LTV
        &8000,                  // 80% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
        &0i128,                 // borrow_cap (uncapped)
        &0i128,                 // supply_cap (uncapped)
    );
    client.set_asset_params(
        &admin,
        &asset_dbt,
        &6000,                  // 60% LTV
        &7000,                  // 70% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
        &0i128,                 // borrow_cap (uncapped)
        &0i128,                 // supply_cap (uncapped)
    );

    // Set collateral asset for oracle pricing
    client.set_collateral_asset(&asset_col);

    // Initial prices: $1.00 for col, $1.00 for dbt
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    (env, client, id, admin, user, asset_col, asset_dbt)
}

/// Inject a price record directly into the contract's persistent storage
/// (bypasses the oracle signature check).
fn set_price(env: &Env, contract_id: &Address, asset: &Address, price: i128) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord {
                price,
                timestamp: env.ledger().timestamp(),
            },
        );
    });
}

/// Directly seed the legacy single-asset collateral and debt storage keys
/// (DataKey::Collateral and DataKey::Debt) that the liquidate function reads.
fn seed_legacy_position(
    env: &Env,
    contract_id: &Address,
    user: &Address,
    col_amt: i128,
    debt_amt: i128,
) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(user.clone()), &col_amt);
        env.storage().persistent().set(
            &DataKey::Debt(user.clone()),
            &DebtPosition {
                principal: debt_amt,
                borrow_index_snapshot: crate::debt::INDEX_SCALE,
                last_update: env.ledger().timestamp(),
            },
        );
    });
}

/// Pre-set the FirstUnhealthyTimestamp so the grace-period check sees a
/// known timestamp in persistent storage.
fn seed_unhealthy_timestamp(env: &Env, contract_id: &Address, user: &Address, ts: u64) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::FirstUnhealthyTimestamp(user.clone()), &ts);
    });
}

/// Mint mock tokens to an address so that TokenClient transfers inside
/// liquidate succeed.
fn mint_mock_token(env: &Env, asset: &Address, to: &Address, amount: i128) {
    let token = MockTokenClient::new(env, asset);
    token.mint(to, &amount);
}

// ─────────────────────────────────────────────────────────────────────────────
// Grace period admin setter / getter
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_period_admin() {
    let (_env, client, _, _admin, _, _asset_col, _asset_dbt) = setup();

    // Default should be 0
    assert_eq!(client.get_liquidation_grace_period(), 0);

    // Set valid grace period (e.g. 1 hour)
    client.set_liquidation_grace_period(&3600);
    assert_eq!(client.get_liquidation_grace_period(), 3600);

    // Set to 0 (immediate liquidation — back to default behaviour)
    client.set_liquidation_grace_period(&0);
    assert_eq!(client.get_liquidation_grace_period(), 0);

    // Set max allowed (30 days)
    client.set_liquidation_grace_period(&MAX_LIQUIDATION_GRACE_PERIOD_SECS);
    assert_eq!(
        client.get_liquidation_grace_period(),
        MAX_LIQUIDATION_GRACE_PERIOD_SECS
    );

    // Reject if too large (31 days)
    let res = client.try_set_liquidation_grace_period(&(MAX_LIQUIDATION_GRACE_PERIOD_SECS + 1));
    assert!(matches!(
        res,
        Err(Ok(LendingError::InvalidLiquidationGracePeriod))
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// Grace period: grace = 0 preserves immediate liquidation (no-op path)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_zero_is_immediate() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    // Grace period remains 0 (default)
    assert_eq!(client.get_liquidation_grace_period(), 0);

    // Seed an unhealthy position: coll = 500, debt = 1000 => HF = 500*8000/1000 = 4000
    seed_legacy_position(&env, &id, &user, 500, 1000);

    let liquidator = Address::generate(&env);
    mint_mock_token(&env, &asset_col, &id, 2000); // contract holds collateral tokens
    mint_mock_token(&env, &asset_dbt, &liquidator, 2000); // liquidator holds debt tokens

    // With grace = 0, liquidation should succeed immediately
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(res.is_ok(), "grace=0 should allow immediate liquidation");
}

// ─────────────────────────────────────────────────────────────────────────────
// Grace period enforcement: reject before grace elapses
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_rejects_before_elapsed() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);

    let base_ts = 100_000;
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts;
    });
    // Refresh prices so they are not stale at base_ts.
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    seed_legacy_position(&env, &id, &user, 500, 1000);
    seed_unhealthy_timestamp(&env, &id, &user, base_ts);

    let liquidator = Address::generate(&env);
    mint_mock_token(&env, &asset_col, &id, 2000);
    mint_mock_token(&env, &asset_dbt, &liquidator, 2000);

    // Still at base_ts — no time has passed
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));

    // Advance by 30 minutes — still within grace period
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts + 1800;
    });
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));

    // Advance by another 1801 seconds — grace period has now elapsed
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts + 3601;
    });
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(res.is_ok(), "liquidation should succeed after grace period");
}

// ─────────────────────────────────────────────────────────────────────────────
// Grace period: allowed exactly at the boundary
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_allowed_at_boundary() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);

    let base_ts = 100_000;
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts;
    });
    // Refresh prices so they are not stale at base_ts.
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    seed_legacy_position(&env, &id, &user, 500, 1000);
    seed_unhealthy_timestamp(&env, &id, &user, base_ts);

    // Advance by exactly the grace period
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts + 3600;
    });
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    let liquidator = Address::generate(&env);
    mint_mock_token(&env, &asset_col, &id, 2000);
    mint_mock_token(&env, &asset_dbt, &liquidator, 2000);

    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(
        res.is_ok(),
        "liquidation should succeed exactly at grace period boundary"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Health recovery resets the grace timer
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_resets_on_health_recovery() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);

    let base_ts = 100_000;
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts;
    });
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    // Start unhealthy with timestamp in the past (grace period already elapsed).
    seed_legacy_position(&env, &id, &user, 500, 1000);
    seed_unhealthy_timestamp(&env, &id, &user, base_ts);

    let liquidator = Address::generate(&env);
    mint_mock_token(&env, &asset_col, &id, 5000);
    mint_mock_token(&env, &asset_dbt, &liquidator, 5000);

    // First attempt: still at base_ts (no time passed) — should reject.
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));

    // Advance past grace period — first liquidation should now succeed.
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts + 3600;
    });
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(
        res.is_ok(),
        "liquidation should succeed after grace period elapses"
    );

    // Simulate health recovery: clear unhealthy timestamp so that when the
    // position becomes unhealthy again a new grace period starts from scratch.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::FirstUnhealthyTimestamp(user.clone()));
    });

    // Re-seed an unhealthy position at a later timestamp.
    let new_base = 300_000;
    env.ledger().with_mut(|info| {
        info.timestamp = new_base;
    });
    seed_legacy_position(&env, &id, &user, 500, 1000);
    set_price(&env, &id, &asset_col, 10_000_000);
    set_price(&env, &id, &asset_dbt, 10_000_000);

    // First attempt at new_base: no unhealthy timestamp exists → should stamp
    // a new one and reject (grace period just started).
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// Liquidating a healthy position returns PositionHealthy (grace not involved)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_healthy_position() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);

    // Healthy: 5000 collateral, 1000 debt => HF = 40000
    seed_legacy_position(&env, &id, &user, 5000, 1000);

    let liquidator = Address::generate(&env);
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &200i128);
    assert!(matches!(res, Err(Ok(LendingError::PositionHealthy))));
}

// ─────────────────────────────────────────────────────────────────────────────
// Unauthorized setter rejection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_unauthorized_setter() {
    let env = Env::default();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    // Initialize with admin
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &id,
            fn_name: "initialize",
            args: (admin.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin);

    // Attempt to set grace period as non-admin — should fail
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &id,
            fn_name: "set_liquidation_grace_period",
            args: (3600u64,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let res = client.try_set_liquidation_grace_period(&3600);
    assert!(
        res.is_err(),
        "non-admin should not be able to set grace period"
    );
}
