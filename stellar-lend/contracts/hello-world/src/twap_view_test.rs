/// twap_view_test.rs — Tests for `oracle::get_pool_twap_price`, the
/// read-only view exposing the AMM TWAP fallback (issue #1128).
///
/// Coverage targets
/// ───────────────
/// ✓ No pool state at all for the asset → None
/// ✓ Pool exists but no elapsed time / no snapshot yet → None
/// ✓ window_secs below amm_twap::MIN_WINDOW_SECS → None
/// ✓ Window exactly at the boundary covered by the earliest available data → Some
/// ✓ Multiple snapshots, window selecting a specific one → Some, matches get_twap exactly
/// ✓ Never panics for any combination of the above (the whole point of this view)
/// ✓ Pure read: does not mutate has_window_coverage's own inputs across repeated calls

#[cfg(test)]
mod twap_view_tests {
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{testutils::Ledger, Address, Env};

    use crate::amm_twap::{
        self, update_twap_accumulators, MIN_WINDOW_SECS, SNAPSHOT_INTERVAL_SECS,
    };
    use crate::oracle::get_pool_twap_price;

    fn advance_time(env: &Env, secs: u64) {
        let current = env.ledger().timestamp();
        env.ledger().set_timestamp(current + secs);
    }

    fn mock_asset(env: &Env) -> Address {
        Address::generate(env)
    }

    // -----------------------------------------------------------------------
    // No data at all
    // -----------------------------------------------------------------------

    #[test]
    fn returns_none_when_asset_has_no_pool_state() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        // Never called update_twap_accumulators for this asset at all.
        let result = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS);
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // Pool exists, but no snapshot covers the window yet
    // -----------------------------------------------------------------------

    #[test]
    fn returns_none_immediately_after_first_update_with_no_elapsed_time() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        // First write: establishes pool state. Note that
        // `maybe_write_snapshot` does record a snapshot here too (its
        // "last snapshot" baseline is 0, so the first ever write always
        // clears SNAPSHOT_INTERVAL_SECS) -- but that snapshot's timestamp
        // is `now` itself, so it does not predate `now - window_secs` for
        // any positive window, and therefore cannot cover one yet.
        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);

        let result = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS);
        assert_eq!(
            result, None,
            "the only snapshot so far is dated 'now', so no positive-length window is covered"
        );
    }

    #[test]
    fn returns_none_when_history_is_shorter_than_the_requested_window() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);

        // Only a few seconds of history -- far short of MIN_WINDOW_SECS.
        advance_time(&env, 3);
        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);

        let result = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS);
        assert_eq!(
            result, None,
            "3s of history cannot cover a MIN_WINDOW_SECS window"
        );
    }

    // -----------------------------------------------------------------------
    // window_secs below the protocol minimum
    // -----------------------------------------------------------------------

    #[test]
    fn returns_none_when_window_secs_is_below_min_window_secs() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);
        advance_time(&env, MIN_WINDOW_SECS * 3);
        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);

        // Plenty of history exists, but the requested window itself is too small.
        let result = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS - 1);
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // Exact-window boundary: just enough history, via the "no snapshot
    // before target_start, use earliest available" path
    // -----------------------------------------------------------------------

    #[test]
    fn returns_some_when_elapsed_history_exactly_meets_min_window_secs() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);
        // Advance exactly MIN_WINDOW_SECS and write again, so the pool's
        // very first state is exactly MIN_WINDOW_SECS old -- the boundary
        // case for the "no snapshot yet, use earliest history" path.
        advance_time(&env, MIN_WINDOW_SECS);
        update_twap_accumulators(&env, &asset, 1_500_000, 300_000);

        let result = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS);
        assert!(
            result.is_some(),
            "exactly MIN_WINDOW_SECS of elapsed history must be sufficient"
        );

        // Must be identical to calling get_twap directly -- this view calls
        // the same path, not a reimplementation.
        let direct = amm_twap::get_twap(&env, &asset, MIN_WINDOW_SECS);
        assert_eq!(result.unwrap(), direct);
    }

    // -----------------------------------------------------------------------
    // Multiple snapshots: window selects a specific historical snapshot
    // -----------------------------------------------------------------------

    #[test]
    fn returns_some_matching_get_twap_exactly_with_multiple_snapshots() {
        let env = Env::default();
        env.ledger().set_timestamp(10_000);
        let asset = mock_asset(&env);

        // Seed enough snapshot history for a real windowed query: several
        // updates spaced past SNAPSHOT_INTERVAL_SECS apart so each one gets
        // persisted as its own snapshot.
        update_twap_accumulators(&env, &asset, 1_000_000, 200_000); // price 0.2
        advance_time(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 250_000); // price 0.25
        advance_time(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 300_000); // price 0.3
        advance_time(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 350_000); // price 0.35
        advance_time(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 400_000); // price 0.4

        let window = 2 * SNAPSHOT_INTERVAL_SECS;
        let result = get_pool_twap_price(&env, &asset, window);
        assert!(
            result.is_some(),
            "ample multi-snapshot history must cover this window"
        );

        let direct = amm_twap::get_twap(&env, &asset, window);
        assert_eq!(
            result.unwrap(),
            direct,
            "view must return exactly what get_twap computes for the same window"
        );
    }

    // -----------------------------------------------------------------------
    // Never panics, across a sweep of windows including ones that don't
    // have coverage. This is the central guarantee this view exists for.
    // -----------------------------------------------------------------------

    #[test]
    fn never_panics_across_a_sweep_of_windows_with_and_without_coverage() {
        let env = Env::default();
        env.ledger().set_timestamp(50_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);
        advance_time(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 220_000);
        advance_time(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 240_000);

        // A mix of windows: too small, too large for available history, and
        // comfortably covered. None of these should panic regardless of
        // whether the result is Some or None.
        for window in [
            1,
            MIN_WINDOW_SECS - 1,
            MIN_WINDOW_SECS,
            SNAPSHOT_INTERVAL_SECS,
            10 * SNAPSHOT_INTERVAL_SECS,
            10_000,
        ] {
            let _ = get_pool_twap_price(&env, &asset, window);
        }
    }

    // -----------------------------------------------------------------------
    // Purity: repeated calls with identical inputs return identical output
    // and don't change subsequent results (no hidden state mutation).
    // -----------------------------------------------------------------------

    #[test]
    fn repeated_calls_are_idempotent() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        let asset = mock_asset(&env);

        update_twap_accumulators(&env, &asset, 1_000_000, 200_000);
        advance_time(&env, MIN_WINDOW_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 250_000);

        let first = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS);
        let second = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS);
        let third = get_pool_twap_price(&env, &asset, MIN_WINDOW_SECS);

        assert_eq!(first, second);
        assert_eq!(second, third);
    }
}
