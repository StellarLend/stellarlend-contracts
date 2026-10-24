//! Integration tests for `liquidate_cross_asset`.
//!
//! These tests live in the `tests/` directory so they compile against the
//! crate as a library, unaffected by broken `#[cfg(test)]` modules in `src/`.
//!
//! Real SEP-41 asset contracts are used (`register_stellar_asset_contract_v2`)
//! so that token transfers inside `liquidate_cross_asset` succeed end-to-end.
//! The liquidator is minted enough debt-asset to cover the repayment, and the
//! lending contract is minted the collateral it will transfer out.
//!
//! # Test inventory
//!
//! | Test | Description |
//! |------|-------------|
//! | `price_drop_enables_liquidation` | Collateral price drop makes position liquidatable |
//! | `healthy_rejected` | Healthy position returns `PositionHealthy` |
//! | `stale_leg_rejected` | Stale price on any leg returns `StaleOracleTimestamp` |
//! | `close_factor_cap` | Repay amount is capped by close factor |
//! | `shortfall_to_insurance_then_bad_debt` | Insufficient collateral routes shortfall correctly |
//! | `self_liquidation_rejected` | Borrower cannot liquidate themselves |
//! | `zero_repay_amount_rejected` | Zero repay rejected |
//! | `multi_round_liquidation_restores_health` | Multiple partial liquidations restore health |
//! | `seized_value_never_exceeds_repaid_value_times_incentive` | Arithmetic invariant |

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, LedgerInfo},
    token::StellarAssetClient,
    Address, Env,
};
use stellarlend_lending::{DataKey, LendingContract, LendingContractClient, LendingError, PriceRecord};

// ─── constants mirrored from the contract ───────────────────────────────────

/// 7-decimal oracle price scale: 10_000_000 = $1.00
const PRICE_DIVISOR: i128 = 10_000_000;
/// Health factor scale: 10_000 = 1.0 (healthy threshold)
const HEALTH_FACTOR_SCALE: i128 = 10_000;
/// Default close factor: 50 %
const DEFAULT_CLOSE_FACTOR_BPS: i128 = 5_000;
/// Default liquidation incentive: 10 %
const DEFAULT_INCENTIVE_BPS: i128 = 1_000;
const BPS_DENOM: i128 = 10_000;

// ─── helpers ────────────────────────────────────────────────────────────────

/// Register a real SEP-41 asset and return its address.
fn new_asset(env: &Env) -> Address {
    env.register_stellar_asset_contract_v2(Address::generate(env))
        .address()
}

/// Directly write an oracle price into persistent storage.
/// Timestamp is set to the current ledger time (fresh).
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

/// Advance ledger time by `secs` seconds (without refreshing oracle prices).
fn advance_time(env: &Env, secs: u64) {
    let mut li: LedgerInfo = env.ledger().get();
    li.timestamp = li.timestamp.saturating_add(secs);
    li.sequence_number = li.sequence_number.saturating_add(1);
    env.ledger().set(li);
}

/// Seed an insurance fund balance directly into instance storage.
fn seed_insurance(env: &Env, contract_id: &Address, amount: i128) {
    env.as_contract(contract_id, || {
        env.storage()
            .instance()
            .set(&DataKey::InsuranceFund, &amount);
    });
}

fn read_insurance(env: &Env, contract_id: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::InsuranceFund)
            .unwrap_or(0)
    })
}

fn read_bad_debt(env: &Env, contract_id: &Address) -> i128 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::BadDebt)
            .unwrap_or(0)
    })
}

