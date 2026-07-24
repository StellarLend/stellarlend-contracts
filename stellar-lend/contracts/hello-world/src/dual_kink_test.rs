//! Dual Kink Interest Rate Model Tests
//!
//! These tests validate the three‑segment piecewise linear borrow‑rate calculation
//! introduced by Issue #1125. They exercise the boundary points, a mid‑segment
//! point, high‑utilization behavior, and monotonicity across the full range.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::interest_rate::{
    compute_borrow_rate, initialize_interest_rate_config, set_protocol_totals,
    InterestRateConfig,
};

fn with_contract<F, T>(env: &Env, f: F) -> T
where
    F: FnOnce(Address) -> T,
{
    let contract_id = env.register(crate::cross_asset::NoOpContract {}, ());
    env.as_contract(&contract_id, || f(Address::generate(&env)))
}

fn init(env: &Env, admin: Address, total_deposits: i128, total_borrows: i128) {
    initialize_interest_rate_config(env, admin).unwrap();
    set_protocol_totals(env, total_deposits, total_borrows).unwrap();
}

#[test]
fn zero_utilization_uses_base_rate() {
    let env = Env::default();
    env.mock_all_auths();
    with_contract(&env, |admin| {
        init(&env, admin, 1_000, 0);
        let config = InterestRateConfig::default();
        assert_eq!(compute_borrow_rate(0, 0, &config).unwrap(), config.base_rate_bps);
    });
}

#[test]
fn at_first_kink_uses_first_segment_endpoint() {
    let env = Env::default();
    env.mock_all_auths();
    with_contract(&env, |admin| {
        init(&env, admin, 1_000, 800); // utilization = 8_000 bps (kink1)
        let config = InterestRateConfig::default();
        let expected = config.base_rate_bps
            .checked_add(
                config.kink_utilization_bps
                    .checked_mul(config.multiplier_bps)
                    .unwrap()
                    .checked_div(config.kink_utilization_bps)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(compute_borrow_rate(config.kink_utilization_bps, 0, &config).unwrap(), expected);
    });
}

#[test]
fn mid_between_kinks_uses_second_segment() {
    let env = Env::default();
    env.mock_all_auths();
    with_contract(&env, |admin| {
        init(&env, admin, 1_000, 850); // utilization = 8_500 bps
        let config = InterestRateConfig::default();
        let utilization = 8_500;
        let denominator = config.kink2_bps - config.kink_utilization_bps;
        let expected = config.base_rate_bps
            .checked_add(config.multiplier_bps)
            .unwrap()
            .checked_add(
                utilization
                    .checked_sub(config.kink_utilization_bps)
                    .unwrap()
                    .checked_mul(config.jump_multiplier_bps)
                    .unwrap()
                    .checked_div(denominator)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(compute_borrow_rate(utilization, 0, &config).unwrap(), expected);
    });
}

#[test]
fn at_second_kink_uses_second_segment_endpoint() {
    let env = Env::default();
    env.mock_all_auths();
    with_contract(&env, |admin| {
        init(&env, admin, 1_000, 900); // utilization = 9_000 bps (kink2)
        let config = InterestRateConfig::default();
        let denominator = config.kink2_bps - config.kink_utilization_bps;
        let expected = config.base_rate_bps
            .checked_add(config.multiplier_bps)
            .unwrap()
            .checked_add(
                config.kink2_bps
                    .checked_sub(config.kink_utilization_bps)
                    .unwrap()
                    .checked_mul(config.jump_multiplier_bps)
                    .unwrap()
                    .checked_div(denominator)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(compute_borrow_rate(config.kink2_bps, 0, &config).unwrap(), expected);
    });
}

#[test]
fn high_utilization_uses_third_segment() {
    let env = Env::default();
    env.mock_all_auths();
    with_contract(&env, |admin| {
        init(&env, admin, 1_000, 980); // utilization = 9_800 bps
        let config = InterestRateConfig::default();
        let utilization = 9_800;
        let denominator = 10_000 - config.kink2_bps;
        let expected = config.base_rate_bps
            .checked_add(config.multiplier_bps)
            .unwrap()
            .checked_add(config.jump_multiplier_bps)
            .unwrap()
            .checked_add(
                utilization
                    .checked_sub(config.kink2_bps)
                    .unwrap()
                    .checked_mul(config.slope3_bps)
                    .unwrap()
                    .checked_div(denominator)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(compute_borrow_rate(utilization, 0, &config).unwrap(), expected);
    });
}

#[test]
fn monotonic_rate_non_decreasing() {
    let env = Env::default();
    env.mock_all_auths();
    with_contract(&env, |admin| {
        init(&env, admin, 1_000, 0);
        let config = InterestRateConfig::default();
        let mut prev = compute_borrow_rate(0, 0, &config).unwrap();
        for u in (0..=10_000).step_by(250) {
            let cur = compute_borrow_rate(u, 0, &config).unwrap();
            assert!(cur >= prev, "rate decreased at utilization {}", u);
            prev = cur;
        }
    });
}
