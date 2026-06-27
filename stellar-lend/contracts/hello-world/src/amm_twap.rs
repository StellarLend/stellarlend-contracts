/// amm_twap.rs — TWAP (Time-Weighted Average Price) accumulator for the StellarLend AMM.
///
/// # Design
///
/// We maintain a **cumulative price** per asset pair using the standard Uniswap v2 / constant-
/// product model, adapted for Soroban's ledger timestamp (seconds since Unix epoch).
///
/// For a pool holding `reserve_a` of asset A and `reserve_b` of asset B:
///
/// ```text
/// price_a_cumulative += (reserve_b / reserve_a) * Δt
/// price_b_cumulative += (reserve_a / reserve_b) * Δt
/// ```
///
/// where `Δt = current_timestamp − last_timestamp`.
///
/// To query the TWAP over a window `[T-window, T]` a caller snapshots the cumulative value at
/// two points and divides the difference by the elapsed time:
///
/// ```text
/// twap = (price_cumulative_now − price_cumulative_then) / window_seconds
/// ```
///
/// # Manipulation resistance
///
/// * The accumulator only moves forward in time — it cannot be rewound.
/// * Prices are only updated *after* the reserves have changed; a sandwich attack must hold the
///   position for at least one ledger close (≈ 5 s) to shift the TWAP.
/// * Callers should use a window of at least **30 ledgers** (≈ 150 s) for any security-critical
///   use such as liquidation valuation.  A 300-ledger window (≈ 25 min) is recommended for
///   large-value positions.
/// * A single-block flash-loan cannot meaningfully influence a 30+ ledger TWAP.
///
/// # Snapshot eviction policy
///
/// Snapshots are persisted every [`SNAPSHOT_INTERVAL_SECS`] seconds. To bound storage rent and
/// per-read deserialisation cost, the ring buffer is capped at [`MAX_SNAPSHOTS`] entries.
///
/// When `maybe_write_snapshot` would exceed the cap it first checks that evicting the oldest
/// entry will not drop a snapshot that is still within the maximum supported TWAP window
/// ([`MAX_TWAP_WINDOW_SECS`]).  Because the ring is sized so that
/// `MAX_SNAPSHOTS × SNAPSHOT_INTERVAL_SECS ≥ MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR`,
/// eviction only removes data that is older than the longest supported query window, ensuring
/// every valid `get_twap` call can still find a start snapshot.
///
/// See `docs/TWAP_SNAPSHOT_POLICY.md` for the full rationale.
///
/// # Storage keys
///
/// | Key                              | Type              | Meaning                          |
/// |----------------------------------|-------------------|----------------------------------|
/// | `TwapState(asset)`               | `TwapPoolState`   | Cumulative prices + last ts      |
/// | `TwapSnaps(asset)`               | `Vec<TwapSnapshot>` | Ring-buffered checkpoints      |
///
/// Historic snapshots are written every `SNAPSHOT_INTERVAL_SECS` seconds to bound the lookup
/// window granularity.  Lookups binary-search the available snapshots.
use soroban_sdk::{contracttype, symbol_short, Address, Env, Map, Symbol, Vec};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Minimum observation window in seconds (~5 ledger closes).
pub const MIN_WINDOW_SECS: u64 = 25;

/// Recommended minimum for security-critical callers (≈ 30 ledger closes).
pub const RECOMMENDED_WINDOW_SECS: u64 = 150;

/// How often we persist a snapshot (every ~60 s / 12 ledgers).
pub const SNAPSHOT_INTERVAL_SECS: u64 = 60;

/// Maximum supported TWAP query window in seconds.
///
/// This is the longest `window_secs` value that `get_twap` is designed to serve.
/// The snapshot ring is sized so that it always covers at least this much history.
///
/// Current value: 86 400 s = 24 h.  At 60 s per snapshot that requires 1 440 snapshots,
/// which is exactly [`MAX_SNAPSHOTS`].
pub const MAX_TWAP_WINDOW_SECS: u64 = 86_400;

