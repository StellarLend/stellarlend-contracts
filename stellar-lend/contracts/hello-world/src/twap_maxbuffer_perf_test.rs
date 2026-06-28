/// twap_maxbuffer_perf_test.rs — Full-buffer lookup budget tests for `get_twap`.
///
/// Coverage matrix
/// ───────────────
/// ✓ Snapshot ring filled to `MAX_SNAPSHOTS`
/// ✓ Short-window lookup remains within binary-search budget
/// ✓ Long-window lookup remains within binary-search budget
/// ✓ Lookup returns the bounding snapshot at or before the target timestamp
/// ✓ TWAP values remain unchanged under maximum buffer occupancy

#[cfg(test)]
mod tests {
    use soroban_sdk::{testutils::Ledger, Address, Env};

    use crate::amm_twap::{
        get_snapshots, get_twap, snapshot_search_metrics_for_test, update_twap_accumulators,
        MAX_SNAPSHOTS, MIN_WINDOW_SECS, PRICE_SCALE, SNAPSHOT_INTERVAL_SECS,
    };

    const LOOKUP_COMPARISON_BUDGET: u32 = 11;

    /// Advances the mock ledger by `secs` seconds.
    fn advance(env: &Env, secs: u64) {
        let now = env.ledger().timestamp();
        env.ledger().set_timestamp(now + secs);
    }

    /// Fills the snapshot ring to exactly `MAX_SNAPSHOTS` entries at a stable 1:1 price.
    fn fill_snapshot_ring(env: &Env, asset: &Address) {
        env.ledger().set_timestamp(0);
        update_twap_accumulators(env, asset, 1_000_000, 1_000_000);

        for _ in 0..MAX_SNAPSHOTS {
            advance(env, SNAPSHOT_INTERVAL_SECS);
            update_twap_accumulators(env, asset, 1_000_000, 1_000_000);
        }
    }

    /// Returns the expected worst-case comparison count for a binary search over `len` items.
    fn binary_search_budget(len: u32) -> u32 {
        let mut budget = 0u32;
        let mut span = len;
        while span > 0 {
            budget += 1;
            span /= 2;
        }
        budget
    }

    /// On a full ring, a near-tail lookup stays within the logarithmic budget and returns
    /// the latest snapshot at or before the requested window start.
    #[test]
    fn full_buffer_short_window_lookup_stays_within_budget() {
        let env = Env::default();
        let asset = Address::generate(&env);
        fill_snapshot_ring(&env, &asset);

        let snaps = get_snapshots(&env, &asset);
        assert_eq!(snaps.len(), MAX_SNAPSHOTS);

        let now = env.ledger().timestamp();
        let short_window = MIN_WINDOW_SECS * 2;
        let target_start = now.saturating_sub(short_window);

        let (start_snap, comparisons) = snapshot_search_metrics_for_test(&snaps, target_start);
        let start_snap = start_snap.expect("full ring should have a bounding snapshot");

        assert!(
            comparisons <= LOOKUP_COMPARISON_BUDGET,
            "expected <= {LOOKUP_COMPARISON_BUDGET} comparisons, got {comparisons}"
        );
        assert_eq!(
            comparisons,
            binary_search_budget(MAX_SNAPSHOTS),
            "full-ring lookup should stay on the binary-search budget line"
        );
        assert!(start_snap.timestamp <= target_start);

        let next_timestamp = start_snap.timestamp + SNAPSHOT_INTERVAL_SECS;
        assert!(
            next_timestamp > target_start || next_timestamp > now,
            "returned snapshot must be the last one at or before target_start"
        );

        assert_eq!(get_twap(&env, &asset, short_window), PRICE_SCALE);
    }

    /// On a full ring, a long-window lookup near the head still stays within the same budget
    /// and keeps the TWAP value unchanged.
    #[test]
    fn full_buffer_long_window_lookup_stays_within_budget() {
        let env = Env::default();
        let asset = Address::generate(&env);
        fill_snapshot_ring(&env, &asset);

        let snaps = get_snapshots(&env, &asset);
        let now = env.ledger().timestamp();
        let long_window = SNAPSHOT_INTERVAL_SECS * (MAX_SNAPSHOTS as u64 - 1);
        let target_start = now.saturating_sub(long_window);

        let (start_snap, comparisons) = snapshot_search_metrics_for_test(&snaps, target_start);
        let start_snap = start_snap.expect("full ring should have a long-window anchor");

        assert!(
            comparisons <= LOOKUP_COMPARISON_BUDGET,
            "expected <= {LOOKUP_COMPARISON_BUDGET} comparisons, got {comparisons}"
        );
        assert_eq!(comparisons, binary_search_budget(MAX_SNAPSHOTS));
        assert!(start_snap.timestamp <= target_start);
        assert_eq!(start_snap.timestamp, SNAPSHOT_INTERVAL_SECS);

        assert_eq!(get_twap(&env, &asset, long_window), PRICE_SCALE);
    }

    /// Targets that fall between two snapshots resolve immediately to the lower bound and do not
    /// degenerate into a linear walk across the buffer.
    #[test]
    fn lookup_short_circuits_to_the_bounding_snapshot() {
        let env = Env::default();
        let asset = Address::generate(&env);
        fill_snapshot_ring(&env, &asset);

        let snaps = get_snapshots(&env, &asset);
        let target_start = 95;

        let (start_snap, comparisons) = snapshot_search_metrics_for_test(&snaps, target_start);
        let start_snap = start_snap.expect("expected a bounding snapshot");

        assert_eq!(start_snap.timestamp, 60);
        assert!(
            comparisons < MAX_SNAPSHOTS,
            "lookup should remain sublinear, got {comparisons} comparisons for {} snapshots",
            MAX_SNAPSHOTS
        );
        assert!(
            comparisons <= LOOKUP_COMPARISON_BUDGET,
            "comparison count should stay within the documented budget"
        );
    }
}
