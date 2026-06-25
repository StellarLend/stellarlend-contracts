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
/// # Storage keys
///
/// | Key                              | Type              | Meaning                          |
/// |----------------------------------|-------------------|----------------------------------|
/// | `TwapState(asset)`               | `TwapPoolState`   | Cumulative prices + last ts      |
/// | `TwapSnapshot(asset, ts)`        | `TwapSnapshot`    | Historic checkpoint              |
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

/// Maximum number of snapshots retained per asset (≈ 24 h at 60 s interval → 1440).
pub const MAX_SNAPSHOTS: u32 = 1440;

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
#[contracttype]
#[derive(Clone, Debug)]
pub struct TwapSnapshot {
    pub timestamp: u64,
    pub price0_cumulative: u128,
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

fn maybe_write_snapshot(env: &Env, asset: &Address, state: &TwapPoolState) {
    let snaps_key = twap_snaps_key(asset);
    let mut snaps: Vec<TwapSnapshot> = env
        .storage()
        .persistent()
        .get(&snaps_key)
        .unwrap_or_else(|| Vec::new(env));

    let last_snap_ts = snaps.last().map(|s: TwapSnapshot| s.timestamp).unwrap_or(0);

    if state.last_timestamp.saturating_sub(last_snap_ts) >= SNAPSHOT_INTERVAL_SECS {
        let snap = TwapSnapshot {
            timestamp: state.last_timestamp,
            price0_cumulative: state.price0_cumulative,
            price1_cumulative: state.price1_cumulative,
        };
        snaps.push_back(snap);

        // Trim oldest entries if we exceed the cap.
        while snaps.len() > MAX_SNAPSHOTS {
            snaps.remove(0);
        }

        env.storage().persistent().set(&snaps_key, &snaps);
    }
}

// ---------------------------------------------------------------------------
// TWAP query
// ---------------------------------------------------------------------------

/// Compute the time-weighted average price of `asset` (base) in terms of the
/// paired quote token over the requested window.
///
/// Returns the TWAP scaled by `PRICE_SCALE` (i.e. divide the result by 1e18
/// to obtain the human-readable price).
///
/// # Arguments
/// * `asset`         – Base token address.
/// * `window_secs`   – Look-back window in seconds.  Must be ≥ `MIN_WINDOW_SECS`.
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

    // First update the in-memory accumulator to the current ledger (the stored
    // state may lag if no swap happened in this ledger).
    let elapsed_since_stored = now.saturating_sub(current_state.last_timestamp);
    let mut cumulative_now = current_state.price0_cumulative;
    if elapsed_since_stored > 0 && current_state.last_reserve0 > 0 {
        let extrapolation = (current_state.last_reserve1 * PRICE_SCALE
            / current_state.last_reserve0)
            * elapsed_since_stored as u128;
        cumulative_now = cumulative_now.wrapping_add(extrapolation);
    }

    // Find the snapshot closest to (but not after) target_start.
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
            // No snapshot before target_start — pool is too new, use all available history.
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

/// Returns the snapshot with the greatest timestamp ≤ `target_ts`, or `None`.
fn find_snapshot_at_or_before(snaps: &Vec<TwapSnapshot>, target_ts: u64) -> Option<TwapSnapshot> {
    let len = snaps.len();
    if len == 0 {
        return None;
    }

    // Linear scan from the end (most-recent snapshots are at the back).
    // For MAX_SNAPSHOTS=1440 this is acceptable; a binary search would require
    // random-access which Soroban Vec supports via `.get(index)`.
    let mut result: Option<TwapSnapshot> = None;
    for i in 0..len {
        let snap: TwapSnapshot = snaps.get(i).unwrap();
        if snap.timestamp <= target_ts {
            result = Some(snap);
        } else {
            break; // Vec is ordered; no need to continue.
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Convenience: read current pool state (used by oracle fallback)
// ---------------------------------------------------------------------------

/// Returns the most-recently stored `TwapPoolState`, if any.
pub fn get_pool_state(env: &Env, asset: &Address) -> Option<TwapPoolState> {
    env.storage().persistent().get(&twap_state_key(asset))
}