/// Ring-buffer capacity: maximum number of snapshots retained per asset.
///
/// Sizing rationale:
/// ```text
/// MAX_TWAP_WINDOW_SECS / SNAPSHOT_INTERVAL_SECS = 86_400 / 60 = 1_440
/// ```
/// With this cap the ring always holds at least 24 h of checkpoint history,
/// comfortably covering the maximum supported query window.  Storage cost per
/// asset is bounded at `1_440 × ~48 bytes ≈ 69 KiB` of persistent storage,
/// which keeps rent predictable regardless of pool age.
///
/// Eviction only removes the single oldest entry once the cap is hit, keeping
/// the amortised write cost at O(1) per snapshot interval.
pub const MAX_SNAPSHOTS: u32 = 1_440;

/// Safety multiplier used during eviction to guarantee the oldest retained
/// snapshot is never within the maximum query window.
///
/// Value 2 means we keep twice as much history as strictly required, so a
/// pool that hasn't seen any swap in a full window still has a valid start
/// anchor for the longest `get_twap` query.
pub const EVICTION_SAFETY_FACTOR: u64 = 2;

/// Fixed-point scale factor (10^18) used for cumulative prices to preserve precision
/// while avoiding floating-point.
pub const PRICE_SCALE: u128 = 1_000_000_000_000_000_000_u128; // 1e18

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

/// Persistent TWAP state for a single pool identified by the *quote* asset address.
/// (The base asset is always the pool's tracked token; the quote is the paired token.)
#[contracttype]
#[derive(Clone, Debug)]
pub struct TwapPoolState {
    /// Cumulative (reserve_quote / reserve_base) × PRICE_SCALE × elapsed_seconds.
    /// Stored as u128 to avoid overflow for high-volume pools over long periods.
    pub price0_cumulative: u128,
    /// Cumulative (reserve_base / reserve_quote) × PRICE_SCALE × elapsed_seconds.
    pub price1_cumulative: u128,
    /// Unix timestamp (seconds) of the last accumulator update.
    pub last_timestamp: u64,
    /// Reserve of the base token at the last update (used for TWAP computation).
    pub last_reserve0: u128,
    /// Reserve of the quote token at the last update.
    pub last_reserve1: u128,
}

/// A point-in-time snapshot used for window-based TWAP queries.
///
/// Snapshots are written at most once per [`SNAPSHOT_INTERVAL_SECS`] and are
/// stored in a ring buffer capped at [`MAX_SNAPSHOTS`] entries per asset.
/// When the ring is full the oldest snapshot is evicted provided it falls
/// outside the maximum supported query window ([`MAX_TWAP_WINDOW_SECS`]).
#[contracttype]
#[derive(Clone, Debug)]
pub struct TwapSnapshot {
    /// Ledger timestamp (seconds since Unix epoch) when this snapshot was taken.
    pub timestamp: u64,
    /// Value of `price0_cumulative` at `timestamp`.
    pub price0_cumulative: u128,
    /// Value of `price1_cumulative` at `timestamp`.
    pub price1_cumulative: u128,
}

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

fn twap_state_key(asset: &Address) -> (Symbol, Address) {
    (symbol_short!("TwapState"), asset.clone())
}

fn twap_snaps_key(asset: &Address) -> (Symbol, Address) {
    (symbol_short!("TwapSnaps"), asset.clone())
}

// ---------------------------------------------------------------------------
// Accumulator update (called on every swap / liquidity event)
// ---------------------------------------------------------------------------

