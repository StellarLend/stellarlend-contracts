#![cfg(test)]

use crate::rate_model::{update_and_get_rate, RateParams};
use crate::{LendingContract, LendingContractClient, RateSmoothingState};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

fn setup() -> (Env, LendingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence_number(100);

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    (env, client, id)
}

fn smoothing_params() -> RateParams {
    let mut params = RateParams::default();
    params.max_rate_change_per_ledger_bps = 50;
    params.hysteresis_bps = 0;
    params
}

#[test]
fn rate_smoothing_state_defaults_when_uninitialized() {
    let (_env, client, _id) = setup();

    assert_eq!(
        client.get_rate_smoothing_state(),
        RateSmoothingState {
            schema_version: 1,
            current_rate_bps: 0,
            last_target_rate_bps: 0,
            last_update_ledger: 0,
        }
    );
}

#[test]
fn rate_smoothing_state_matches_single_persisted_update() {
    let (env, client, id) = setup();
    let params = smoothing_params();

    env.as_contract(&id, || {
        assert_eq!(update_and_get_rate(&env, 1_700, &params), 1_700);
    });

    assert_eq!(
        client.get_rate_smoothing_state(),
        RateSmoothingState {
            schema_version: 1,
            current_rate_bps: 1_700,
            last_target_rate_bps: 1_700,
            last_update_ledger: 100,
        }
    );
}

#[test]
fn rate_smoothing_state_tracks_several_updates_without_recomputation() {
    let (env, client, id) = setup();
    let params = smoothing_params();

    env.as_contract(&id, || {
        assert_eq!(update_and_get_rate(&env, 1_700, &params), 1_700);
    });

    env.ledger().set_sequence_number(101);
    env.as_contract(&id, || {
        assert_eq!(update_and_get_rate(&env, 2_700, &params), 1_750);
    });

    env.ledger().set_sequence_number(111);
    env.as_contract(&id, || {
        assert_eq!(update_and_get_rate(&env, 900, &params), 1_250);
    });

    assert_eq!(
        client.get_rate_smoothing_state(),
        RateSmoothingState {
            schema_version: 1,
            current_rate_bps: 1_250,
            last_target_rate_bps: 900,
            last_update_ledger: 111,
        }
    );
}

#[test]
fn rate_smoothing_state_schema_is_stable_for_indexers() {
    let (_env, client, _id) = setup();
    let state = client.get_rate_smoothing_state();

    assert_eq!(state.schema_version, 1);
    let RateSmoothingState {
        schema_version,
        current_rate_bps,
        last_target_rate_bps,
        last_update_ledger,
    } = state;
    assert_eq!(schema_version, 1);
    assert_eq!(current_rate_bps, 0);
    assert_eq!(last_target_rate_bps, 0);
    assert_eq!(last_update_ledger, 0);
}
