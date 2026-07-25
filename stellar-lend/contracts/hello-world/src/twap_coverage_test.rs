/// twap_coverage_test.rs — Tests for `has_window_coverage` and
/// `find_snapshot_at_or_before` in `amm_twap.rs`.
///
/// # Coverage targets
/// ───────────────────
/// ✓ `has_window_coverage`: false when no pool state exists (empty buffer)
/// ✓ `has_window_coverage`: false when `window_secs` < `MIN_WINDOW_SECS`
/// ✓ `has_window_coverage`: false when oldest snapshot is newer than window start
/// ✓ `has_window_coverage`: true when oldest snapshot is older than window start
/// ✓ `has_window_coverage`: snapshot exactly at window start boundary → true
/// ✓ `has_window_coverage`: snapshot one second after window start → false
/// ✓ `has_window_coverage`: pool state but no snapshots yet, enough elapsed time → true
/// ✓ `find_snapshot_at_or_before`: empty buffer returns None without panic
/// ✓ `find_snapshot_at_or_before`: exact timestamp hit returns matching snapshot
/// ✓ `find_snapshot_at_or_before`: target between snapshots returns nearest lower bound
/// ✓ `find_snapshot_at_or_before`: target before first snapshot returns None
/// ✓ `find_snapshot_at_or_before`: target at first snapshot returns first snapshot
/// ✓ `find_snapshot_at_or_before`: single-element buffer, exact hit
/// ✓ `find_snapshot_at_or_before`: single-element buffer, target above → Some
/// ✓ `find_snapshot_at_or_before`: single-element buffer, target below → None
/// ✓ `find_snapshot_at_or_before`: target beyond last snapshot returns last

#[cfg(test)]
mod twap_coverage_tests {
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{testutils::Ledger, Address, Env};

    use crate::amm_twap::{
        has_window_coverage, snapshot_search_metrics_for_test, update_twap_accumulators,
        TwapSnapshot, MIN_WINDOW_SECS, SNAPSHOT_INTERVAL_SECS,
    };

    // ── Test helpers ─────────────────────────────────────────────────────────

    /// Advance the mock ledger by `secs` seconds.
    fn advance(env: &Env, secs: u64) {
        let t = env.ledger().timestamp();
        env.ledger().set_timestamp(t + secs);
    }

    /// Return a freshly generated mock asset address.
    fn mock_asset(env: &Env) -> Address {
        Address::generate(env)
    }

    /// Build a `soroban_sdk::Vec<TwapSnapshot>` from a slice of timestamps.
    ///
    /// Cumulative values are set to zero; only timestamps matter for the
    /// binary-search correctness tests in this file.
    fn make_snaps(env: &Env, timestamps: &[u64]) -> soroban_sdk::Vec<TwapSnapshot> {
        let mut v: soroban_sdk::Vec<TwapSnapshot> = soroban_sdk::Vec::new(env);
        for &ts in timestamps {
            v.push_back(TwapSnapshot {
                timestamp: ts,
                price0_cumulative: 0,
                price1_cumulative: 0,
            });
        }
        v
    }

    // ── has_window_coverage ──────────────────────────────────────────────────

