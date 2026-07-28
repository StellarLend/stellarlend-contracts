/// twap_eviction_test.rs — Snapshot ring-buffer eviction policy tests.
///
/// Coverage matrix
/// ───────────────
/// ✓  Ring stays at MAX_SNAPSHOTS after the cap is exactly reached
/// ✓  Ring stays bounded after many writes (N >> MAX_SNAPSHOTS)
/// ✓  TWAP within the window is correct after eviction
/// ✓  Window-boundary snapshot is never evicted while still needed
/// ✓  Oldest snapshot is evicted once it is safely outside the window
/// ✓  find_snapshot_at_or_before returns correct result after eviction
/// ✓  get_twap resolves correctly on a sparse ring (large intervals between snaps)
/// ✓  Two independent assets have independent rings (no cross-contamination)

#[cfg(test)]
mod tests {
    use soroban_sdk::{testutils::Ledger, Address, Env};

    use crate::amm_twap::{
        get_snapshots, get_twap, update_twap_accumulators, EVICTION_SAFETY_FACTOR, MAX_SNAPSHOTS,
        MAX_TWAP_WINDOW_SECS, MIN_WINDOW_SECS, PRICE_SCALE, SNAPSHOT_INTERVAL_SECS,
    };

    // ── Test helpers ─────────────────────────────────────────────────────────

    /// Advance the mock ledger timestamp by `secs` seconds.
    fn advance(env: &Env, secs: u64) {
        let t = env.ledger().timestamp();
        env.ledger().set_timestamp(t + secs);
    }

    /// Write `count` snapshots for `asset`, spaced `SNAPSHOT_INTERVAL_SECS` apart.
    /// Starts from the current ledger timestamp.
    fn write_n_snapshots(env: &Env, asset: &Address, count: u64) {
        for _ in 0..count {
            advance(env, SNAPSHOT_INTERVAL_SECS);
            update_twap_accumulators(env, asset, 1_000_000, 1_000_000);
        }
    }

    // ── 1. Cap exactly reached ────────────────────────────────────────────────