/// Standard three-asset fixture.
///
/// Assets:
///   - `asset_col_a`: primary collateral, price $1.00, 80 % liq-threshold
///   - `asset_col_b`: secondary collateral, price $1.00, 80 % liq-threshold
///   - `asset_dbt`:   debt asset, price $1.00, 70 % liq-threshold
///
/// Position: borrower deposits 1_000 of `asset_col_a` and 500 of
/// `asset_col_b`, then borrows 600 of `asset_dbt`.
///
/// Token funding:
///   - The lending contract is minted `col_a_supply` of `asset_col_a` and
///     `col_b_supply` of `asset_col_b` so it can transfer seized collateral.
///   - The liquidator is minted `liquidator_dbt` of `asset_dbt` so it can
///     repay debt.
#[allow(clippy::type_complexity)]
fn setup_with_balances(
    col_a_supply: i128,
    col_b_supply: i128,
    liquidator_dbt: i128,
) -> (
    Env,
    LendingContractClient<'static>,
    Address, // contract id
    Address, // admin
    Address, // borrower
    Address, // liquidator
    Address, // asset_col_a
    Address, // asset_col_b
    Address, // asset_dbt
) {
    let env = Env::default();
    env.mock_all_auths();

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);

    let admin = Address::generate(&env);
    let borrower = Address::generate(&env);
    let liquidator = Address::generate(&env);

    let asset_col_a = new_asset(&env);
    let asset_col_b = new_asset(&env);
    let asset_dbt = new_asset(&env);

    client.initialize(&admin);

    // Asset params: (ltv_bps, liq_threshold_bps, debt_ceiling, borrow_cap, supply_cap)
    client.set_asset_params(&admin, &asset_col_a, &7500, &8000, &1_000_000_000_000i128, &0i128, &0i128);
    client.set_asset_params(&admin, &asset_col_b, &7500, &8000, &1_000_000_000_000i128, &0i128, &0i128);
    client.set_asset_params(&admin, &asset_dbt,   &6000, &7000, &1_000_000_000_000i128, &0i128, &0i128);

    // Initial prices: all $1.00
    set_price(&env, &id, &asset_col_a, PRICE_DIVISOR);
    set_price(&env, &id, &asset_col_b, PRICE_DIVISOR);
    set_price(&env, &id, &asset_dbt,   PRICE_DIVISOR);

    // Seed cross-asset position directly (bypass deposit/borrow which would
    // need separate token funding for the borrower, keeping setup simple).
    env.as_contract(&id, || {
        env.storage().persistent().set(
            &DataKey::CollateralAsset(borrower.clone(), asset_col_a.clone()),
            &1_000i128,
        );
        env.storage().persistent().set(
            &DataKey::CollateralAsset(borrower.clone(), asset_col_b.clone()),
            &500i128,
        );
        // UserCollateralAssets list
        let mut col_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        col_list.push_back(asset_col_a.clone());
        col_list.push_back(asset_col_b.clone());
        env.storage().persistent().set(
            &DataKey::UserCollateralAssets(borrower.clone()),
            &col_list,
        );
        // Debt position
        env.storage().persistent().set(
            &DataKey::DebtAsset(borrower.clone(), asset_dbt.clone()),
            &stellarlend_lending::debt::DebtPosition {
                principal: 600,
                borrow_index_snapshot: 0,
                last_update: env.ledger().timestamp(),
            },
        );
        // UserDebtAssets list
        let mut dbt_list: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
        dbt_list.push_back(asset_dbt.clone());
        env.storage().persistent().set(
            &DataKey::UserDebtAssets(borrower.clone()),
            &dbt_list,
        );
        // TotalDebtAsset tracker
        env.storage().persistent().set(
            &DataKey::TotalDebtAsset(asset_dbt.clone()),
            &600i128,
        );
    });

    // Mint collateral into lending contract so it can transfer seized amounts out
    if col_a_supply > 0 {
        StellarAssetClient::new(&env, &asset_col_a).mint(&id, &col_a_supply);
    }
    if col_b_supply > 0 {
        StellarAssetClient::new(&env, &asset_col_b).mint(&id, &col_b_supply);
    }
    // Mint debt asset to liquidator so it can repay
    if liquidator_dbt > 0 {
        StellarAssetClient::new(&env, &asset_dbt).mint(&liquidator, &liquidator_dbt);
    }

    (env, client, id, admin, borrower, liquidator, asset_col_a, asset_col_b, asset_dbt)
}

/// Convenience wrapper with generous default balances.
fn setup() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    // col_a_supply=10_000 covers all seize scenarios; liquidator gets 1_000 dbt tokens.
    setup_with_balances(10_000, 5_000, 1_000)
}

