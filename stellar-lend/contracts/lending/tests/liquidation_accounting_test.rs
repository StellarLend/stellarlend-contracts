//! Integration tests for liquidation accounting:
//! - Settlement at live borrow rate (instead of DEFAULT_APR_BPS)
//! - Protocol TotalDebt and TotalDeposits consistency
//! - Shortfall path bad-debt event and non-negative TotalDeposits

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};
use stellarlend_lending::{
    rate_model::RateParams,
    BadDebtEvent, DataKey, LendingContract, LendingContractClient, PriceRecord,
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

fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address, // contract id
    Address, // admin
    Address, // asset_col
    Address, // asset_dbt
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let asset_col = new_asset(&env);
    let asset_dbt = new_asset(&env);

    client.initialize(&admin);
    client.set_collateral_asset(&asset_col);
    client.set_debt_asset(&asset_dbt);

    set_price(&env, &id, &asset_col, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

    (env, client, id, admin, asset_col, asset_dbt)
}

#[test]
fn liquidate_uses_live_rate() {
    let rate_params = RateParams {
        base_rate_bps: 2000, // 20% APR (significantly higher than DEFAULT_APR_BPS of 500)
        kink_utilization_bps: 7_000,
        multiplier_bps: 1_000,
        jump_multiplier_bps: 5_000,
        rate_floor_bps: 100,
        rate_ceiling_bps: 10_000,
        max_rate_change_per_ledger_bps: i128::MAX,
        hysteresis_bps: 0,
        surcharge_kink_bps: 10_000,
        surcharge_slope: 0,
    };

    // Environment 1: run liquidation
    let (env1, client1, id1, _admin1, col1, dbt1) = setup();
    client1.set_rate_params(&rate_params);

    let borrower1 = Address::generate(&env1);
    let liquidator1 = Address::generate(&env1);

    StellarAssetClient::new(&env1, &col1).mint(&id1, &100_000);
    StellarAssetClient::new(&env1, &dbt1).mint(&liquidator1, &100_000);

    client1.deposit(&borrower1, &1000);
    client1.borrow(&borrower1, &790);

    // Environment 2: run repay under identical initial conditions
    let (env2, client2, _id2, _admin2, _col2, _dbt2) = setup();
    client2.set_rate_params(&rate_params);

    let borrower2 = Address::generate(&env2);
    client2.deposit(&borrower2, &1000);
    client2.borrow(&borrower2, &790);

    // Advance time in both environments equally
    const ELAPSED: u64 = 100_000;
    env1.ledger().with_mut(|info| info.timestamp += ELAPSED);
    env2.ledger().with_mut(|info| info.timestamp += ELAPSED);
    set_price(&env1, &id1, &col1, PRICE_DIVISOR);
    set_price(&env1, &id1, &dbt1, PRICE_DIVISOR);

    // Liquidate 100 in env1
    let repaid = client1.liquidate(&liquidator1, &borrower1, &dbt1, &col1, &100);
    assert!(repaid > 0);

    // Repay 100 in env2
    let remaining = client2.repay(&borrower2, &100);
    assert!(remaining > 0);

    // Both operations advance the borrow index using current_borrow_rate(&env)
    let index_liq = client1.get_borrow_index();
    let index_repay = client2.get_borrow_index();
    assert_eq!(
        index_liq, index_repay,
        "borrow index after liquidate must equal borrow index after repay"
    );
}

#[test]
fn liquidate_updates_total_debt() {
    let (env, client, id, _admin, col, dbt) = setup();

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let user_c = Address::generate(&env);
    let liquidator = Address::generate(&env);

    StellarAssetClient::new(&env, &col).mint(&id, &100_000);
    StellarAssetClient::new(&env, &dbt).mint(&liquidator, &100_000);

    client.deposit(&user_a, &1_000);
    client.borrow(&user_a, &600);

    client.deposit(&user_b, &2_000);
    client.borrow(&user_b, &800);

    client.deposit(&user_c, &3_000);

    let metrics_before = client.get_protocol_metrics();
    assert_eq!(metrics_before.total_borrow, 1_400);
    assert_eq!(metrics_before.total_supply, 6_000);

    // Drop collateral price so user_a becomes undercollateralised
    // Collateral value = 1000 * 0.6 = $600. 80% threshold => $480 < $600 debt.
    set_price(&env, &id, &col, 6_000_000);

    let repaid = client.liquidate(&liquidator, &user_a, &dbt, &col, &200);
    assert!(repaid > 0);

    let metrics_after = client.get_protocol_metrics();
    let debt_a = client.get_position(&user_a).debt;
    let debt_b = client.get_position(&user_b).debt;

    assert_eq!(
        metrics_after.total_borrow,
        debt_a + debt_b,
        "TotalDebt must equal sum of user debts after liquidation"
    );
}

#[test]
fn liquidate_updates_total_deposits() {
    let (env, client, id, _admin, col, dbt) = setup();

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let user_c = Address::generate(&env);
    let liquidator = Address::generate(&env);

    StellarAssetClient::new(&env, &col).mint(&id, &100_000);
    StellarAssetClient::new(&env, &dbt).mint(&liquidator, &100_000);

    client.deposit(&user_a, &1_000);
    client.borrow(&user_a, &600);

    client.deposit(&user_b, &2_000);
    client.borrow(&user_b, &800);

    client.deposit(&user_c, &3_000);

    set_price(&env, &id, &col, 6_000_000);

    let initial_deposits = client.get_protocol_metrics().total_supply;
    assert_eq!(initial_deposits, 6_000);

    let repaid = client.liquidate(&liquidator, &user_a, &dbt, &col, &200);
    assert!(repaid > 0);

    let metrics_after = client.get_protocol_metrics();
    let col_a = client.get_position(&user_a).collateral;
    let col_b = client.get_position(&user_b).collateral;
    let col_c = client.get_position(&user_c).collateral;

    assert_eq!(
        metrics_after.total_supply,
        col_a + col_b + col_c,
        "TotalDeposits must equal sum of user collaterals after liquidation"
    );
    assert!(
        metrics_after.total_supply < initial_deposits,
        "TotalDeposits must have decreased by seized collateral"
    );
}

#[test]
fn shortfall_path_totals_consistent() {
    let (env, client, id, _admin, col, dbt) = setup();

    let borrower = Address::generate(&env);
    let depositor = Address::generate(&env);
    let liquidator = Address::generate(&env);

    StellarAssetClient::new(&env, &col).mint(&id, &100_000);
    StellarAssetClient::new(&env, &dbt).mint(&liquidator, &100_000);

    // Depositor provides collateral to protocol
    client.deposit(&depositor, &1_000);

    // Directly seed a deep shortfall position: borrower has only 50 collateral but 500 debt
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Collateral(borrower.clone()), &50i128);
        env.storage().persistent().set(
            &DataKey::Debt(borrower.clone()),
            &stellarlend_lending::debt::DebtPosition {
                principal: 500,
                borrow_index_snapshot: 0,
                last_update: env.ledger().timestamp(),
            },
        );
        // Total deposits: 1000 (depositor) + 50 (borrower)
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &1_050i128);
        // Total debt: 500 (borrower)
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &500i128);
    });

    // Max repay is 500 * 50% = 250.
    // Seized collateral with 10% bonus would be 250 * 1.1 = 275.
    // Available collateral is only 50 -> shortfall = 225.
    let repaid = client.liquidate(&liquidator, &borrower, &dbt, &col, &250);
    assert_eq!(repaid, 250);

    // Bad debt should be recorded as 225
    assert_eq!(client.get_bad_debt(), 225);

    // Borrower collateral is completely seized (now 0)
    assert_eq!(client.get_position(&borrower).collateral, 0);

    // TotalDeposits decreased from 1050 by 50 (final_seized) to 1000. It cannot go negative.
    let metrics = client.get_protocol_metrics();
    assert_eq!(metrics.total_supply, 1_000);
    assert!(metrics.total_supply >= 0);

    // TotalDebt decreased from 500 by 250 to 250
    assert_eq!(metrics.total_borrow, 250);

    // Verify BadDebtEvent contractevent was emitted
    let cid = client.address.clone();
    let expected_event = BadDebtEvent {
        borrower: borrower.clone(),
        amount: 225,
    }
    .to_xdr(&env, &cid);

    let all_events = env.events().all();
    let has_bad_debt_event = all_events.events().iter().any(|ev| ev == &expected_event);
    assert!(
        has_bad_debt_event,
        "BadDebtEvent must be emitted on shortfall liquidation"
    );
}
