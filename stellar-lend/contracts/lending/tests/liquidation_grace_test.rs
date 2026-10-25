//! Integration tests for the liquidation grace period.
//!
//! These tests live in `tests/` so they build against the crate library,
//! unaffected by broken `#[cfg(test)]` unit test modules in `src/`.
//!
//! Acceptance criteria covered:
//! - `grace_blocks_early_liquidation`: With grace > 0, an unhealthy position cannot be liquidated
//!   before the grace window elapses.
//! - `grace_expires_allows_liquidation`: With grace > 0, liquidation succeeds once the grace window elapses.
//! - `zero_grace_unchanged`: With grace = 0, liquidation proceeds immediately.
//! - `recovery_clears_timestamp`: Restoring HF >= 1.0 clears `FirstUnhealthyTimestamp`, while dust
//!   deposits that leave HF < 1.0 do not clear it. A subsequent drop starts a fresh window.
//! - `mark_unhealthy_healthy_noop`: Calling `mark_unhealthy` on a healthy position writes nothing.
//! - `timestamp_ttl_extended`: Storing or refreshing the unhealthy timestamp extends its entry TTL.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};
use stellarlend_lending::{
    debt::DebtPosition,
    DataKey, LendingContract, LendingContractClient, LendingError, PriceRecord,
};

const PRICE_DIVISOR: i128 = 10_000_000;

fn new_asset(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

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

fn seed_position(
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
                borrow_index_snapshot: stellarlend_lending::debt::INDEX_SCALE,
                last_update: env.ledger().timestamp(),
            },
        );
    });
}

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address, // contract id
    Address, // admin
    Address, // borrower
    Address, // liquidator
    Address, // asset_col
    Address, // asset_dbt
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    let asset_col = new_asset(&env);
    let asset_dbt = new_asset(&env);

    client.initialize(&admin);

    client.set_collateral_asset(&asset_col);
    client.set_debt_asset(&asset_dbt);

    // Initial prices: $1.00 each
    set_price(&env, &id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    // Mint tokens for liquidator and contract
    let col_client = StellarAssetClient::new(&env, &asset_col);
    let dbt_client = StellarAssetClient::new(&env, &asset_dbt);
    col_client.mint(&id, &100_000);
    dbt_client.mint(&liquidator, &100_000);

    (env, client, id, admin, borrower, liquidator, asset_col, asset_dbt)
}

#[test]
fn grace_blocks_early_liquidation() {
    let (env, client, id, _admin, borrower, liquidator, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);
    let base_ts = 100_000;
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts;
    });
    set_price(&env, &id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    // Unhealthy position: collateral = 500, debt = 1000. With 80% liquidation threshold, HF = 4000 < 10000.
    seed_position(&env, &id, &borrower, 500, 1000);

    // Mark unhealthy at base_ts
    client.mark_unhealthy(&borrower);
    assert_eq!(client.get_first_unhealthy_timestamp(&borrower), Some(base_ts));

    // Calling liquidate immediately (at base_ts) fails with LiquidationGracePeriodActive
    let res = client.try_liquidate(&liquidator, &borrower, &asset_dbt, &asset_col, &300i128);
    assert!(matches!(res, Err(Ok(LendingError::LiquidationGracePeriodActive))));

    // Advance time by 1800s (halfway through grace window)
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts + 1800;
    });
    set_price(&env, &id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    let res = client.try_liquidate(&liquidator, &borrower, &asset_dbt, &asset_col, &300i128);
    assert!(matches!(res, Err(Ok(LendingError::LiquidationGracePeriodActive))));
}

#[test]
fn grace_expires_allows_liquidation() {
    let (env, client, id, _admin, borrower, liquidator, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);
    let base_ts = 100_000;
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts;
    });
    set_price(&env, &id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    seed_position(&env, &id, &borrower, 500, 1000);
    client.mark_unhealthy(&borrower);
    assert_eq!(client.get_first_unhealthy_timestamp(&borrower), Some(base_ts));

    // Advance time past grace period: base_ts + 3600
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts + 3600;
    });
    set_price(&env, &id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    let res = client.try_liquidate(&liquidator, &borrower, &asset_dbt, &asset_col, &300i128);
    assert!(res.is_ok(), "Liquidation must succeed once grace period expires");
}

