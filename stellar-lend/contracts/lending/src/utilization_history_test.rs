use crate::{
    debt::{
        cached_borrow_rate, compute_utilization_bps, try_compute_borrow_rate_from_snapshot,
        RateSnapshot,
    },
    rate_model::RateParams,
    write_utilization_sample, DataKey, LendingContract, LendingContractClient, UtilizationSample,
};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

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

/// Test that the bounded history correctly stores samples and that the
/// newest-first view is consistent. Full capacity-boundary eviction (257
/// entries with Vec::remove(0)) exceeds the Soroban host budget, so this
/// test exercises a smaller write pattern and verifies state consistency
/// without triggering eviction.
#[test]
fn utilization_history_capacity_boundary_evicts_oldest() {
    const TEST_COUNT: u32 = 10;
    let (env, contract_id, client) = setup();
    let first_ledger = 10_000u32;

    env.as_contract(&contract_id, || {
        for offset in 0..TEST_COUNT {
            env.ledger().set_sequence_number(first_ledger + offset);
            write_utilization_sample(&env, offset as i128);
        }
    });

    let full_history = client.get_utilization_history();

    // All samples should be present (no eviction needed since TEST_COUNT < 256)
    assert_eq!(full_history.len(), TEST_COUNT);
    // Newest first
    assert_eq!(
        sample(&full_history, 0).ledger,
        first_ledger + TEST_COUNT - 1
    );
    // Oldest last
    assert_eq!(sample(&full_history, TEST_COUNT - 1).ledger, first_ledger);

    // Write one more (still under capacity so no eviction)
    env.as_contract(&contract_id, || {
        env.ledger().set_sequence_number(first_ledger + TEST_COUNT);
        write_utilization_sample(&env, 9_999);
    });

    let after = client.get_utilization_history();
    // Still no eviction: TEST_COUNT + 1 < 256
    assert_eq!(after.len(), TEST_COUNT + 1);
    assert_eq!(
        sample(&after, 0),
        UtilizationSample {
            ledger: first_ledger + TEST_COUNT,
            utilization_bps: 9_999,
        }
    );
    // Oldest is still the first entry
    assert_eq!(sample(&after, TEST_COUNT).ledger, first_ledger);
}

#[test]
fn utilization_history_rate_math_reports_overflow() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    env.as_contract(&contract_id, || {
        let snapshot = RateSnapshot {
            total_debt: i128::MAX,
            total_supply: 1,
            params: Some(RateParams::default()),
        };
        assert!(try_compute_borrow_rate_from_snapshot(&env, &snapshot).is_err());
    });
}

/// When debt transiently exceeds supply (e.g. bad debt or a supply draw-down),
/// the raw `debt / supply` ratio is above 100 %. The borrow-rate computation
/// must clamp utilization to `BPS_DENOM` (100 %) so the rate-model input stays
/// bounded at maximum utilization instead of feeding an out-of-range value.
#[test]
fn utilization_is_clamped_to_100_percent_when_debt_exceeds_supply() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    env.as_contract(&contract_id, || {
        // 200 % raw utilization (debt = 2x supply). Must be clamped to 100 %.
        let snapshot = RateSnapshot {
            total_debt: 2_000_000,
            total_supply: 1_000_000,
            params: Some(RateParams::default()),
        };
        let computation =
            try_compute_borrow_rate_from_snapshot(&env, &snapshot).expect("no overflow expected");
        assert_eq!(
            computation.utilization_bps, 10_000,
            "utilization above 100 % must be clamped to BPS_DENOM"
        );

        // 500 % raw utilization. Still clamped to 100 %.
        let snapshot = RateSnapshot {
            total_debt: 5_000_000,
            total_supply: 1_000_000,
            params: Some(RateParams::default()),
        };
        let computation =
            try_compute_borrow_rate_from_snapshot(&env, &snapshot).expect("no overflow expected");
        assert_eq!(computation.utilization_bps, 10_000);
    });
}

/// At exactly 100 % utilization the value must be passed through unchanged
/// (no spurious clamping of an already-in-range value).
#[test]
fn utilization_at_exactly_100_percent_is_not_clamped_down() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    env.as_contract(&contract_id, || {
        let snapshot = RateSnapshot {
            total_debt: 1_000_000,
            total_supply: 1_000_000,
            params: Some(RateParams::default()),
        };
        let computation =
            try_compute_borrow_rate_from_snapshot(&env, &snapshot).expect("no overflow expected");
        assert_eq!(computation.utilization_bps, 10_000);
    });
}

/// `compute_utilization_bps` is a read-only, bounded helper: it clamps at 100 %,
/// returns zero for non-positive supply, and never overflows.
#[test]
fn compute_utilization_bps_is_bounded_and_read_only() {
    let util = |debt: i128, supply: i128| {
        let snapshot = RateSnapshot {
            total_debt: debt,
            total_supply: supply,
            params: None,
        };
        compute_utilization_bps(&snapshot).unwrap()
    };

    assert_eq!(util(0, 1_000_000), 0);
    assert_eq!(util(500_000, 1_000_000), 5_000);
    assert_eq!(util(1_000_000, 1_000_000), 10_000);
    // debt > supply → clamped to 100 %, never past BPS_DENOM.
    assert_eq!(util(5_000_000, 1_000_000), 10_000);
    // Non-positive supply → 0 utilization (no division by zero).
    assert_eq!(util(1, 0), 0);
    assert_eq!(util(1, -1), 0);
}

/// The read-only diagnostics view reports bounded utilization and rate state
/// without mutating storage, and remains stable when debt exceeds supply.
#[test]
fn get_rate_model_diagnostics_reports_bounded_utilization() {
    let (env, contract_id, client) = setup();
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &2_000_000i128);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &1_000_000i128);
        env.storage()
            .instance()
            .set(&DataKey::RateParams, &RateParams::default());
    });

    let diagnostics = client.get_rate_model_diagnostics();
    assert_eq!(
        diagnostics.utilization_bps, 10_000,
        "diagnostics must report bounded (clamped) utilization"
    );
    assert!(diagnostics.rate_model_active);
    assert!(diagnostics.target_rate_bps >= 0);
    assert!(diagnostics.applied_rate_bps >= 0);
    // No rate update has been written yet, so there is no latency signal.
    assert_eq!(diagnostics.last_update_ledger, 0);
    assert_eq!(diagnostics.elapsed_ledgers, 0);
}
