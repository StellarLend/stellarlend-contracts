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
use soroban_sdk::testutils::{Address as _, Ledger};

/// Set up a test environment with a registered LendingContract, admin, user,
/// and a pair of mock assets (collateral and debt).
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
    let asset_col = env.register(MockAsset, ());
    let asset_dbt = env.register(MockAsset, ());
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

    // Set collateral asset for oracle pricing (debt asset is optional)
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

// ─────────────────────────────────────────────────────────────────────────────
// Grace period admin setter / getter
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_period_admin() {
    let (_env, client, _, _admin, _, asset_col, asset_dbt) = setup();

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

    // Deposit collateral and borrow to create a health position
    client.deposit_collateral_asset(&user, &asset_col, &1000i128);
    client.borrow_asset(&user, &asset_dbt, &700i128);

    // Drop collateral price to make position unhealthy
    set_price(&env, &id, &asset_col, 8_000_000);

    // With grace = 0, liquidation should succeed immediately
    let liquidator = Address::generate(&env);
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(res.is_ok(), "grace=0 should allow immediate liquidation");
}

// ─────────────────────────────────────────────────────────────────────────────
// Grace period enforcement: reject before grace elapses
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_rejects_before_elapsed() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    // Set grace period to 1 hour (3600 seconds)
    client.set_liquidation_grace_period(&3600);

    // Create a healthy position
    client.deposit_collateral_asset(&user, &asset_col, &1000i128);
    client.borrow_asset(&user, &asset_dbt, &700i128);

    // Price drop: collateral price to $0.80 → position becomes unhealthy
    set_price(&env, &id, &asset_col, 8_000_000);

    let liquidator = Address::generate(&env);

    // Attempt liquidation immediately — should fail (grace period just started)
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));

    // Advance ledger by 30 minutes (1800 seconds) — still within grace period
    env.ledger().with_mut(|info| {
        info.timestamp += 1800;
    });

    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));

    // Advance ledger by another 1801 seconds — grace period has now elapsed
    env.ledger().with_mut(|info| {
        info.timestamp += 1801;
    });

    // Liquidation should now succeed
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(res.is_ok(), "liquidation should succeed after grace period");
}

// ─────────────────────────────────────────────────────────────────────────────
// Grace period: allowed exactly at the boundary
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_allowed_at_boundary() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    // Set grace period to exactly 1 hour
    client.set_liquidation_grace_period(&3600);

    // Create a healthy position
    client.deposit_collateral_asset(&user, &asset_col, &1000i128);
    client.borrow_asset(&user, &asset_dbt, &700i128);

    // Price drop to make position unhealthy
    set_price(&env, &id, &asset_col, 8_000_000);

    // Advance ledger by exactly the grace period (3600 seconds)
    env.ledger().with_mut(|info| {
        info.timestamp += 3600;
    });

    let liquidator = Address::generate(&env);
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

    // Create a healthy position
    client.deposit_collateral_asset(&user, &asset_col, &1000i128);
    client.borrow_asset(&user, &asset_dbt, &700i128);

    // Price drop → unhealthy
    set_price(&env, &id, &asset_col, 8_000_000);

    let liquidator = Address::generate(&env);

    // First liquidation attempt: grace timer should be stamped, reject
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));

    // Restore health: price back to $1.00
    set_price(&env, &id, &asset_col, 10_000_000);

    // Perform a deposit to trigger health check and clear the unhealthy timestamp
    client.deposit_collateral_asset(&user, &asset_col, &100i128);

    // Make position unhealthy again (new grace period starts now)
    set_price(&env, &id, &asset_col, 8_000_000);

    // Advance ledger by 1800 seconds (30 min) from the NEW unhealthy timestamp
    env.ledger().with_mut(|info| {
        info.timestamp += 1800;
    });

    // Attempt liquidation — should still be within the NEW grace period
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(matches!(
        res,
        Err(Ok(LendingError::LiquidationGracePeriodNotMet))
    ));

    // Advance ledger by another 1801 seconds to exceed the new grace period
    env.ledger().with_mut(|info| {
        info.timestamp += 1801;
    });

    // Now liquidation should succeed
    let res = client.try_liquidate(&liquidator, &user, &asset_dbt, &asset_col, &350i128);
    assert!(
        res.is_ok(),
        "liquidation should succeed after new grace period elapses"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Liquidating a healthy position returns PositionHealthy (grace not involved)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_liquidation_grace_healthy_position() {
    let (env, client, id, _admin, user, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);

    // Create a healthy position
    client.deposit_collateral_asset(&user, &asset_col, &1000i128);
    client.borrow_asset(&user, &asset_dbt, &500i128);

    // Price remains $1.00 — position is healthy
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

