use super::*;
use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};

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
    let asset_a = env.register(MockAsset, ());
    let asset_b = env.register(MockAsset, ());
    client.initialize(&admin);

    // Configure asset params
    client.set_asset_params(
        &admin,
        &asset_a,
        &7500,                  // 75% LTV
        &8000,                  // 80% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
        &0i128,                 // borrow_cap (0 = uncapped)
    );
    client.set_asset_params(
        &admin,
        &asset_b,
        &6000,                  // 60% LTV
        &7000,                  // 70% liquidation threshold
        &1_000_000_000_000i128, // debt ceiling
        &0i128,                 // borrow_cap (0 = uncapped)
    );

    // Set oracle prices: 10_000_000 = $1.00 (7-decimal precision)
    env.as_contract(&id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset_a.clone()),
            &PriceRecord {
                price: 10_000_000i128,
                timestamp: env.ledger().timestamp(),
            },
        );
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset_b.clone()),
            &PriceRecord {
                price: 20_000_000_000i128,
                timestamp: env.ledger().timestamp(),
            },
        );
    });

    (env, client, id, admin, user, asset_a, asset_b)
}

/// Advances ledger time and sequence together so timestamp-based freshness
/// checks observe the same monotonic clock as the rest of the test harness.
fn advance_time(env: &Env, seconds: u64) {
    let mut ledger: LedgerInfo = env.ledger().get();
    ledger.timestamp = ledger.timestamp.saturating_add(seconds);
    ledger.sequence_number = ledger.sequence_number.saturating_add(seconds as u32);
    env.ledger().set(ledger);
}

/// Overwrites an asset's oracle price with a *fresh* timestamp equal to the
/// current ledger time, so the staleness guard treats it as fresh again.
fn set_fresh_price(env: &Env, id: &Address, asset: &Address, price: i128) {
    env.as_contract(id, || {
        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord {
                price,
                timestamp: env.ledger().timestamp(),
            },
        );
    });
}

/// Boundary #1: a *fresh* borrowed asset whose *collateral* leg is stale must be
/// rejected (fail closed). Because the user has no outstanding debt on the
/// first borrow, `compute_aggregate_health_factor` would otherwise short-circuit
/// to the no-debt sentinel and skip the stale collateral price — the guard is
/// what actually blocks this under-collateralised borrow.
#[test]
fn borrow_rejects_when_collateral_leg_is_stale() {
    let (env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);

    let res = client.try_borrow_asset(&user, &asset_a, &100i128);
    assert!(
        matches!(res, Err(Ok(LendingError::StaleOracleTimestamp))),
        "Borrow against stale collateral must fail closed with StaleOracleTimestamp"
    );
}

/// Boundary #2: when *every* leg of the position is fresh, the borrow succeeds.
/// Guards the happy path so the new staleness check does not regress normal flow.
#[test]
fn borrow_allows_when_all_legs_fresh() {
    let (_env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    let principal = client.borrow_asset(&user, &asset_a, &100i128);
    assert_eq!(principal, 100);
    let pos = client.get_debt_asset_position(&user, &asset_a);
    assert_eq!(pos.principal, 100);
}

/// Boundary #3: a stale *debt* leg must also reject the borrow, even when the
/// collateral leg is freshly re-priced. This isolates and exercises the
/// debt-leg branch of `ensure_position_prices_fresh`.
#[test]
fn borrow_rejects_when_debt_leg_is_stale() {
    let (env, client, id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    client.borrow_asset(&user, &asset_a, &100i128);

    // Advance past the max age, then re-price ONLY the collateral leg so it is
    // fresh again. The debt leg (asset_a) keeps its now-stale timestamp.
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);
    set_fresh_price(&env, &id, &asset_b, 20_000_000_000i128);

    let res = client.try_borrow_asset(&user, &asset_a, &50i128);
    assert!(
        matches!(res, Err(Ok(LendingError::StaleOracleTimestamp))),
        "Borrow with a stale debt leg must fail closed with StaleOracleTimestamp"
    );
}

/// Boundary #4: repay is intentionally *not* gated by the staleness guard, so a
/// user can always reduce risk (repay) even when every price leg is stale.
#[test]
fn repay_allows_when_legs_are_stale() {
    let (env, client, _id, _admin, user, asset_a, asset_b) = setup();
    client.deposit_collateral_asset(&user, &asset_b, &1i128);
    client.borrow_asset(&user, &asset_a, &100i128);

    // Stale BOTH legs.
    advance_time(&env, DEFAULT_ORACLE_MAX_AGE_SECS + 1);

    let remaining = client.repay_asset(&user, &asset_a, &40i128);
    assert_eq!(remaining, 60, "Repay must always succeed, even with stale prices");
}