/// Update the cumulative price accumulators for the pool identified by `asset`.
///
/// This **must** be called:
/// 1. After reserves have been updated following a swap, `add_liquidity`, or
///    `remove_liquidity`.
/// 2. With the **new** reserve values.
///
/// After updating the on-chain `TwapPoolState` this function delegates to
/// [`maybe_write_snapshot`] which persists a checkpoint at most once per
/// [`SNAPSHOT_INTERVAL_SECS`] and enforces the [`MAX_SNAPSHOTS`] ring-buffer cap.
///
/// # Arguments
/// * `env`         – Soroban environment.
/// * `asset`       – Address of the pool's base token (used as the map key).
/// * `reserve0`    – New reserve of the base token (raw, unscaled units).
/// * `reserve1`    – New reserve of the quote token.
///
/// # Panics
/// Panics if either reserve is zero (division by zero).
pub fn update_twap_accumulators(env: &Env, asset: &Address, reserve0: u128, reserve1: u128) {
    assert!(reserve0 > 0 && reserve1 > 0, "reserves must be non-zero");

    let now: u64 = env.ledger().timestamp();
    let key = twap_state_key(asset);

    let mut state: TwapPoolState = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(TwapPoolState {
            price0_cumulative: 0,
            price1_cumulative: 0,
            last_timestamp: now,
            last_reserve0: reserve0,
            last_reserve1: reserve1,
        });

    let elapsed = now.saturating_sub(state.last_timestamp);

    if elapsed > 0 && state.last_reserve0 > 0 && state.last_reserve1 > 0 {
        // price0 = reserve1 / reserve0  (how many quote tokens per base token)
        let price0_contribution =
            (state.last_reserve1 * PRICE_SCALE / state.last_reserve0) * elapsed as u128;
        // price1 = reserve0 / reserve1
        let price1_contribution =
            (state.last_reserve0 * PRICE_SCALE / state.last_reserve1) * elapsed as u128;

        state.price0_cumulative = state.price0_cumulative.wrapping_add(price0_contribution);
        state.price1_cumulative = state.price1_cumulative.wrapping_add(price1_contribution);
    }

    state.last_timestamp = now;
    state.last_reserve0 = reserve0;
    state.last_reserve1 = reserve1;

    env.storage().persistent().set(&key, &state);

    // Persist a snapshot if enough time has passed since the last one.
    maybe_write_snapshot(env, asset, &state);
}

// ---------------------------------------------------------------------------
// Snapshot management
// ---------------------------------------------------------------------------

/// Conditionally write a new snapshot and enforce the [`MAX_SNAPSHOTS`] ring-buffer cap.
///
/// # Write condition
/// A snapshot is written only when `state.last_timestamp − last_snap.timestamp ≥
/// SNAPSHOT_INTERVAL_SECS`.  This throttles writes to at most one per interval,
/// keeping the ring bounded in both size and write frequency.
///
/// # Eviction policy
/// When appending a new entry would exceed [`MAX_SNAPSHOTS`]:
///
/// 1. Inspect the **oldest** snapshot (index 0).
/// 2. Compute the age of that snapshot relative to `state.last_timestamp`.
/// 3. Only evict if `age > MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR`.
///    This ensures the oldest retained snapshot is never inside the maximum
///    supported query window, so every valid `get_twap` call can still resolve
///    a start anchor.
/// 4. If the safety condition is not met (pool is fresh and has not yet
///    accumulated enough history to safely evict), the append is skipped and
///    the ring retains all existing snapshots until the condition is satisfied.
///
/// # Amortised cost
/// Under steady-state operation (one snapshot per interval, eviction rate = write
/// rate), each call that triggers eviction removes exactly one entry — O(1)
/// amortised.  The `remove(0)` on a Soroban `Vec` is O(n) in the worst case, but
/// `n` is bounded by [`MAX_SNAPSHOTS`] so the absolute cost is constant.
fn maybe_write_snapshot(env: &Env, asset: &Address, state: &TwapPoolState) {
    let snaps_key = twap_snaps_key(asset);
    let mut snaps: Vec<TwapSnapshot> = env
        .storage()
        .persistent()
        .get(&snaps_key)
        .unwrap_or_else(|| Vec::new(env));

    // Throttle: only write once per SNAPSHOT_INTERVAL_SECS.
    let last_snap_ts = snaps.last().map(|s: TwapSnapshot| s.timestamp).unwrap_or(0);
    if state.last_timestamp.saturating_sub(last_snap_ts) < SNAPSHOT_INTERVAL_SECS {
        return;
    }

    // Enforce the ring-buffer cap before appending.
    if snaps.len() >= MAX_SNAPSHOTS {
        // Safety check: only evict the oldest entry when it falls outside the
        // maximum supported query window (with safety margin), so that callers
        // using the longest window always have a valid start anchor.
        let oldest_ts: u64 = snaps.first().map(|s: TwapSnapshot| s.timestamp).unwrap_or(0);
        let oldest_age = state.last_timestamp.saturating_sub(oldest_ts);
        let safe_eviction_threshold = MAX_TWAP_WINDOW_SECS.saturating_mul(EVICTION_SAFETY_FACTOR);

        if oldest_age <= safe_eviction_threshold {
            // The oldest snapshot is still within (or too close to) the maximum
            // supported window.  Skip this write to avoid dropping needed history.
            // This branch is hit only during a pool's initial fill-up phase when
            // the ring is not yet old enough to rotate safely.
            return;
        }

        // Evict the single oldest entry (O(n) Vec shift, but n ≤ MAX_SNAPSHOTS).
        snaps.remove(0);
    }

    let snap = TwapSnapshot {
        timestamp: state.last_timestamp,
        price0_cumulative: state.price0_cumulative,
        price1_cumulative: state.price1_cumulative,
    };
    snaps.push_back(snap);

    env.storage().persistent().set(&snaps_key, &snaps);
}

