//! Deterministic read-cost benchmarks for the `get_twap` snapshot lookup.
//!
//! Comparison counts are used instead of wall-clock timings so the budget is
//! stable across developer machines and CI runners.

use soroban_sdk::{testutils::Ledger, Address, Env, Vec};

use crate::amm_twap::{
    get_snapshots, get_twap, snapshot_search_metrics_for_test, update_twap_accumulators,
    TwapSnapshot, MAX_SNAPSHOTS, PRICE_SCALE, SNAPSHOT_INTERVAL_SECS,
    TWAP_READ_SEARCH_COMPARISON_BUDGET,
};

const BENCHMARK_SNAPSHOT_COUNTS: [u32; 6] = [1, 4, 16, 64, 512, MAX_SNAPSHOTS];

/// Advances the mock ledger by `seconds` without writing a snapshot.
fn advance(env: &Env, seconds: u64) {
    env.ledger()
        .set_timestamp(env.ledger().timestamp().saturating_add(seconds));
}

/// Builds an on-chain snapshot vector with `count` entries at a stable 1:1 price.
fn fill_snapshot_vector(env: &Env, asset: &Address, count: u32) {
    env.ledger().set_timestamp(0);
    update_twap_accumulators(env, asset, 1_000_000, 1_000_000);

    for _ in 0..count {
        advance(env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(env, asset, 1_000_000, 1_000_000);
    }
}

/// Returns the worst-case binary-search comparison count for `item_count` entries.
fn binary_search_budget(item_count: u32) -> u32 {
    let mut remaining = item_count;
    let mut comparisons = 0u32;
    while remaining > 0 {
        comparisons = comparisons.saturating_add(1);
        remaining /= 2;
    }
    comparisons
}

/// Returns a sorted synthetic snapshot vector with timestamps 60, 120, ... .
fn synthetic_snapshots(env: &Env, count: u32) -> Vec<TwapSnapshot> {
    let mut snapshots = Vec::new(env);
    for index in 1..=count {
        let timestamp = u64::from(index) * SNAPSHOT_INTERVAL_SECS;
        snapshots.push_back(TwapSnapshot {
            timestamp,
            price0_cumulative: u128::from(timestamp) * PRICE_SCALE,
            price1_cumulative: u128::from(timestamp) * PRICE_SCALE,
        });
    }
    snapshots
}

#[test]
fn get_twap_read_cost_stays_logarithmic_as_snapshots_grow() {
    assert_eq!(
        binary_search_budget(MAX_SNAPSHOTS),
        TWAP_READ_SEARCH_COMPARISON_BUDGET
    );

    for snapshot_count in BENCHMARK_SNAPSHOT_COUNTS {
        let env = Env::default();
        let asset = Address::generate(&env);
        fill_snapshot_vector(&env, &asset, snapshot_count);

        let snapshots = get_snapshots(&env, &asset);
        assert_eq!(snapshots.len(), snapshot_count);

        // Query one interval after the latest write so the target lands on the
        // newest snapshot and the complete get_twap path has a non-zero window.
        advance(&env, SNAPSHOT_INTERVAL_SECS);
        let target_timestamp = env
            .ledger()
            .timestamp()
            .saturating_sub(SNAPSHOT_INTERVAL_SECS);
        let (anchor, comparisons) = snapshot_search_metrics_for_test(&snapshots, target_timestamp);

        assert_eq!(
            anchor.map(|snapshot| snapshot.timestamp),
            Some(target_timestamp)
        );
        assert!(
            comparisons <= binary_search_budget(snapshot_count),
            "{snapshot_count} snapshots required {comparisons} comparisons"
        );
        assert!(
            comparisons <= TWAP_READ_SEARCH_COMPARISON_BUDGET,
            "lookup exceeded the global read budget"
        );
        assert_eq!(get_twap(&env, &asset, SNAPSHOT_INTERVAL_SECS), PRICE_SCALE);
    }
}

#[test]
fn snapshot_lookup_budget_covers_vector_ends_and_middle() {
    let env = Env::default();
    let snapshot_count = 64u32;
    let snapshots = synthetic_snapshots(&env, snapshot_count);
    let first_timestamp = SNAPSHOT_INTERVAL_SECS;
    let middle_timestamp = (u64::from(snapshot_count / 2) + 1) * SNAPSHOT_INTERVAL_SECS;
    let last_timestamp = u64::from(snapshot_count) * SNAPSHOT_INTERVAL_SECS;
    let cases = [
        (first_timestamp - 1, None),
        (first_timestamp, Some(first_timestamp)),
        (middle_timestamp + 1, Some(middle_timestamp)),
        (last_timestamp, Some(last_timestamp)),
        (
            last_timestamp + SNAPSHOT_INTERVAL_SECS,
            Some(last_timestamp),
        ),
    ];

    for (target_timestamp, expected_timestamp) in cases {
        let (anchor, comparisons) = snapshot_search_metrics_for_test(&snapshots, target_timestamp);
        assert_eq!(
            anchor.map(|snapshot| snapshot.timestamp),
            expected_timestamp,
            "wrong anchor for target {target_timestamp}"
        );
        assert!(
            comparisons <= binary_search_budget(snapshot_count),
            "target {target_timestamp} exceeded the per-size budget"
        );
        assert!(comparisons <= TWAP_READ_SEARCH_COMPARISON_BUDGET);
    }
}
