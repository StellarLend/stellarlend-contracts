#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::interest_rate::{
    calculate_borrow_rate, calculate_supply_rate, calculate_utilization, compute_borrow_rate,
    initialize_interest_rate_config, set_protocol_totals, update_interest_rate_config,
    InterestRateConfig,
};

fn with_rate_contract<F, T>(env: &Env, f: F) -> T
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
fn utilization_zero_defaults_to_current_curve_behavior() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin, 1_000, 0);

        assert_eq!(calculate_utilization(&env).unwrap(), 0);
        // Defaults preserve old behavior: floor is 0, so the base rate is not raised.
        assert_eq!(calculate_borrow_rate(&env).unwrap(), 100);
    });
}

#[test]
fn utilization_at_100_percent_uses_curve_when_ceiling_is_open_by_default() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin, 1_000, 1_000);

        assert_eq!(calculate_utilization(&env).unwrap(), 10_000);
        // base + multiplier + post-kink jump = 100 + 2_000 + 10_000 = 12_100
        assert_eq!(calculate_borrow_rate(&env).unwrap(), 12_100);
    });
}

#[test]
fn borrow_rate_is_clamped_to_configured_floor_as_final_step() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin.clone(), 1_000, 0);
        update_interest_rate_config(
            &env,
            admin,
            Some(0),
            None,
            Some(0),
            None,
            Some(250),
            Some(5_000),
            None,
        )
        .unwrap();

        assert_eq!(calculate_utilization(&env).unwrap(), 0);
        assert_eq!(calculate_borrow_rate(&env).unwrap(), 250);
    });
}

#[test]
fn borrow_rate_is_clamped_to_configured_ceiling_as_final_step() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin.clone(), 1_000, 1_000);
        update_interest_rate_config(
            &env,
            admin,
            Some(100),
            Some(8_000),
            Some(2_000),
            Some(10_000),
            Some(0),
            Some(3_000),
            None,
        )
        .unwrap();

        assert_eq!(calculate_utilization(&env).unwrap(), 10_000);
        assert_eq!(calculate_borrow_rate(&env).unwrap(), 3_000);
    });
}

#[test]
fn supply_rate_uses_the_clamped_borrow_rate() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin.clone(), 1_000, 1_000);
        update_interest_rate_config(
            &env,
            admin,
            None,
            None,
            None,
            None,
            Some(0),
            Some(3_000),
            Some(500),
        )
        .unwrap();

        assert_eq!(calculate_borrow_rate(&env).unwrap(), 3_000);
        assert_eq!(calculate_supply_rate(&env).unwrap(), 2_500);
    });
}

#[test]
fn pure_compute_clamps_extreme_curve_outputs() {
    let config = InterestRateConfig {
        base_rate_bps: 0,
        kink_utilization_bps: 8_000,
        kink2_bps: 9_000,
        multiplier_bps: 100_000,
        jump_multiplier_bps: 10_000,
        slope3_bps: 20_000,
        min_rate_bps: 1_000,
        max_rate_bps: 4_000,
        rate_floor_bps: 1_000,
        rate_ceiling_bps: 4_000,
        spread_bps: 0,
    };

    assert_eq!(compute_borrow_rate(0, 0, &config).unwrap(), 1_000);
    assert_eq!(compute_borrow_rate(10_000, 0, &config).unwrap(), 4_000);
}