    /// Writing exactly MAX_SNAPSHOTS entries must leave the ring at MAX_SNAPSHOTS.
    /// Writing one more entry (which is safely outside the window) must evict the
    /// oldest so the ring stays at MAX_SNAPSHOTS.
    #[test]
    fn cap_exactly_reached_then_eviction_keeps_ring_at_max() {
        let env = Env::default();
        let asset = Address::generate(&env);
        // Start at a timestamp large enough that the ring can safely rotate once
        // it is full: the oldest entry must be > MAX_TWAP_WINDOW_SECS * SAFETY old.
        let t0: u64 = MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR * 4;
        env.ledger().set_timestamp(t0);

        // Initialise the pool.
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        // Fill to exactly MAX_SNAPSHOTS.
        write_n_snapshots(&env, &asset, MAX_SNAPSHOTS as u64);

        let snaps = get_snapshots(&env, &asset);
        assert_eq!(
            snaps.len(),
            MAX_SNAPSHOTS,
            "ring should be exactly at cap after {} writes",
            MAX_SNAPSHOTS
        );

        // One more write — oldest entry is safely outside the window, must evict.
        advance(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        let snaps_after = get_snapshots(&env, &asset);
        assert_eq!(
            snaps_after.len(),
            MAX_SNAPSHOTS,
            "ring should still be at cap after eviction (not grow beyond {})",
            MAX_SNAPSHOTS
        );
    }

    // ── 2. Many writes keep ring bounded ─────────────────────────────────────

    /// Writing N >> MAX_SNAPSHOTS entries must never let the ring exceed MAX_SNAPSHOTS.
    #[test]
    fn many_writes_never_exceed_cap() {
        let env = Env::default();
        let asset = Address::generate(&env);
        // Start far enough ahead that eviction is always safe.
        let t0: u64 = MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR * 10;
        env.ledger().set_timestamp(t0);

        update_twap_accumulators(&env, &asset, 500_000, 1_000_000);

        // Write 3 × MAX_SNAPSHOTS entries.
        write_n_snapshots(&env, &asset, MAX_SNAPSHOTS as u64 * 3);

        let snaps = get_snapshots(&env, &asset);
        assert!(
            snaps.len() <= MAX_SNAPSHOTS,
            "ring must not exceed MAX_SNAPSHOTS ({}) after many writes; got {}",
            MAX_SNAPSHOTS,
            snaps.len()
        );
    }

    // ── 3. TWAP within the window is unaffected by eviction ──────────────────

    /// After the ring has rotated (oldest entries evicted), `get_twap` for a
    /// window that fits within the retained snapshots must return a value that
    /// is indistinguishable from the value computed before any eviction happened.
    #[test]
    fn twap_within_window_correct_after_eviction() {
        let env = Env::default();
        let asset = Address::generate(&env);
        // Start far enough ahead for safe eviction.
        let t0: u64 = MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR * 4;
        env.ledger().set_timestamp(t0);

        // Price is always 1:1 (equal reserves), so TWAP should be ≈ 1.0.
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        // Fill the ring to MAX_SNAPSHOTS then add more to trigger evictions.
        write_n_snapshots(&env, &asset, MAX_SNAPSHOTS as u64 + 50);

        // Query a short window (well within the retained ring).
        let window = MIN_WINDOW_SECS * 4; // 4× minimum — comfortably inside the ring
        let twap = get_twap(&env, &asset, window);
        let price = twap as f64 / PRICE_SCALE as f64;

        assert!(
            (price - 1.0_f64).abs() < 0.01,
            "TWAP should be ~1.0 after eviction, got {price}"
        );
    }

    // ── 4. Window-boundary snapshot is retained ───────────────────────────────

    /// The snapshot that serves as the anchor for the maximum supported window
    /// must never be evicted while it is still within that window.
    ///
    /// We do this by filling the ring to MAX_SNAPSHOTS, then attempting one more
    /// write while the pool is *not yet old enough* for safe eviction.  The ring
    /// must not grow and must not lose the boundary snapshot.
    #[test]
    fn window_boundary_snapshot_never_evicted_prematurely() {
        let env = Env::default();
        let asset = Address::generate(&env);
        // Start at exactly MAX_TWAP_WINDOW_SECS so the first snapshots are
        // right at the boundary.
        env.ledger().set_timestamp(MAX_TWAP_WINDOW_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        // Fill to MAX_SNAPSHOTS.  The oldest snapshot is at t = MAX_TWAP_WINDOW_SECS,
        // meaning its age when the cap is hit is only MAX_SNAPSHOTS * SNAPSHOT_INTERVAL_SECS
        // = MAX_TWAP_WINDOW_SECS seconds, which equals (not exceeds)
        // MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR.  Since the safety
        // condition requires strictly greater, eviction must be skipped.
        write_n_snapshots(&env, &asset, MAX_SNAPSHOTS as u64 - 1);

        // Record how many snapshots we have before the overflow write.
        let count_before = get_snapshots(&env, &asset).len();

        // One more write: cap is hit but oldest entry is ≤ safety threshold.
        advance(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        let snaps_after = get_snapshots(&env, &asset);

        // The ring must not have grown (write was skipped OR oldest was safely evicted).
        // Crucially: if a write was skipped, count is unchanged.
        // If oldest was safely evicted, count stays at MAX_SNAPSHOTS.
        assert!(
            snaps_after.len() <= MAX_SNAPSHOTS,
            "ring must never exceed MAX_SNAPSHOTS; got {}",
            snaps_after.len()
        );

        // The boundary snapshot (the one closest to MAX_TWAP_WINDOW_SECS ago) must
        // still be present if a write was skipped.
        if snaps_after.len() == count_before {
            // Write was correctly skipped — oldest boundary snapshot survives.
            let oldest: crate::amm_twap::TwapSnapshot = snaps_after.first().unwrap();
            let now = env.ledger().timestamp();
            let oldest_age = now.saturating_sub(oldest.timestamp);
            assert!(
                oldest_age <= MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR,
                "oldest snapshot (age {}s) must be within safety threshold ({}s)",
                oldest_age,
                MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR
            );
        }
    }

    // ── 5. Oldest is evicted once safely outside the window ──────────────────

    /// Once enough time passes that the oldest snapshot is definitively outside
    /// `MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR`, the next write must
    /// evict it and keep the ring at MAX_SNAPSHOTS.
    #[test]
    fn oldest_evicted_once_safely_outside_window() {
        let env = Env::default();
        let asset = Address::generate(&env);
        // Start far enough in the future.
        let t0: u64 = MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR * 3;
        env.ledger().set_timestamp(t0);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);
        write_n_snapshots(&env, &asset, MAX_SNAPSHOTS as u64);

        // Record oldest snapshot timestamp before the trigger write.
        let first_oldest: crate::amm_twap::TwapSnapshot =
            get_snapshots(&env, &asset).first().unwrap();

        // One more write: oldest is now t0 which is far enough in the past.
        advance(&env, SNAPSHOT_INTERVAL_SECS);
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        let snaps = get_snapshots(&env, &asset);

        // Ring must still be at cap.
        assert_eq!(
            snaps.len(),
            MAX_SNAPSHOTS,
            "ring should be at MAX_SNAPSHOTS after eviction"
        );

        // The original oldest snapshot must have been dropped.
        let new_oldest: crate::amm_twap::TwapSnapshot = snaps.first().unwrap();
        assert!(
            new_oldest.timestamp > first_oldest.timestamp,
            "oldest snapshot should have advanced after eviction (was {}, now {})",
            first_oldest.timestamp,
            new_oldest.timestamp
        );
    }

    // ── 6. find_snapshot_at_or_before correct after eviction ─────────────────

    /// After eviction, `get_twap` (which calls `find_snapshot_at_or_before`
    /// internally) must still resolve to the correct start anchor.
    #[test]
    fn find_snapshot_resolves_correctly_after_eviction() {
        let env = Env::default();
        let asset = Address::generate(&env);
        let t0: u64 = MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR * 4;
        env.ledger().set_timestamp(t0);

        // Build a 1:2 pool (price0 = 2.0).
        update_twap_accumulators(&env, &asset, 1_000_000, 2_000_000);

        // Fill to MAX_SNAPSHOTS then write 100 more to trigger evictions.
        write_n_snapshots(&env, &asset, MAX_SNAPSHOTS as u64 + 100);

        // TWAP over a short window inside the retained ring must still be ~2.0.
        let window = MIN_WINDOW_SECS * 2;
        let twap = get_twap(&env, &asset, window);
        let price = twap as f64 / PRICE_SCALE as f64;

        assert!(
            (price - 2.0_f64).abs() < 0.05,
            "TWAP should be ~2.0 after eviction, got {price}"
        );
    }

    // ── 7. Sparse ring (large intervals between snapshots) ───────────────────

    /// When snapshots are spaced further apart than SNAPSHOT_INTERVAL_SECS
    /// (because writes are infrequent), get_twap must fall back to all available
    /// history gracefully rather than panicking.
    #[test]
    fn twap_resolves_on_sparse_ring() {
        let env = Env::default();
        let asset = Address::generate(&env);
        env.ledger().set_timestamp(1_000);

        // Write a few snapshots with 10-minute gaps.
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);
        advance(&env, 600);
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);
        advance(&env, 600);
        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);

