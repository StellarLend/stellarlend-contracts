//! AMM TWAP accumulator — time-weighted average price derived from on-chain
//! AMM pool reserves.
//!
//! ## Overview
//!
//! Each time pool reserves change (swap, add/remove liquidity) the caller
//! invokes [`update_twap_accumulators`].  The function:
//!
//! 1. Reads (or initialises) the [`PoolState`] for the asset.
//! 2. Accumulates `price * elapsed_seconds` into `price0_cumulative` /
//!    `price1_cumulative`.
//! 3. Optionally writes a [`TwapSnapshot`] to the rolling ring buffer when
//!    `SNAPSHOT_INTERVAL_SECS` have elapsed since the last snapshot.
//!
//! [`get_twap`] returns the time-weighted average price for the last
//! `window_secs` seconds by linearly interpolating between the two bounding
//! snapshots.
//!
//! The ring buffer is capped at [`MAX_SNAPSHOTS`] entries.  When the ring is
//! full and the oldest snapshot is safely outside the maximum TWAP window
//! (guarded by [`EVICTION_SAFETY_FACTOR`]) it is evicted.

use soroban_sdk::{contracttype, Address, Env, Vec};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Fixed-point scale for TWAP prices (10^18 == 1.0).
pub const PRICE_SCALE: u128 = 1_000_000_000_000_000_000u128;

/// Maximum number of snapshots retained per asset.
pub const MAX_SNAPSHOTS: u32 = 120;

/// Minimum TWAP window in seconds (must have at least this much history).
pub const MIN_WINDOW_SECS: u64 = 10;

/// Maximum supported TWAP window in seconds (≈10 minutes).
pub const MAX_TWAP_WINDOW_SECS: u64 = 600;

/// How frequently a snapshot is written (every 5 seconds by default).
pub const SNAPSHOT_INTERVAL_SECS: u64 = 5;

/// Safety multiplier: the oldest snapshot must be at least this many times
/// `MAX_TWAP_WINDOW_SECS` old before it is eligible for eviction.
pub const EVICTION_SAFETY_FACTOR: u64 = 2;

/// Worst-case comparison budget for the binary-search inside `get_twap`.
/// `ceil(log2(MAX_SNAPSHOTS)) + 1`
pub const TWAP_READ_SEARCH_COMPARISON_BUDGET: u32 = 8; // log2(120) ≈ 7, +1

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TwapDataKey {
    /// Cumulative price accumulators and last-update metadata.
    PoolState(Address),
    /// Rolling ring buffer of snapshots.
    Snapshots(Address),
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Cumulative price accumulators and last-timestamp, per asset pair.
///
/// The "price" stored is `reserve1 / reserve0` (base→quote).  Both the
/// forward and inverse cumulatives are tracked so callers can derive either
/// direction.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolState {
    /// Last recorded reserve0 (base token).
    pub last_reserve0: u128,
    /// Last recorded reserve1 (quote token).
    pub last_reserve1: u128,
    /// Ledger timestamp of the last update.
    pub last_timestamp: u64,
    /// Cumulative sum of (reserve1/reserve0 * PRICE_SCALE * elapsed_secs).
    pub price0_cumulative: u128,
    /// Cumulative sum of (reserve0/reserve1 * PRICE_SCALE * elapsed_secs).
    pub price1_cumulative: u128,
}