// ─── tests ──────────────────────────────────────────────────────────────────

/// A collateral price drop must enable liquidation.  After liquidation the
/// debt and collateral decrease by the expected computed amounts.
#[test]
fn price_drop_enables_liquidation() {
    let (env, client, id, _admin, borrower, liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        setup();

    // Verify healthy at initial prices
    assert!(
        client.get_cross_health_factor(&borrower) >= HEALTH_FACTOR_SCALE,
        "position must start healthy"
    );

    // Drop collateral A price to $0.20 to make position unhealthy.
    // weighted_col_a = 1000 * 2_000_000 * 8000 / 10_000 = 1_600_000_000
    // weighted_col_b = 500  * 10_000_000 * 8000 / 10_000 = 4_000_000_000
    // total_weighted = 5_600_000_000
    // debt_value     = 600 * 10_000_000 = 6_000_000_000
    // HF = 5_600_000_000 / 6_000_000_000 = 0  (< 10_000)
    set_price(&env, &id, &asset_col_a, 2_000_000); // $0.20

    let hf = client.get_cross_health_factor(&borrower);
    assert!(hf < HEALTH_FACTOR_SCALE, "expected unhealthy, got HF={hf}");

    let debt_before = client.get_debt_asset_position(&borrower, &asset_dbt).principal;
    let col_a_before = client.get_collateral_asset_balance(&borrower, &asset_col_a);

    // Liquidate: repay 300 of debt_asset (within close-factor cap of 50%)
    let repaid = client.liquidate_cross_asset(
        &liquidator,
        &borrower,
        &asset_dbt,
        &asset_col_a,
        &300i128,
    );
    assert_eq!(repaid, 300, "expected 300 repaid");

    let debt_after = env.as_contract(&id, || {
        env.storage()
            .persistent()
            .get::<DataKey, stellarlend_lending::debt::DebtPosition>(
                &DataKey::DebtAsset(borrower.clone(), asset_dbt.clone()),
            )
            .map(|p| p.principal)
            .unwrap_or(0)
    });
    let col_a_after = client.get_collateral_asset_balance(&borrower, &asset_col_a);

    assert_eq!(debt_after, debt_before - repaid, "debt must decrease by repaid");
    assert!(col_a_after < col_a_before, "collateral must be seized");

    // Verify seized = repay × (price_debt / price_col) × (1 + incentive)
    //              = 300 × (10_000_000 / 2_000_000) × (11_000 / 10_000)
    //              = 300 × 5 × 1.1 = 1_650
    let expected_seized = stellarlend_lending::math::checked_mul_div_floor(
        stellarlend_lending::math::checked_mul_div_floor(300, PRICE_DIVISOR, 2_000_000).unwrap(),
        BPS_DENOM + DEFAULT_INCENTIVE_BPS,
        BPS_DENOM,
    )
    .unwrap();
    assert_eq!(
        col_a_before - col_a_after,
        expected_seized,
        "seized amount mismatch (expected {expected_seized})"
    );
}

/// Healthy positions must be rejected with `PositionHealthy`.
#[test]
fn healthy_rejected() {
    let (_env, client, _id, _admin, borrower, liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        setup();

    let result = client.try_liquidate_cross_asset(
        &liquidator,
        &borrower,
        &asset_dbt,
        &asset_col_a,
        &100i128,
    );
    assert_eq!(
        result,
        Err(Ok(LendingError::PositionHealthy)),
        "healthy position must return PositionHealthy"
    );
}

/// Stale price on any leg must cause `StaleOracleTimestamp`.
#[test]
fn stale_leg_rejected() {
    let (env, client, id, _admin, borrower, liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        setup();

    // Make position unhealthy first
    set_price(&env, &id, &asset_col_a, 2_000_000);

    // Advance time past oracle max-age (3600 s) — asset_col_b and asset_dbt
    // prices become stale.
    advance_time(&env, 3601);

    // Refresh only asset_col_a — the other legs remain stale.
    set_price(&env, &id, &asset_col_a, 2_000_000);

    let result = client.try_liquidate_cross_asset(
        &liquidator,
        &borrower,
        &asset_dbt,
        &asset_col_a,
        &100i128,
    );
    assert_eq!(
        result,
        Err(Ok(LendingError::StaleOracleTimestamp)),
        "stale leg must return StaleOracleTimestamp"
    );
}

/// The repay amount is capped by `close_factor × debt`.
#[test]
fn close_factor_cap() {
    let (env, client, id, _admin, borrower, liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        setup();

    // Make unhealthy
    set_price(&env, &id, &asset_col_a, 2_000_000);

    let debt = client.get_debt_asset_position(&borrower, &asset_dbt).principal;
    let max_repay = stellarlend_lending::math::checked_mul_div_floor(
        debt,
        DEFAULT_CLOSE_FACTOR_BPS,
        BPS_DENOM,
    )
    .unwrap();

    // Request more than the cap
    let repaid = client.liquidate_cross_asset(
        &liquidator,
        &borrower,
        &asset_dbt,
        &asset_col_a,
        &(debt + 1_000),
    );
    assert_eq!(repaid, max_repay, "repaid must be capped by close factor");
}

/// When seized collateral exceeds the borrower's balance, the shortfall is
/// drawn from the insurance fund first; any remainder is posted to BadDebt.
#[test]
fn shortfall_to_insurance_then_bad_debt() {
    // Give lending contract only 1_000 col_a (the borrower's actual balance)
    let (env, client, id, _admin, borrower, liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        setup_with_balances(1_000, 0, 1_000);

    // Collapse collateral A price to $0.01 → seized ≈ 300 × 100 × 1.1 = 33_000
    // But only 1_000 col_a available → huge shortfall
    set_price(&env, &id, &asset_col_a, 100_000); // $0.01

    // Seed insurance fund with partial coverage (500 units)
    let insurance_seed = 500i128;
    seed_insurance(&env, &id, insurance_seed);

    let col_a_before = client.get_collateral_asset_balance(&borrower, &asset_col_a);

    let _repaid = client.liquidate_cross_asset(
        &liquidator,
        &borrower,
        &asset_dbt,
        &asset_col_a,
        &300i128,
    );

    // All collateral must be seized (drained)
    let col_a_after = client.get_collateral_asset_balance(&borrower, &asset_col_a);
    assert_eq!(col_a_after, 0, "all collateral must be seized");

    // Insurance should be fully drained
    let insurance_after = read_insurance(&env, &id);
    assert_eq!(insurance_after, 0, "insurance should be fully drained");

    // Compute expected bad debt:
    //   seized_theoretical = repay × price_debt / price_col × (BPS + incentive) / BPS
    //                      = 300 × (10_000_000 / 100_000) × (11_000 / 10_000)
    //                      = 300 × 100 × 1.1 = 33_000
    //   shortfall = 33_000 - 1_000 = 32_000
    //   insurance draws 500 → residual bad debt = 31_500
    let seized_theoretical = stellarlend_lending::math::checked_mul_div_floor(
        stellarlend_lending::math::checked_mul_div_floor(300, PRICE_DIVISOR, 100_000).unwrap(),
        BPS_DENOM + DEFAULT_INCENTIVE_BPS,
        BPS_DENOM,
    )
    .unwrap();
    let shortfall = seized_theoretical - col_a_before;
    let expected_bad_debt = shortfall - insurance_seed;

    let bad_debt = read_bad_debt(&env, &id);
    assert_eq!(bad_debt, expected_bad_debt, "bad debt mismatch (expected {expected_bad_debt}, got {bad_debt})");
}

/// Self-liquidation must return `SelfLiquidation`.
#[test]
fn self_liquidation_rejected() {
    let (env, client, id, _admin, borrower, _liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        setup();

    set_price(&env, &id, &asset_col_a, 2_000_000);

    let result = client.try_liquidate_cross_asset(
        &borrower, // liquidator == borrower
        &borrower,
        &asset_dbt,
        &asset_col_a,
        &100i128,
    );
    assert_eq!(result, Err(Ok(LendingError::SelfLiquidation)));
}

/// Zero `repay_amount` must return `InvalidAmount`.
#[test]
fn zero_repay_amount_rejected() {
    let (env, client, id, _admin, borrower, liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        setup();

    set_price(&env, &id, &asset_col_a, 2_000_000);

    let result = client.try_liquidate_cross_asset(
        &liquidator,
        &borrower,
        &asset_dbt,
        &asset_col_a,
        &0i128,
    );
    assert_eq!(result, Err(Ok(LendingError::InvalidAmount)));
}

/// Multiple partial liquidations eventually restore health.
#[test]
fn multi_round_liquidation_restores_health() {
    let (env, client, id, _admin, borrower, liquidator, asset_col_a, _asset_col_b, asset_dbt) =
        // Large balances to support many rounds
        setup_with_balances(10_000, 5_000, 10_000);

    // Drop to $0.20 — unhealthy
    set_price(&env, &id, &asset_col_a, 2_000_000);
    assert!(client.get_cross_health_factor(&borrower) < HEALTH_FACTOR_SCALE);

    let mut rounds = 0u32;
    loop {
        let hf = client.get_cross_health_factor(&borrower);
        if hf >= HEALTH_FACTOR_SCALE {
            break;
        }
        let result = client.try_liquidate_cross_asset(
            &liquidator,
            &borrower,
            &asset_dbt,
            &asset_col_a,
            &100i128,
        );
        match result {
            Ok(_) => {}
            Err(Ok(LendingError::PositionHealthy)) => break,
            Err(e) => panic!("unexpected error in round {rounds}: {:?}", e),
        }
        rounds += 1;
        assert!(rounds < 200, "too many liquidation rounds");
    }

    let final_hf = client.get_cross_health_factor(&borrower);
    assert!(
        final_hf >= HEALTH_FACTOR_SCALE,
        "position should be healthy after multi-round liquidation, HF={final_hf}"
    );
}

/// Invariant: the value seized (in price units) must never exceed
/// `repaid_value × (1 + incentive_bps / 10_000)`.
///
/// This is a pure arithmetic invariant verified without a running contract.
#[test]
fn seized_value_never_exceeds_repaid_value_times_incentive() {
    let cases: &[(i128, i128, i128)] = &[
        (10_000_000, 2_000_000, 100),    // col cheaper than debt
        (10_000_000, 10_000_000, 250),   // equal prices
        (5_000_000, 10_000_000, 300),    // col more expensive
        (10_000_000, 1_000_000, 50),     // large price ratio
        (7_777_777, 3_333_333, 111),     // odd prices
        (1, 1_000_000_000, 1_000),       // extreme cheap debt
    ];

    for &(price_debt, price_col, repay) in cases {
        // This is the same computation as in liquidate_cross_asset:
        //   step A: repay × price_debt / price_col
        //   step B: × (BPS + incentive) / BPS
        let repay_in_col =
            stellarlend_lending::math::checked_mul_div_floor(repay, price_debt, price_col)
                .expect("no overflow in step A");
        let seized =
            stellarlend_lending::math::checked_mul_div_floor(
                repay_in_col,
                BPS_DENOM + DEFAULT_INCENTIVE_BPS,
                BPS_DENOM,
            )
            .expect("no overflow in step B");

        // Convert seized back to debt-asset value: seized × price_col / price_debt
        let seized_value_in_debt_units =
            stellarlend_lending::math::checked_mul_div_floor(seized, price_col, price_debt)
                .expect("no overflow in back-conversion");

        // Must satisfy: seized_value ≤ repay × (1 + incentive)  [floor rounding guarantees this]
        let max_allowed =
            stellarlend_lending::math::checked_mul_div_floor(
                repay,
                BPS_DENOM + DEFAULT_INCENTIVE_BPS,
                BPS_DENOM,
            )
            .expect("no overflow in max_allowed");

        assert!(
            seized_value_in_debt_units <= max_allowed,
            "invariant violated: price_debt={price_debt} price_col={price_col} repay={repay}: \
             seized_value={seized_value_in_debt_units} > max_allowed={max_allowed}"
        );
    }
}