// ---------------------------------------------------------------------------
// TWAP query
// ---------------------------------------------------------------------------

/// Compute the time-weighted average price of `asset` (base) in terms of the
/// paired quote token over the requested window.
///
/// Returns the TWAP scaled by [`PRICE_SCALE`] (i.e. divide the result by 1e18
/// to obtain the human-readable price).
///
/// # Resolution
/// `get_twap` first extrapolates the cumulative price to `now` using the most
/// recently stored reserves (so the result is current even if no swap happened
/// in this ledger).  It then binary-searches the snapshot ring for the closest
/// checkpoint at or before `now − window_secs` to obtain the start anchor.
///
/// # Arguments
/// * `asset`         – Base token address.
/// * `window_secs`   – Look-back window in seconds.  Must be ≥ [`MIN_WINDOW_SECS`].
///
/// # Errors (via panic / contract error)
/// * Window < `MIN_WINDOW_SECS`.
/// * No observations available within the window (pool too new).
///
/// # Example (pseudo-code caller)
/// ```text
/// let raw = get_twap(&env, &xlm_address, 150);
/// let price_in_usdc = raw / PRICE_SCALE; // e.g. 0.11 USDC per XLM
/// ```
pub fn get_twap(env: &Env, asset: &Address, window_secs: u64) -> u128 {
    assert!(
        window_secs >= MIN_WINDOW_SECS,
        "window_secs must be >= MIN_WINDOW_SECS ({})",
        MIN_WINDOW_SECS
    );

    let now: u64 = env.ledger().timestamp();
    let target_start = now.saturating_sub(window_secs);

    // Load current accumulator state.
    let state_key = twap_state_key(asset);
    let current_state: TwapPoolState = env
        .storage()
        .persistent()
        .get(&state_key)
        .expect("TwapPoolState: no observations for asset");

    // Extrapolate the cumulative price to the current ledger timestamp.
    // The stored state may lag if no swap happened in this ledger, so we
    // project forward using the last known reserves.
    let elapsed_since_stored = now.saturating_sub(current_state.last_timestamp);
    let mut cumulative_now = current_state.price0_cumulative;
    if elapsed_since_stored > 0 && current_state.last_reserve0 > 0 {
        let extrapolation = (current_state.last_reserve1 * PRICE_SCALE
            / current_state.last_reserve0)
            * elapsed_since_stored as u128;
        cumulative_now = cumulative_now.wrapping_add(extrapolation);
    }

    // Load the snapshot ring and binary-search for the start anchor.
    let snaps_key = twap_snaps_key(asset);
    let snaps: Vec<TwapSnapshot> = env
        .storage()
        .persistent()
        .get(&snaps_key)
        .unwrap_or_else(|| Vec::new(env));

    // Binary search: find the last snapshot with timestamp <= target_start.
    let snap_start = find_snapshot_at_or_before(&snaps, target_start);

    match snap_start {
        None => {
            // No snapshot before target_start — pool is too new; use all available history.
            let earliest_snap = snaps.first().unwrap_or(TwapSnapshot {
                timestamp: current_state.last_timestamp,
                price0_cumulative: 0,
                price1_cumulative: 0,
            });

            let actual_window = now.saturating_sub(earliest_snap.timestamp);
            assert!(
                actual_window >= MIN_WINDOW_SECS,
                "insufficient TWAP history ({}s < {}s minimum)",
                actual_window,
                MIN_WINDOW_SECS
            );

            let delta = cumulative_now.wrapping_sub(earliest_snap.price0_cumulative);
            delta / actual_window as u128
        }
        Some(start_snap) => {
            let actual_window = now.saturating_sub(start_snap.timestamp);
            assert!(actual_window > 0, "zero-length TWAP window");

            let delta = cumulative_now.wrapping_sub(start_snap.price0_cumulative);
            delta / actual_window as u128
        }
    }
}

