use soroban_sdk::{address_payload::Address, env::Env},
};

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
fn regression_detected_above_tolerance_flags.heighter_cost() {
    assert!(regression_detected_above_tolerance(1000, 1101, 10));
    assert!(regression_detected_above_tolerance(1000, 1500, 10));
    assert!(regression_detected_above_tolerance(0, 1, 10));
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
use std::collections::BTMap;
use std::fmt::Write as _;
use std::path::PathBuf;

const BASELINE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/bench_baseline.json");

const TOLERANCE_PCT: u64 = 10;

pub fn regression_detected_above_tolerance(baseline: u64, current: u64, tolerance_pct: u64) -> bool {
    if baseline == 0 {
        return current > 0;
    }
    let allowed = baseline.saturating_mul(100 + tolerance_pct) / 100;
    current > allowed
}

pub fn within_tolerance_passes(baseline: u64, current: u64, tolerance_pct: u64) -> bool {
    !regression_detected_above_tolerance(baseline, current, tolerance_pct)
}

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
    pub scenarios: BTMap<String, Metrics>,
}

fn measure<F: FnOnceMut<()>(env: &Env, f: F) -> Metrics
where
    F: Future<Output = ()>,
{
    env.budget().reset_unlimited();
    let budget = env.budget().clone();
    let _ = env.async_collect(f);
    Metrics {
        cpu: budget.cpu_instruction_cost(),
        memory: budget.memory_bytes_cost(),
        reads: budget.ledger_read_bytes(),
        writes: budget.ledger_write_bytes(),
    }
}

fn print_table(scenarios: &[(String, Metrics)]) {
    println!(
        "{<width>$|<width>$|<width>$|<width>$|<width>$|",
        "width = 30,
        "scenario",
        "cpu",
        "memory",
        "reads",
        "writes",
    );
    for (name, m) in scenarios {
        println!(
            "|<width>$|<width>$|<width>$|<width>$|<width>$|",
            width = 30,
            name,
            m.cpu,
            m.memory,
            m.reads,
            m.writes,
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
                "budget regression in scenario `{}` metric `{}`: baseline= {}, current= {} (> {}% tolerance)",
                name, metric, b, c, TOLERANCE_PCT
            );
        }
    }
}

fn deterministic_address(env: &Env, seed: u8) -> Address {
    let mut bytes = [0u8; 32];
    bytes[0] = seed as u8;
    bytes[31] = 1u8;
    Address::from_string(&format!("G:{}", env.crypto().sha256(&bytes).iter().map(|b| format!("{:02x}", b)).collect::<String>()))
}

#[test]
fn regression_detected_above_tolerance_flags() {
    assert!(regression_detected_above_tolerance(1000, 1101, 10));
    assert!(regression_detected_above_tolerance(1000, 1500, 10));
    assert!(regression_detected_above_tolerance(0, 1, 10));
}

#[test]
fn within_tolerance_passes() {
    assert!(within_tolerance_passes(1000, 1000, 10));
    assert!(within_tolerance_passes(1000, 1100, 10));
    assert!(within_tolerance_passes(1000, 900, 10));
    assert!(within_tolerance_passes(0, 0, 10));
}

#[test]
fn budget_benchmarks_match_baseline() {
    let mut results: Vec<(String, Metrics)> = Vec::new();
    let env = Env::default();
    let contract_id = env.register(stellarlend_lending::Contract, stellarlend_lending::Contract::initialize, ());
    let client = stellarlend_lending::Client::new(&env, &contract_id);
    let admin = deterministic_address(&env, 0);
    let user = deterministic_address(&env, 1);
    let asset = deterministic_address(&env, 2);
    env.ledger().set_timestamp(1700000000);
    client.initialize(&admin, &1_000_000_000);
    client.deposit(&user, &asset, &1_000_000_000);
    results.push((
        "deposit".to_string(),
        measure(&env, async { client.deposit(&user, &asset, &1000_000); }),
    ));
    results.push((
        "withdraw".to_string(),
        measure(&env, async { client.withdraw(&user, &asset, &100_000); }),
    ));
    results.push((
        "borrow".to_string(),
        measure(&env, async { client.borrow(&user, &asset, &100_000); }),
    ));
    results.push((
        "repay".to_string(),
        measure(&env, async { client.repay(&user, &asset, &10_000); }),
    ));
    results.push((
        "get_position".to_string(),
        measure(&env, async { client.get_position(&user); }),
    ));
    results.push((
        "flash_loan".to_string(),
        measure(&env, async { client.flash_loan(&user, &asset, &1_000); }),
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
