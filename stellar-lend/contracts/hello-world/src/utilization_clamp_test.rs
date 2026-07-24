#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::interest_rate::{
    calculate_utilization, initialize_interest_rate_config, set_protocol_totals, BASIS_POINTS_SCALE,
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

fn assert_within_bounds(utilization: i128) {
    assert!(utilization >= 0, "utilization should never be negative");
    assert!(
        utilization <= BASIS_POINTS_SCALE,
        "utilization should never exceed 100%"
    );
}

#[test]
fn calculate_utilization_returns_zero_without_divide_by_zero_when_deposits_are_zero() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin, 0, 1_000);

        let utilization = calculate_utilization(&env).unwrap();
        assert_eq!(utilization, 0);
        assert_within_bounds(utilization);
    });
}

#[test]
fn calculate_utilization_clamps_to_full_utilization_when_borrows_cover_deposits() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin.clone(), 1_000, 1_000);

        let equal_case = calculate_utilization(&env).unwrap();
        assert_eq!(equal_case, BASIS_POINTS_SCALE);
        assert_within_bounds(equal_case);

        set_protocol_totals(&env, 1_000, 1_500).unwrap();

        let over_borrow_case = calculate_utilization(&env).unwrap();
        assert_eq!(over_borrow_case, BASIS_POINTS_SCALE);
        assert_within_bounds(over_borrow_case);
    });
}

#[test]
fn calculate_utilization_matches_exact_bps_for_common_ratios() {
    let env = Env::default();
    env.mock_all_auths();

    with_rate_contract(&env, |admin| {
        init(&env, admin.clone(), 1_000, 0);

        let cases = [
            (1_000, 250, 2_500),
            (1_000, 500, 5_000),
            (1_000, 800, 8_000),
        ];

        for (deposits, borrows, expected) in cases {
            set_protocol_totals(&env, deposits, borrows).unwrap();

            let utilization = calculate_utilization(&env).unwrap();
            assert_eq!(utilization, expected);
            assert_within_bounds(utilization);
        }
    });
}