/// Binary-search the snapshot ring for the entry with the greatest timestamp ≤ `target_ts`.
///
/// Returns `Some(snapshot)` when a qualifying entry exists, or `None` when
/// either the ring is empty or every snapshot is strictly newer than `target_ts`.
///
/// # Correctness after eviction
/// Because [`maybe_write_snapshot`] only evicts entries older than
/// `MAX_TWAP_WINDOW_SECS × EVICTION_SAFETY_FACTOR`, the oldest retained
/// snapshot is always outside the maximum query window.  Any call to
/// `find_snapshot_at_or_before` with `target_ts = now − window_secs` where
/// `window_secs ≤ MAX_TWAP_WINDOW_SECS` will therefore find a valid anchor.
///
/// # Complexity
/// O(log n) comparisons via binary search, where n ≤ [`MAX_SNAPSHOTS`].
fn find_snapshot_at_or_before(snaps: &Vec<TwapSnapshot>, target_ts: u64) -> Option<TwapSnapshot> {
    let len = snaps.len();
    if len == 0 {
        return None;
    }

    // Binary search: maintain an invariant window [lo, hi).
    // Invariant: snaps[lo].timestamp <= target_ts (if lo is a valid candidate).
    let mut lo: u32 = 0;
    let mut hi: u32 = len;
    let mut result: Option<TwapSnapshot> = None;

    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let snap: TwapSnapshot = snaps.get(mid).unwrap();
        if snap.timestamp <= target_ts {
            // mid is a valid candidate; try to find a later one.
            result = Some(snap);
            lo = mid + 1;
        } else {
            // mid is too new; search left half.
            hi = mid;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Convenience: read current pool state (used by oracle fallback)
// ---------------------------------------------------------------------------

/// Returns the most-recently stored [`TwapPoolState`], if any.
pub fn get_pool_state(env: &Env, asset: &Address) -> Option<TwapPoolState> {
    env.storage().persistent().get(&twap_state_key(asset))
}

/// Returns the snapshot ring for `asset` as a `Vec<TwapSnapshot>`.
///
/// Exposed for testing and external inspection.  The ring is ordered oldest-first.
pub fn get_snapshots(env: &Env, asset: &Address) -> Vec<TwapSnapshot> {
    env.storage()
        .persistent()
        .get(&twap_snaps_key(asset))
        .unwrap_or_else(|| Vec::new(env))
}
