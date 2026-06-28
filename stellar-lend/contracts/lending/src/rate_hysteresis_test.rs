#![cfg(test)]

use crate::rate_model::{compute_smoothed_rate, RateParams};
use crate::{DataKey, LendingContract, LendingContractClient};
use soroban_sdk::{testutils::Ledger, Address, Env};

fn setup_with_params(
    params: RateParams,
) -> (Env, LendingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_sequence(100);

    let id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);

    env.as_contract(&id, || {
        env.storage().instance().set(&DataKey::RateParams, &params);
    });

    (env, client, admin, user)
}

#[test]
fn band_zero_preserves_legacy_behavior() {
    let legacy = compute_smoothed_rate(1_000, 1_040, 10, 1, 0);
    let with_zero_band = compute_smoothed_rate(1_000, 1_040, 10, 1, 0);
    assert_eq!(with_zero_band, legacy);
}

#[test]
fn target_exactly_at_band_edge_holds_current_rate() {
    assert_eq!(compute_smoothed_rate(1_000, 1_025, 10, 5, 25), 1_000);
    assert_eq!(compute_smoothed_rate(1_000, 975, 10, 5, 25), 1_000);
}

#[test]
fn target_inside_band_holds_current_rate() {
    assert_eq!(compute_smoothed_rate(1_000, 1_020, 10, 5, 25), 1_000);
    assert_eq!(compute_smoothed_rate(1_000, 985, 10, 5, 25), 1_000);
}

#[test]
fn large_move_still_converges_from_band_edge() {
    assert_eq!(compute_smoothed_rate(1_000, 1_200, 20, 1, 25), 1_020);
    assert_eq!(compute_smoothed_rate(1_020, 1_200, 20, 8, 25), 1_175);
    assert_eq!(compute_smoothed_rate(1_175, 1_200, 20, 8, 25), 1_175);
}

#[test]
fn overflow_delta_attempt_is_checked() {
    let rate = compute_smoothed_rate(i128::MIN, i128::MAX, 1, 1, i128::MAX);
    assert_eq!(rate, -1);
}

#[test]
fn contract_view_keeps_rate_flat_inside_band_and_respects_clamp() {
    let mut params = RateParams::default();
    params.max_rate_change_per_ledger_bps = 50;
    params.hysteresis_bps = 100;
    params.rate_floor_bps = 1_100;
    params.rate_ceiling_bps = 1_760;

    let (env, client, _admin, user) = setup_with_params(params);

    client.deposit(&user, &10_000);
    client.borrow(&user, &8_000);

    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_700);
    });

    env.ledger().set_sequence(101);
    client.borrow(&user, &100);

    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_700);
    });

    env.ledger().set_sequence(102);
    client.borrow(&user, &900);

    env.as_contract(&client.address, || {
        assert_eq!(crate::current_borrow_rate(&env), 1_760);
    });
}
