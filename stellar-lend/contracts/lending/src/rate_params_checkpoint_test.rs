use crate::debt::{self, DebtPosition, INDEX_SCALE};
use crate::rate_model::{self, RateParams};
use crate::{DataKey, LendingContract, LendingContractClient, LendingError};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

const SECONDS_PER_YEAR: u64 = 365 * 24 * 60 * 60;

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, id)
}

fn fixed_rate(rate_bps: i128) -> RateParams {
    RateParams {
        base_rate_bps: rate_bps,
        rate_floor_bps: rate_bps,
        rate_ceiling_bps: rate_bps,
        multiplier_bps: 0,
        jump_multiplier_bps: 0,
        surcharge_slope: 0,
        ..RateParams::default()
    }
}

fn set_pool(env: &Env, contract_id: &Address, debt: i128, supply: i128) {
    env.as_contract(contract_id, || {
        env.storage().persistent().set(&DataKey::TotalDebt, &debt);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &supply);
    });
}

fn assert_invalid_params(client: &LendingContractClient, params: &RateParams) {
    assert!(matches!(
        client.try_set_rate_params(params),
        Err(Ok(LendingError::InvalidRateParams))
    ));
}

#[test]
fn parameter_change_checkpoints_interest_at_the_old_rate_when_raising_or_lowering() {
    for (old_rate, new_rate) in [(1_000, 9_000), (9_000, 1_000)] {
        let (env, client, contract_id) = setup();
        let user = Address::generate(&env);
        set_pool(&env, &contract_id, 10_000, 10_000);
        env.as_contract(&contract_id, || {
            debt::save_debt(
                &env,
                &user,
                &DebtPosition {
                    principal: 10_000,
                    borrow_index_snapshot: INDEX_SCALE,
                    last_update: 0,
                },
            );
        });

        client.set_rate_params(&fixed_rate(old_rate));
        let elapsed = SECONDS_PER_YEAR / 3;
        env.ledger().set_timestamp(elapsed);
        client.set_rate_params(&fixed_rate(new_rate));

        let expected_index = debt::accrue_index(INDEX_SCALE, elapsed, old_rate);
        let actual_index = client.get_borrow_index();
        assert_eq!(actual_index, expected_index);
        let position = client.get_debt_position(&user);
        assert_eq!(
            debt::compute_debt(&position, actual_index),
            10_000 * expected_index / INDEX_SCALE
        );
    }
}

#[test]
fn a_second_parameter_change_in_the_same_ledger_has_zero_elapsed_index_accrual() {
    let (env, client, contract_id) = setup();
    set_pool(&env, &contract_id, 10_000, 10_000);
    client.set_rate_params(&fixed_rate(1_000));

    let now = SECONDS_PER_YEAR / 4;
    env.ledger().set_timestamp(now);
    client.set_rate_params(&fixed_rate(2_000));
    let first_checkpoint = client.get_borrow_index();

    client.set_rate_params(&fixed_rate(3_000));
    assert_eq!(client.get_borrow_index(), first_checkpoint);
    env.as_contract(&contract_id, || {
        assert_eq!(debt::load_last_index_update(&env), now);
    });
}

#[test]
fn parameter_change_with_no_debt_still_advances_the_index_timestamp() {
    let (env, client, contract_id) = setup();
    let now = SECONDS_PER_YEAR;
    env.ledger().set_timestamp(now);

    client.set_rate_params(&fixed_rate(2_000));
    assert_eq!(client.get_borrow_index(), INDEX_SCALE);
    env.as_contract(&contract_id, || {
        assert_eq!(debt::load_last_index_update(&env), now);
    });
}

#[test]
fn rate_parameter_bounds_accept_the_limit_and_reject_values_above_it() {
    let (_env, client, _contract_id) = setup();

    let ceiling_at_limit = RateParams {
        rate_ceiling_bps: rate_model::MAX_RATE_CEILING_BPS,
        ..RateParams::default()
    };
    client.set_rate_params(&ceiling_at_limit);
    assert_invalid_params(
        &client,
        &RateParams {
            rate_ceiling_bps: rate_model::MAX_RATE_CEILING_BPS + 1,
            ..RateParams::default()
        },
    );

    let slope_setters: [fn(&mut RateParams, i128); 3] = [
        |p: &mut RateParams, v| p.multiplier_bps = v,
        |p: &mut RateParams, v| p.jump_multiplier_bps = v,
        |p: &mut RateParams, v| p.surcharge_slope = v,
    ];
    for slope in slope_setters {
        let mut at_limit = RateParams::default();
        slope(&mut at_limit, rate_model::MAX_RATE_SLOPE_BPS);
        client.set_rate_params(&at_limit);

        let mut above_limit = RateParams::default();
        slope(&mut above_limit, rate_model::MAX_RATE_SLOPE_BPS + 1);
        assert_invalid_params(&client, &above_limit);
    }

    client.set_rate_params(&RateParams {
        kink_utilization_bps: 10_000,
        max_rate_change_per_ledger_bps: 0,
        ..RateParams::default()
    });
    assert_invalid_params(
        &client,
        &RateParams {
            kink_utilization_bps: 10_001,
            ..RateParams::default()
        },
    );
    assert_invalid_params(
        &client,
        &RateParams {
            max_rate_change_per_ledger_bps: -1,
            ..RateParams::default()
        },
    );
}