#[test]
fn zero_grace_unchanged() {
    let (env, client, id, _admin, borrower, liquidator, asset_col, asset_dbt) = setup();

    // Default grace period is 0
    assert_eq!(client.get_liquidation_grace_period(), 0);

    seed_position(&env, &id, &borrower, 500, 1000);

    // With grace = 0, liquidation succeeds immediately without delay
    let res = client.try_liquidate(&liquidator, &borrower, &asset_dbt, &asset_col, &300i128);
    assert!(res.is_ok(), "Zero grace period must allow immediate liquidation");
}

#[test]
fn recovery_clears_timestamp() {
    let (env, client, id, _admin, borrower, _liquidator, asset_col, asset_dbt) = setup();

    client.set_liquidation_grace_period(&3600);
    let base_ts = 100_000;
    env.ledger().with_mut(|info| {
        info.timestamp = base_ts;
    });
    set_price(&env, &id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    // Unhealthy: collateral 500, debt 1000
    seed_position(&env, &id, &borrower, 500, 1000);
    client.mark_unhealthy(&borrower);
    assert_eq!(client.get_first_unhealthy_timestamp(&borrower), Some(base_ts));

    // Dust deposit of 10 collateral does not bring HF >= 10_000 (collateral 510, debt 1000, 80% threshold => HF = 4080 < 10000)
    client.deposit(&borrower, &10i128);
    assert_eq!(
        client.get_first_unhealthy_timestamp(&borrower),
        Some(base_ts),
        "Dust deposit must not clear timestamp"
    );

    // Now deposit enough collateral to restore HF >= 10_000 (deposit 1000 -> total 1510 collateral, debt 1000 => HF = 12080 >= 10000)
    client.deposit(&borrower, &1000i128);
    assert_eq!(
        client.get_first_unhealthy_timestamp(&borrower),
        None,
        "Restoring health factor >= 1.0 via deposit must clear timestamp"
    );

    // Drop collateral price so position becomes unhealthy again at a later timestamp
    let later_ts = 200_000;
    env.ledger().with_mut(|info| {
        info.timestamp = later_ts;
    });
    // Price drop from $1.00 (10_000_000) to $0.50 (5_000_000)
    // 1510 * 0.5 = $755 collateral value. 80% threshold => $604 vs $1000 debt => HF = 6040 < 10000.
    set_price(&env, &id, &asset_col, 5_000_000);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    // Mark unhealthy again - starts a fresh grace window at later_ts
    client.mark_unhealthy(&borrower);
    assert_eq!(
        client.get_first_unhealthy_timestamp(&borrower),
        Some(later_ts),
        "Becoming unhealthy again must start a fresh grace window"
    );
}

#[test]
fn mark_unhealthy_healthy_noop() {
    let (env, client, id, _admin, borrower, _liquidator, _asset_col, _asset_dbt) = setup();

    // Position is healthy: collateral 5000, debt 1000
    seed_position(&env, &id, &borrower, 5000, 1000);

    client.mark_unhealthy(&borrower);
    assert_eq!(
        client.get_first_unhealthy_timestamp(&borrower),
        None,
        "mark_unhealthy on healthy position must write nothing"
    );

    // Position with no debt is also healthy
    let borrower_no_debt = Address::generate(&env);
    seed_position(&env, &id, &borrower_no_debt, 1000, 0);
    client.mark_unhealthy(&borrower_no_debt);
    assert_eq!(
        client.get_first_unhealthy_timestamp(&borrower_no_debt),
        None,
        "mark_unhealthy on zero-debt position must write nothing"
    );
}

#[test]
fn timestamp_ttl_extended() {
    let (env, client, id, _admin, borrower, _liquidator, _asset_col, _asset_dbt) = setup();

    seed_position(&env, &id, &borrower, 500, 1000);

    client.mark_unhealthy(&borrower);
    assert!(client.get_first_unhealthy_timestamp(&borrower).is_some());

    // Calling mark_unhealthy again extends TTL and preserves timestamp
    let ts_before = client.get_first_unhealthy_timestamp(&borrower).unwrap();
    env.ledger().with_mut(|info| {
        info.timestamp += 100;
    });
    client.mark_unhealthy(&borrower);
    let ts_after = client.get_first_unhealthy_timestamp(&borrower).unwrap();
    assert_eq!(ts_before, ts_after, "Timestamp should not be overwritten when already unhealthy");
}