/// A point-in-time snapshot of the cumulative accumulators.
///
/// TWAP is derived by differencing two snapshots:
/// `twap = (end.cumulative − start.cumulative) / (end.timestamp − start.timestamp)`
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwapSnapshot {
    /// Ledger timestamp when this snapshot was taken.
    pub timestamp: u64,
    /// `price0_cumulative` at snapshot time.
    pub price0_cumulative: u128,
    /// `price1_cumulative` at snapshot time.
    pub price1_cumulative: u128,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Update TWAP accumulators after a pool reserve change.
///
/// This should be called by every AMM operation that modifies `reserve0` or
/// `reserve1`.  On the very first call the accumulators are initialised; no
/// time has elapsed so the cumulatives remain at zero.
///
/// # Arguments
///
/// * `asset`    — The base asset address used as the storage key.
/// * `reserve0` — New base-token reserve.
/// * `reserve1` — New quote-token reserve.
pub fn update_twap_accumulators(env: &Env, asset: &Address, reserve0: u128, reserve1: u128) {
    let now = env.ledger().timestamp();

    let state = env
        .storage()
        .persistent()
        .get::<TwapDataKey, PoolState>(&TwapDataKey::PoolState(asset.clone()));

    let new_state = match state {
        None => {
            // First write: initialise with zero cumulatives.
            PoolState {
                last_reserve0: reserve0,
                last_reserve1: reserve1,
                last_timestamp: now,
                price0_cumulative: 0,
                price1_cumulative: 0,
            }
        }
        Some(mut s) => {
            let elapsed = now.saturating_sub(s.last_timestamp);
            if elapsed > 0 && s.last_reserve0 > 0 && s.last_reserve1 > 0 {
                // price0 = reserve1 / reserve0  (scaled)
                let price0 = (s.last_reserve1 as u128)
                    .saturating_mul(PRICE_SCALE)
                    / (s.last_reserve0 as u128);
                let price1 = (s.last_reserve0 as u128)
                    .saturating_mul(PRICE_SCALE)
                    / (s.last_reserve1 as u128);

                s.price0_cumulative = s
                    .price0_cumulative
                    .saturating_add(price0.saturating_mul(elapsed as u128));
                s.price1_cumulative = s
                    .price1_cumulative
                    .saturating_add(price1.saturating_mul(elapsed as u128));
            }
            s.last_reserve0 = reserve0;
            s.last_reserve1 = reserve1;
            s.last_timestamp = now;
            s
        }
    };

    env.storage()
        .persistent()
        .set(&TwapDataKey::PoolState(asset.clone()), &new_state);

    // Conditionally write a snapshot to the ring buffer.
    maybe_write_snapshot(env, asset, &new_state, now);
}

/// Compute the time-weighted average price for `asset` over the last
/// `window_secs` seconds.
///
/// Returns `None` when:
/// * the pool has no state (never updated),
/// * `window_secs < MIN_WINDOW_SECS`,
/// * the available snapshot history does not cover the requested window.
///
/// The returned value is in `PRICE_SCALE` units (i.e., multiply by a raw
/// price and divide by `PRICE_SCALE` to convert).
pub fn get_twap(env: &Env, asset: &Address, window_secs: u64) -> Option<u128> {
    if window_secs < MIN_WINDOW_SECS {
        return None;
    }

    let snapshots = load_snapshots(env, asset);
    if snapshots.is_empty() {
        return None;
    }

    let now = env.ledger().timestamp();
    let target_start = now.saturating_sub(window_secs);

    // We need the latest snapshot as the "end" reference and a snapshot at or
    // before `target_start` as the "start" reference.
    let end_snap: TwapSnapshot = snapshots.get(snapshots.len() - 1)?;

    if end_snap.timestamp <= target_start {
        return None;
    }

    // Binary search for the latest snapshot at or before `target_start`.
    let start_snap = find_snapshot_at_or_before(&snapshots, target_start)?;

    let elapsed = end_snap.timestamp.saturating_sub(start_snap.timestamp);
    if elapsed == 0 {
        return None;
    }

    let cumulative_diff = end_snap
        .price0_cumulative
        .wrapping_sub(start_snap.price0_cumulative);

    Some(cumulative_diff / elapsed as u128)
}

/// Return the full snapshot ring buffer for `asset`.
pub fn get_snapshots(env: &Env, asset: &Address) -> Vec<TwapSnapshot> {
    load_snapshots(env, asset)
}

/// Return the current [`PoolState`] for `asset`, or `None` if uninitialised.
pub fn get_pool_state(env: &Env, asset: &Address) -> Option<PoolState> {
    env.storage()
        .persistent()
        .get::<TwapDataKey, PoolState>(&TwapDataKey::PoolState(asset.clone()))
}

// ---------------------------------------------------------------------------
// Test-only helpers
// ---------------------------------------------------------------------------

/// Returns the number of binary-search comparisons that `get_twap` would
/// perform against the current snapshot ring.  Used by perf tests to pin
/// the search budget.
#[cfg(test)]
pub fn snapshot_search_metrics_for_test(env: &Env, asset: &Address, target: u64) -> u32 {
    let snapshots = load_snapshots(env, asset);
    let len = snapshots.len();
    if len == 0 {
        return 0;
    }
    // Count comparisons of a standard binary search.
    let mut lo: u32 = 0;
    let mut hi: u32 = len.saturating_sub(1);
    let mut comparisons: u32 = 0;
    while lo < hi {
        comparisons += 1;
        let mid = lo + (hi - lo) / 2;
        let snap: TwapSnapshot = snapshots.get(mid).unwrap();
        if snap.timestamp <= target {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    comparisons
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_snapshots(env: &Env, asset: &Address) -> Vec<TwapSnapshot> {
    env.storage()
        .persistent()
        .get::<TwapDataKey, Vec<TwapSnapshot>>(&TwapDataKey::Snapshots(asset.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

fn save_snapshots(env: &Env, asset: &Address, snapshots: &Vec<TwapSnapshot>) {
    env.storage()
        .persistent()
        .set(&TwapDataKey::Snapshots(asset.clone()), snapshots);
}

/// Write a snapshot at most every `SNAPSHOT_INTERVAL_SECS`.  Evicts the
/// oldest entry when the ring is full and the oldest entry is safely outside
/// the maximum TWAP window.
fn maybe_write_snapshot(env: &Env, asset: &Address, state: &PoolState, now: u64) {
    let mut snapshots = load_snapshots(env, asset);

    // Determine the timestamp of the last snapshot (or 0 if none).
    let last_snap_ts: u64 = if snapshots.is_empty() {
        0
    } else {
        let last: TwapSnapshot = snapshots.get(snapshots.len() - 1).unwrap();
        last.timestamp
    };

    // Write a snapshot if enough time has passed since the last one.
    if now < last_snap_ts.saturating_add(SNAPSHOT_INTERVAL_SECS) {
        return;
    }

    let snap = TwapSnapshot {
        timestamp: now,
        price0_cumulative: state.price0_cumulative,
        price1_cumulative: state.price1_cumulative,
    };

    // If the ring is full, try to evict the oldest entry.
    if snapshots.len() >= MAX_SNAPSHOTS {
        let oldest: TwapSnapshot = snapshots.get(0).unwrap();
        let min_age = MAX_TWAP_WINDOW_SECS.saturating_mul(EVICTION_SAFETY_FACTOR);
        if now.saturating_sub(oldest.timestamp) >= min_age {
            // Shift left: drop index 0 by rebuilding without it.
            let mut new_snaps: Vec<TwapSnapshot> = Vec::new(env);
            for i in 1..snapshots.len() {
                new_snaps.push_back(snapshots.get(i).unwrap());
            }
            snapshots = new_snaps;
        } else {
            // Cannot evict safely; skip this snapshot write.
            return;
        }
    }

    snapshots.push_back(snap);
    save_snapshots(env, asset, &snapshots);
}

/// Binary-search for the latest snapshot with `timestamp <= target`.
///
/// Returns `None` when no such snapshot exists (all snapshots are after
/// `target`).
fn find_snapshot_at_or_before(
    snapshots: &Vec<TwapSnapshot>,
    target: u64,
) -> Option<TwapSnapshot> {
    let len = snapshots.len();
    if len == 0 {
        return None;
    }

    let first: TwapSnapshot = snapshots.get(0)?;
    if first.timestamp > target {
        return None;
    }

    let mut lo: u32 = 0;
    let mut hi: u32 = len - 1;

    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        let snap: TwapSnapshot = snapshots.get(mid)?;
        if snap.timestamp <= target {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    snapshots.get(lo)
}