#[test]
fn smoothing_rate_is_clamped_to_the_new_curve_bounds() {
    let (env, _client, contract_id) = setup();
    env.as_contract(&contract_id, || {
        let params = RateParams {
            rate_ceiling_bps: 500,
            ..RateParams::default()
        };
        env.storage()
            .instance()
            .set(&rate_model::RateModelKey::LastRate, &900i128);
        env.storage().instance().set(
            &rate_model::RateModelKey::LastRateLedger,
            &env.ledger().sequence(),
        );

        assert_eq!(rate_model::update_and_get_rate(&env, 400, &params), 500);
        assert_eq!(
            env.storage()
                .instance()
                .get(&rate_model::RateModelKey::LastRate),
            Some(500)
        );
    });
}

#[test]
fn invalid_persisted_curve_falls_back_to_the_last_rate() {
    let (env, _client, contract_id) = setup();
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&rate_model::RateModelKey::LastRate, &321i128);
        let computation = debt::compute_borrow_rate_from_snapshot(
            &env,
            &debt::RateSnapshot {
                total_debt: 1,
                total_supply: 1,
                params: Some(RateParams {
                    max_rate_change_per_ledger_bps: -1,
                    ..RateParams::default()
                }),
            },
        );
        assert_eq!(computation.rate_bps, 321);
    });
}

#[test]
fn repay_succeeds_after_ten_idle_years_at_maximum_accepted_rate() {
    let (env, client, _contract_id) = setup();
    let user = Address::generate(&env);
    client.deposit(&user, &2_000_000);
    client.borrow(&user, &10_000);

    let max_rate_params = RateParams {
        base_rate_bps: rate_model::MAX_RATE_CEILING_BPS,
        rate_floor_bps: rate_model::MAX_RATE_CEILING_BPS,
        rate_ceiling_bps: rate_model::MAX_RATE_CEILING_BPS,
        ..RateParams::default()
    };
    client.set_rate_params(&max_rate_params);
    env.ledger().set_timestamp(10 * SECONDS_PER_YEAR);

    let position = client.get_debt_position(&user);
    let ten_year_index = debt::accrue_index(
        INDEX_SCALE,
        10 * SECONDS_PER_YEAR,
        rate_model::MAX_RATE_CEILING_BPS,
    );
    let debt_due = debt::compute_debt(&position, ten_year_index);
    assert_eq!(client.repay(&user, &debt_due), 0);
}

proptest! {
    #[test]
    fn every_valid_rate_params_curve_computes_for_all_utilizations(
        base_rate_bps in -1i128..200_002,
        kink_utilization_bps in -1i128..10_002,
        multiplier_bps in -1i128..200_002,
        jump_multiplier_bps in -1i128..200_002,
        rate_floor_bps in -1i128..100_002,
        rate_ceiling_bps in -1i128..100_002,
        max_rate_change_per_ledger_bps in -1i128..200_002,
        hysteresis_bps in -1i128..200_002,
        surcharge_kink_bps in -1i128..20_002,
        surcharge_slope in -1i128..200_002,
    ) {
        let params = RateParams {
            base_rate_bps,
            kink_utilization_bps,
            multiplier_bps,
            jump_multiplier_bps,
            rate_floor_bps,
            rate_ceiling_bps,
            max_rate_change_per_ledger_bps,
            hysteresis_bps,
            surcharge_kink_bps,
            surcharge_slope,
        };
        if params.validate().is_ok() {
            for utilization_bps in 0..=10_000 {
                prop_assert!(
                    rate_model::compute_borrow_rate(utilization_bps, &params).is_ok()
                );
            }
        }
    }
}
