/// Percentage tolerance allowed before a metric is considered a regression.
pub const TOLERANCE_PCT: u64 = 10;

/// Returns true when `current` exceeds `baseline` by more than `tolerance_pct`.
///
/// A zero baseline is treated as "any non-zero value is a regression".
pub fn regression_detected_above_tolerance(baseline: u64, current: u64, tolerance_pct: u64) -> bool {
    if baseline == 0 {
        return current > 0;
    }
    let allowed = baseline.saturating_mul(100 + tolerance_pct) / 100;
    current > allowed
}

/// Returns true when `current` is within the tolerance of `baseline`.
pub fn within_tolerance_passes(baseline: u64, current: u64, tolerance_pct: u64) -> bool {
    !regression_detected_above_tolerance(baseline, current, tolerance_pct)
}

#[test]
fn within_tolerance_passes_lower_cost() {
    assert!(within_tolerance_passes(1000, 1000, 10));
    assert!(within_tolerance_passes(1000, 1100, 10));
    assert!(within_tolerance_passes(1000, 900, 10));
    assert!(within_tolerance_passes(0, 0, 10));
}

// --------------------------------------------------------------------------------
// Budget benchmark suite
// --------------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env};
use std::collections::BTreeMap;

const BASELINE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/bench_baseline.json");

/// A single measured budget sample for an entrypoint scenario.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Metrics {
    pub cpu: u64,
    pub memory: u64,
    pub reads: u64,
    pub writes: u64,
}

/// Baseline file shape: scenario name -> metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaselineFile {
    pub scenarios: BTreeMap<String, Metrics>,
}

fn measure<F: FnOnce()>(env: &Env, f: F) -> Metrics {
    let budget = env.cost_estimate().budget();
    budget.reset_unlimited();
    f();
    Metrics {
        cpu: budget.cpu_instruction_cost(),
        memory: budget.memory_bytes_cost(),
        reads: budget.ledger_read_bytes(),
        writes: budget.ledger_write_bytes(),
    }
}

fn print_table(scenarios: &[(String, Metrics)]) {
    println!(
        "| {:<30} | {:>14} | {:>14} | {:>14} | {:>14} |",
        "scenario", "cpu", "memory", "reads", "writes"
    );
    for (name, m) in scenarios {
        println!(
            "| {:<30} | {:>14} | {:>14} | {:>14} | {:>14} |",
            name, m.cpu, m.memory, m.reads, m.writes
        );
    }
}

fn load_baseline() -> BaselineFile {
    match std::fs::read_to_string(BASELINE_PATH) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => BaselineFile::default(),
    }
}

fn save_baseline(file: &BaselineFile) {
    let json = serde_json::to_string_pretty(file).expect("serialize baseline");
    std::fs::write(BASELINE_PATH, json).expect("write baseline");
}

fn assert_within_baseline(name: &str, baseline: &Metrics, current: &Metrics) {
    for (metric, b, c) in [
        ("cpu", baseline.cpu, current.cpu),
        ("memory", baseline.memory, current.memory),
        ("reads", baseline.reads, current.reads),
        ("writes", baseline.writes, current.writes),
    ] {
        if regression_detected_above_tolerance(b, c, TOLERANCE_PCT) {
            panic!(
                "budget regression in scenario `{}` metric `{}`: baseline={}, current={} (> {}% tolerance)",
                name, metric, b, c, TOLERANCE_PCT
            );
        }
    }
}

#[test]
fn regression_detected_above_tolerance_flags() {
    assert!(regression_detected_above_tolerance(1000, 1101, 10));
    assert!(regression_detected_above_tolerance(1000, 1500, 10));
    assert!(regression_detected_above_tolerance(0, 1, 10));
}

#[test]
fn within_tolerance_passes_suite() {
    assert!(within_tolerance_passes(1000, 1000, 10));
    assert!(within_tolerance_passes(1000, 1100, 10));
    assert!(within_tolerance_passes(1000, 900, 10));
    assert!(within_tolerance_passes(0, 0, 10));
}

#[test]
fn budget_benchmarks_match_baseline() {
    let mut results: Vec<(String, Metrics)> = Vec::new();
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(stellarlend_lending::Contract, ());
    let client = stellarlend_lending::ContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let asset = Address::generate(&env);
    env.ledger().set_timestamp(1_700_000_000);
    client.initialize(&admin, &1_000_000_000);
    client.deposit(&user, &asset, &1_000_000_000);

    results.push((
        "deposit".to_string(),
        measure(&env, || {
            client.deposit(&user, &asset, &1_000_000);
        }),
    ));
    results.push((
        "withdraw".to_string(),
        measure(&env, || {
            client.withdraw(&user, &asset, &100_000);
        }),
    ));
    results.push((
        "borrow".to_string(),
        measure(&env, || {
            client.borrow(&user, &asset, &100_000);
        }),
    ));
    results.push((
        "repay".to_string(),
        measure(&env, || {
            client.repay(&user, &asset, &10_000);
        }),
    ));
    results.push((
        "get_position".to_string(),
        measure(&env, || {
            client.get_position(&user);
        }),
    ));
    results.push((
        "flash_loan".to_string(),
        measure(&env, || {
            client.flash_loan(&user, &asset, &1_000);
        }),
    ));

    print_table(&results);

    let mut baseline = load_baseline();
    if std::env::var("UPDATE_BENCH").map(|v| v == "1").unwrap_or(false) {
        for (name, m) in &results {
            baseline.scenarios.insert(name.clone(), m.clone());
        }
        save_baseline(&baseline);
        return;
    }
    for (name, m) in &results {
        if let Some(b) = baseline.scenarios.get(name) {
            assert_within_baseline(name, b, m);
        }
    }
}
