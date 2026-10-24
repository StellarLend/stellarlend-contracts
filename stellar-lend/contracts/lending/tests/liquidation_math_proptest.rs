//! Property-based tests for liquidation math and state invariants.
//!
//! Asserts for random (collateral, debt, close_factor, incentive, threshold, elapsed):
//! (a) actual_repay <= debt * close_factor / 10000
//! (b) final_seized <= collateral
//! (c) seized <= repay * (1 + incentive) (floor rounding)
//! (d) totals equal the per-user sums
//! (e) the borrower's HF never decreases due to a liquidation when there is no shortfall

#![cfg(test)]

use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env,
};
use stellarlend_lending::{
    debt::DebtPosition,
    DataKey, LendingContract, LendingContractClient, PriceRecord,
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
    last_update: u64,
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
                last_update,
            },
        );
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &col_amt);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &debt_amt);
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn liquidation_math_properties(
        debt in 1_000i128..100_000i128,
        threshold in 6_000i128..9_000i128,
        close_factor in 1_000i128..10_000i128,
        incentive in 0i128..2_000i128,
        elapsed in 0u64..100_000u64,
        repay_ratio_bps in 100i128..10_000i128,
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

        client.set_close_factor_bps(&close_factor);
        client.set_liquidation_incentive_bps(&incentive);
        client.set_liquidation_threshold_bps(&threshold);

        let base_ts = 100_000u64;
        env.ledger().with_mut(|info| info.timestamp = base_ts);
        set_price(&env, &id, &asset_col, PRICE_DIVISOR);
        set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);

        // Calculate maximum collateral that keeps HF < 10_000 (strictly undercollateralised)
        // HF = collateral * threshold / debt < 10_000 => collateral < debt * 10_000 / threshold
        let max_unhealthy_col = (debt * 10_000 - 1) / threshold;
        // Keep collateral within [10, max_unhealthy_col]
        let collateral = (max_unhealthy_col * 85 / 100).max(10);

        seed_position(&env, &id, &borrower, collateral, debt, base_ts);

        StellarAssetClient::new(&env, &asset_col).mint(&id, &10_000_000);
        StellarAssetClient::new(&env, &asset_dbt).mint(&liquidator, &10_000_000);

        if elapsed > 0 {
            env.ledger().with_mut(|info| info.timestamp = base_ts + elapsed);
            set_price(&env, &id, &asset_col, PRICE_DIVISOR);
            set_price(&env, &id, &asset_dbt, PRICE_DIVISOR);
        }

        let pos_before = client.get_position(&borrower);
        prop_assert!(pos_before.health_factor < 10_000, "Position must be unhealthy");
        let debt_before = pos_before.debt;
        let hf_before = pos_before.health_factor;

        let repay_attempt = (debt_before * repay_ratio_bps / 10_000).max(1);

        let res = client.try_liquidate(&liquidator, &borrower, &asset_dbt, &asset_col, &repay_attempt);
        prop_assert!(res.is_ok(), "Liquidation should succeed for unhealthy position: {res:?}");
        let actual_repay = res.unwrap().unwrap();

        let pos_after = client.get_position(&borrower);
        let seized = collateral - pos_after.collateral;

        // (a) actual_repay <= debt * close_factor / 10000
        let max_repay = debt_before * close_factor / 10_000;
        prop_assert!(
            actual_repay <= max_repay,
            "actual_repay ({actual_repay}) must not exceed max_repay ({max_repay})"
        );

        // (b) final_seized <= collateral
        prop_assert!(
            seized <= collateral,
            "seized ({seized}) must not exceed available collateral ({collateral})"
        );

        // (c) seized <= repay * (1 + incentive) (floor rounding)
        let max_seized = actual_repay * (10_000 + incentive) / 10_000;
        prop_assert!(
            seized <= max_seized,
            "seized ({seized}) must not exceed theoretical seized with bonus ({max_seized})"
        );

        // (d) totals equal the per-user sums
        let metrics = client.get_protocol_metrics();
        prop_assert_eq!(
            metrics.total_borrow, pos_after.debt,
            "TotalDebt must match borrower remaining debt"
        );
        prop_assert_eq!(
            metrics.total_supply, pos_after.collateral,
            "TotalDeposits must match borrower remaining collateral"
        );

        // (e) the borrower's HF never decreases due to a liquidation when there is no shortfall
        let had_shortfall = seized < max_seized;
        if !had_shortfall && pos_after.debt > 0 {
            prop_assert!(
                pos_after.health_factor >= hf_before,
                "HF must not decrease after liquidation without shortfall: before {hf_before}, after {}",
                pos_after.health_factor
            );
        }
    }
}