    /// No pool state has ever been written for the asset.
    ///
    /// `has_window_coverage` must return `false` immediately (the early-return
    /// on missing `TwapPoolState`) without panicking.  This is the "completely
    /// empty" case — no storage entry at all.
    #[test]
    fn coverage_false_when_no_pool_state() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        // Intentionally skip update_twap_accumulators — nothing written.
        assert!(
            !has_window_coverage(&env, &asset, MIN_WINDOW_SECS),
            "no pool state must return false without panicking"
        );
    }

    /// `window_secs` is below the protocol-enforced minimum (`MIN_WINDOW_SECS`).
    ///
    /// The function's first guard rejects such requests unconditionally, even
    /// when ample snapshot history exists.
    #[test]
    fn coverage_false_when_window_below_minimum() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000_000);
        let asset = mock_asset(&env);

        // Build plenty of history.
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);
        advance(&env, MIN_WINDOW_SECS * 10);
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        assert!(
            !has_window_coverage(&env, &asset, MIN_WINDOW_SECS - 1),
            "window_secs below MIN_WINDOW_SECS must return false even with ample history"
        );
    }

    /// The oldest (and only) snapshot sits **newer** than the requested window
    /// start, so `find_snapshot_at_or_before` returns `None`.  The elapsed
    /// history since the earliest recorded timestamp is also shorter than
    /// `MIN_WINDOW_SECS`, so the function returns `false`.
    ///
    /// Setup:
    ///   - Snapshot written at T = 1_000_000.
    ///   - Ledger advances 20 s → T = 1_000_020.
    ///   - `window_secs` = 25 → `target_start` = 999 995.
    ///   - Snapshot timestamp 1_000_000 > 999_995 → no anchor → `None` path.
    ///   - now − 1_000_000 = 20 < 25 (`MIN_WINDOW_SECS`) → `false`.
    #[test]
    fn coverage_false_when_oldest_snapshot_newer_than_window_start() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        // Advance only 20 s — less than MIN_WINDOW_SECS (25).
        advance(&env, 20);

        assert!(
            !has_window_coverage(&env, &asset, MIN_WINDOW_SECS),
            "snapshot at T=1_000_000 is newer than window start T=999_995; must return false"
        );
    }

    /// The oldest snapshot is **older** than the requested window start, so
    /// `find_snapshot_at_or_before` returns a valid `Some` anchor.  The elapsed
    /// time since that anchor is positive, so the function returns `true`.
    ///
    /// Setup:
    ///   - Snapshot written at T = 1_000.
    ///   - Ledger set to T = 10_000 (no further writes).
    ///   - `window_secs` = 25 → `target_start` = 9_975.
    ///   - Snapshot T=1_000 ≤ 9_975 → `Some` anchor.
    ///   - now − 1_000 = 9_000 > 0 → `true`.
    #[test]
    fn coverage_true_when_oldest_snapshot_older_than_window_start() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        // Jump forward; no new snapshot is needed.
        env.ledger().set_timestamp(10_000);

        assert!(
            has_window_coverage(&env, &asset, MIN_WINDOW_SECS),
            "snapshot at T=1_000 predates window start T=9_975; must return true"
        );
    }

    /// The snapshot sits **exactly** at the window start boundary.
    ///
    /// `find_snapshot_at_or_before` uses a `≤` comparison, so a snapshot
    /// at `target_start` qualifies as a valid anchor.  Because `now −
    /// snap.timestamp` = `MIN_WINDOW_SECS` > 0, the function returns `true`.
    ///
    /// Setup:
    ///   - Snapshot at T = 1_000_000.
    ///   - Advance exactly `MIN_WINDOW_SECS` → now = 1_000_025.
    ///   - `target_start` = 1_000_025 − 25 = 1_000_000 → exact match → anchor.
    ///   - now − 1_000_000 = 25 > 0 → `true`.
    #[test]
    fn coverage_true_when_snapshot_exactly_at_window_start() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);
        advance(&env, MIN_WINDOW_SECS);

        assert!(
            has_window_coverage(&env, &asset, MIN_WINDOW_SECS),
            "snapshot exactly at window start qualifies as anchor; must return true"
        );
    }

    /// One second short of boundary: snapshot is one second **after** the
    /// window start, so no anchor exists.  Elapsed history is also too short,
    /// so the function returns `false`.
    ///
    /// Setup:
    ///   - Snapshot at T = 1_000_000.
    ///   - Advance `MIN_WINDOW_SECS − 1` → now = 1_000_024.
    ///   - `target_start` = 1_000_024 − 25 = 999_999.
    ///   - Snapshot T=1_000_000 > 999_999 → `None` path.
    ///   - now − 1_000_000 = 24 < 25 → `false`.
    #[test]
    fn coverage_false_when_snapshot_one_second_after_window_start() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);
        advance(&env, MIN_WINDOW_SECS - 1);

        assert!(
            !has_window_coverage(&env, &asset, MIN_WINDOW_SECS),
            "snapshot one second newer than window start must return false"
        );
    }

    /// Pool state exists but no snapshot has been written yet (first update was
    /// too recent for `maybe_write_snapshot` to trigger).  After enough time
    /// has elapsed since the pool's `last_timestamp`, `has_window_coverage`
    /// falls back to `current_state.last_timestamp` and returns `true`.
    ///
    /// Setup:
    ///   - T = 30 < `SNAPSHOT_INTERVAL_SECS` (60) → update writes pool state
    ///     but NOT a snapshot.
    ///   - T = 100 → now − last_timestamp = 70 ≥ 25 → `true`.
    #[test]
    fn coverage_true_with_pool_state_but_no_snapshots_after_enough_elapsed_time() {
        let env = Env::default();
        // Start before SNAPSHOT_INTERVAL_SECS so no snapshot is written.
        env.ledger().set_timestamp(30);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        // Advance so that now − last_timestamp = 70 ≥ MIN_WINDOW_SECS (25).
        env.ledger().set_timestamp(100);

        assert!(
            has_window_coverage(&env, &asset, MIN_WINDOW_SECS),
            "70 s elapsed since pool's last_timestamp must satisfy MIN_WINDOW_SECS"
        );
    }

    /// Mirrors the above but with insufficient elapsed time since the pool's
    /// `last_timestamp`; `has_window_coverage` must return `false`.
    ///
    /// Setup:
    ///   - T = 30, no snapshot written.
    ///   - T = 54 → now − last_timestamp = 24 < 25 → `false`.
    #[test]
    fn coverage_false_with_pool_state_but_no_snapshots_and_insufficient_elapsed_time() {
        let env = Env::default();
        env.ledger().set_timestamp(30);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        env.ledger().set_timestamp(54);

        assert!(
            !has_window_coverage(&env, &asset, MIN_WINDOW_SECS),
            "24 s elapsed since pool's last_timestamp is less than MIN_WINDOW_SECS; must be false"
        );
    }

    // ── find_snapshot_at_or_before ───────────────────────────────────────────

    /// Empty snapshot buffer → `None` is returned and no comparisons are made.
    ///
    /// Validates the early-return on `len == 0` inside the binary search.
    /// Must not panic.
    #[test]
    fn find_snapshot_returns_none_for_empty_buffer() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[]);

        let (result, comparisons) = snapshot_search_metrics_for_test(&snaps, 1_000);

        assert!(result.is_none(), "empty buffer must return None");
        assert_eq!(comparisons, 0, "no comparisons expected for an empty buffer");
    }

    /// Target timestamp **exactly matches** a snapshot → that snapshot is returned.
    ///
    /// The binary search uses `snap.timestamp ≤ target_ts`, so an exact hit
    /// continues searching rightward and ultimately returns the matched entry.
    /// Validated for both the middle and the last element.
    #[test]
    fn find_snapshot_exact_timestamp_returns_matching_snapshot() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[100, 200, 300]);

        // Exact hit at the middle entry.
        let (result, _) = snapshot_search_metrics_for_test(&snaps, 200);
        let snap = result.expect("exact hit at middle must return Some");
        assert_eq!(snap.timestamp, 200, "exact match must return the snapshot at T=200");

        // Exact hit at the last entry.
        let (result_last, _) = snapshot_search_metrics_for_test(&snaps, 300);
        let snap_last = result_last.expect("exact hit at last must return Some");
        assert_eq!(snap_last.timestamp, 300, "exact match must return the snapshot at T=300");
    }

    /// Target falls **between** two consecutive snapshots → the most recent
    /// snapshot at or before the target is returned (lower bound), not the
    /// next one above it (upper bound).
    ///
    /// Buffer: [T=100, T=200, T=300].
    ///   - target=250 → lower bound is T=200.
    ///   - target=150 → lower bound is T=100.
    #[test]
    fn find_snapshot_between_timestamps_returns_nearest_lower_bound() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[100, 200, 300]);

        let (result, _) = snapshot_search_metrics_for_test(&snaps, 250);
        let snap = result.expect("target T=250 must return Some");
        assert_eq!(
            snap.timestamp, 200,
            "T=250 is between T=200 and T=300; lower bound must be T=200"
        );

        let (result2, _) = snapshot_search_metrics_for_test(&snaps, 150);
        let snap2 = result2.expect("target T=150 must return Some");
        assert_eq!(
            snap2.timestamp, 100,
            "T=150 is between T=100 and T=200; lower bound must be T=100"
        );
    }

    /// Target is strictly **before** the first snapshot → `None`.
    ///
    /// The binary search exhausts all candidates without ever satisfying
    /// `snap.timestamp ≤ target_ts`, so no result is set and `None` is returned.
    /// Must not panic.
    #[test]
    fn find_snapshot_target_before_first_snapshot_returns_none() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[100, 200, 300]);

        let (result, _) = snapshot_search_metrics_for_test(&snaps, 50);

        assert!(
            result.is_none(),
            "target T=50 is before earliest snapshot T=100; must return None"
        );
    }

    /// Target equals the **first** snapshot's timestamp → that snapshot is returned.
    ///
    /// Validates the left-boundary path of the binary search where
    /// `snap.timestamp == target_ts` at index 0.
    #[test]
    fn find_snapshot_target_at_first_snapshot_returns_first() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[100, 200, 300]);

        let (result, _) = snapshot_search_metrics_for_test(&snaps, 100);
        let snap = result.expect("target at first snapshot must return Some");
        assert_eq!(
            snap.timestamp, 100,
            "target T=100 equals first snapshot; must return T=100"
        );
    }

    /// Single-element buffer with an **exact** hit → returns that snapshot.
    #[test]
    fn find_snapshot_single_element_exact_hit() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[500]);

        let (result, _) = snapshot_search_metrics_for_test(&snaps, 500);
        let snap = result.expect("single-element exact hit must return Some");
        assert_eq!(snap.timestamp, 500);
    }

    /// Single-element buffer with target **above** the snapshot → returns that snapshot.
    #[test]
    fn find_snapshot_single_element_target_above() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[500]);

        let (result, _) = snapshot_search_metrics_for_test(&snaps, 999);
        let snap = result.expect("target above single element must return Some");
        assert_eq!(snap.timestamp, 500);
    }

    /// Single-element buffer with target **below** the snapshot → `None`.
    #[test]
    fn find_snapshot_single_element_target_below_returns_none() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[500]);

        let (result, _) = snapshot_search_metrics_for_test(&snaps, 499);
        assert!(
            result.is_none(),
            "target below a single-element buffer must return None"
        );
    }

    /// Target far beyond the last snapshot → returns the **last** snapshot.
    ///
    /// Ensures the binary search never panics on an out-of-range target and
    /// correctly converges on the rightmost qualifying entry.
    #[test]
    fn find_snapshot_target_beyond_last_snapshot_returns_last() {
        let env = Env::default();
        let snaps = make_snaps(&env, &[100, 200, 300]);

        let (result, _) = snapshot_search_metrics_for_test(&snaps, u64::MAX);
        let snap = result.expect("target beyond all snapshots must return Some");
        assert_eq!(
            snap.timestamp, 300,
            "target beyond all entries must return the last snapshot T=300"
        );
    }
}