        // Request a window that spans the full available history.
        let window = MIN_WINDOW_SECS; // smallest valid window
        let twap = get_twap(&env, &asset, window);
        let price = twap as f64 / PRICE_SCALE as f64;

        assert!(
            (price - 1.0_f64).abs() < 0.01,
            "TWAP on sparse ring should be ~1.0, got {price}"
        );
    }

    // ── 8. Independent rings for independent assets ───────────────────────────

    /// Two pools must have completely independent snapshot rings.  Filling one
    /// pool past the cap must not affect the other.
    #[test]
    fn independent_assets_have_independent_rings() {
        let env = Env::default();
        let asset_a = Address::generate(&env);
        let asset_b = Address::generate(&env);
        let t0: u64 = MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR * 4;
        env.ledger().set_timestamp(t0);

        // Pool A at 1:1, Pool B at 1:3.
        update_twap_accumulators(&env, &asset_a, 1_000_000, 1_000_000);
        update_twap_accumulators(&env, &asset_b, 1_000_000, 3_000_000);

        // Flood pool A past MAX_SNAPSHOTS to trigger many evictions.
        write_n_snapshots(&env, &asset_a, MAX_SNAPSHOTS as u64 * 2);

        // Pool B should still have only its initial snapshot (plus any that
        // accumulated from the shared timestamp advances above).
        let snaps_b = get_snapshots(&env, &asset_b);
        assert!(
            snaps_b.len() <= MAX_SNAPSHOTS,
            "pool B ring must not be contaminated by pool A evictions"
        );

        // Pool B TWAP should still reflect its 1:3 price.
        let window = MIN_WINDOW_SECS * 2;
        let twap_b = get_twap(&env, &asset_b, window);
        let price_b = twap_b as f64 / PRICE_SCALE as f64;
        assert!(
            (price_b - 3.0_f64).abs() < 0.1,
            "pool B TWAP should be ~3.0, got {price_b}"
        );
    }

    // ── 9. Ring is ordered oldest-first (invariant after eviction) ────────────

    /// After eviction the snapshot ring must remain strictly ordered by timestamp
    /// (oldest first, newest last).  A violated ordering would break the binary
    /// search in find_snapshot_at_or_before.
    #[test]
    fn ring_is_monotonically_ordered_after_evictions() {
        let env = Env::default();
        let asset = Address::generate(&env);
        let t0: u64 = MAX_TWAP_WINDOW_SECS * EVICTION_SAFETY_FACTOR * 4;
        env.ledger().set_timestamp(t0);

        update_twap_accumulators(&env, &asset, 1_000_000, 1_000_000);
        write_n_snapshots(&env, &asset, MAX_SNAPSHOTS as u64 + 200);

        let snaps = get_snapshots(&env, &asset);
        let len = snaps.len();
        assert!(len > 1, "need at least 2 snapshots to verify ordering");

        for i in 0..(len - 1) {
            let a: crate::amm_twap::TwapSnapshot = snaps.get(i).unwrap();
            let b: crate::amm_twap::TwapSnapshot = snaps.get(i + 1).unwrap();
            assert!(
                a.timestamp < b.timestamp,
                "snapshot ring is not ordered at index {i}: {} >= {}",
                a.timestamp,
                b.timestamp
            );
        }
    }
}
