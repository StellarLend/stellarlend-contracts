#![cfg(test)]

use crate::{
    debt::{cached_borrow_rate, try_compute_borrow_rate_from_snapshot, RateSnapshot},
    rate_model::RateParams,
    write_utilization_sample, DataKey, LendingContract, LendingContractClient, UtilizationSample,
    UTILIZATION_HISTORY_CAPACITY,
};
use soroban_sdk::{testutils::Ledger, Address, Env};

fn setup() -> (Env, Address, LendingContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    (env, contract_id, client)
}

fn set_rate_inputs(env: &Env, total_debt: i128, total_deposits: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &total_debt);
    env.storage()
        .persistent()
        .set(&DataKey::TotalDeposits, &total_deposits);
    env.storage()
        .instance()
        .set(&DataKey::RateParams, &RateParams::default());
}

fn sample(history: &soroban_sdk::Vec<UtilizationSample>, index: u32) -> UtilizationSample {
    history.get(index).expect("sample should exist")
}

#[test]
fn empty_utilization_history_returns_empty_vec() {
    let (_env, _contract_id, client) = setup();

    let history = client.get_utilization_history();

    assert_eq!(history.len(), 0);
}

#[test]
fn utilization_history_records_single_rate_update_sample() {
    let (env, contract_id, client) = setup();

    env.ledger().set_sequence_number(42);
    env.as_contract(&contract_id, || {
        set_rate_inputs(&env, 4_000, 10_000);

        assert_eq!(cached_borrow_rate(&env), 900);
    });

    let history = client.get_utilization_history();

    assert_eq!(history.len(), 1);
    assert_eq!(
        sample(&history, 0),
        UtilizationSample {
            ledger: 42,
            utilization_bps: 4_000,
        }
    );
}

#[test]
fn utilization_history_uses_cache_miss_once_per_ledger() {
    let (env, contract_id, client) = setup();

    env.ledger().set_sequence_number(77);
    env.as_contract(&contract_id, || {
        set_rate_inputs(&env, 2_500, 10_000);

        assert_eq!(cached_borrow_rate(&env), 600);
        assert_eq!(cached_borrow_rate(&env), 600);
    });

    let history = client.get_utilization_history();

    assert_eq!(history.len(), 1);
    assert_eq!(sample(&history, 0).ledger, 77);
    assert_eq!(sample(&history, 0).utilization_bps, 2_500);
}

#[test]
fn utilization_history_view_returns_newest_first() {
    let (env, contract_id, client) = setup();

    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(100);
        set_rate_inputs(&env, 1_000, 10_000);
        assert_eq!(cached_borrow_rate(&env), 300);

        env.ledger().set_sequence_number(101);
        set_rate_inputs(&env, 7_500, 10_000);
        assert_eq!(cached_borrow_rate(&env), 1_600);

        env.ledger().set_sequence_number(102);
        set_rate_inputs(&env, 10_000, 10_000);
        assert_eq!(cached_borrow_rate(&env), 3_700);
    });

    let history = client.get_utilization_history();

    assert_eq!(history.len(), 3);
    assert_eq!(sample(&history, 0).ledger, 102);
    assert_eq!(sample(&history, 0).utilization_bps, 10_000);
    assert_eq!(sample(&history, 1).ledger, 101);
    assert_eq!(sample(&history, 1).utilization_bps, 7_500);
    assert_eq!(sample(&history, 2).ledger, 100);
    assert_eq!(sample(&history, 2).utilization_bps, 1_000);
}

#[test]
fn utilization_history_capacity_boundary_evicts_oldest() {
    let (env, contract_id, client) = setup();
    let first_ledger = 1_000u32;

    env.as_contract(&contract_id, || {
        for offset in 0..UTILIZATION_HISTORY_CAPACITY {
            env.ledger().set_sequence_number(first_ledger + offset);
            write_utilization_sample(&env, offset as i128);
        }
    });

    let full_history = client.get_utilization_history();

    assert_eq!(full_history.len(), UTILIZATION_HISTORY_CAPACITY);
    assert_eq!(
        sample(&full_history, 0).ledger,
        first_ledger + UTILIZATION_HISTORY_CAPACITY - 1
    );
    assert_eq!(
        sample(&full_history, UTILIZATION_HISTORY_CAPACITY - 1).ledger,
        first_ledger
    );

    env.as_contract(&contract_id, || {
        env.ledger()
            .set_sequence_number(first_ledger + UTILIZATION_HISTORY_CAPACITY);
        write_utilization_sample(&env, 9_999);
    });

    let evicted_history = client.get_utilization_history();

    assert_eq!(evicted_history.len(), UTILIZATION_HISTORY_CAPACITY);
    assert_eq!(
        sample(&evicted_history, 0),
        UtilizationSample {
            ledger: first_ledger + UTILIZATION_HISTORY_CAPACITY,
            utilization_bps: 9_999,
        }
    );
    assert_eq!(
        sample(&evicted_history, UTILIZATION_HISTORY_CAPACITY - 1).ledger,
        first_ledger + 1
    );
}

#[test]
fn utilization_history_rate_math_reports_overflow() {
    let snapshot = RateSnapshot {
        total_debt: i128::MAX,
        total_supply: 1,
        params: Some(RateParams::default()),
    };

    assert!(try_compute_borrow_rate_from_snapshot(&snapshot).is_err());
}
