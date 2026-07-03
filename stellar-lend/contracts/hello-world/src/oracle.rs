//! # Oracle Module
//!
//! Manages price feeds for all protocol assets with staleness checks, deviation
//! guards, caching, and fallback oracle support.
//!
//! ## Price Resolution Order
//! 1. **Cache**: returns a cached price if the TTL has not expired.
//! 2. **Primary feed**: reads the on-chain `PriceFeed` entry; rejects if stale.
//! 3. **AMM TWAP fallback**: if the primary is stale or missing, derives a
//!    time-weighted average price from the on-chain AMM pool reserves.
//!    Emits [`TwapFallbackUsedEvent`] when this path is taken.
//! 4. **Configured fallback oracle**: legacy fallback oracle address support.
//!
//! `get_pool_twap_price` exposes step 3's underlying AMM TWAP value directly
//! as a read-only view (returning `None` rather than panicking when the
//! window isn't covered yet), so integrators/monitors can inspect the
//! fallback without flying blind during a primary-feed outage.
//!
//! ## Safety
//! - Price deviation between consecutive updates is bounded (default ±5%).
//! - Staleness threshold defaults to 1 hour; configurable by admin.
//! - Sanity-check bounds on min/max price are enforced on every update.
//! - Only the admin or the designated oracle address may submit price updates.
//!
//! ## Event observability
//! Every transition from primary to TWAP fallback is signalled by a structured,
//! versioned [`TwapFallbackUsedEvent`] so that indexers and alerting systems
//! can detect oracle degradation in real time without polling.

#![allow(unused)]
use crate::deposit::DepositDataKey;
use crate::events::{
    emit_price_updated, emit_twap_fallback_used, PriceUpdatedEvent, PRIMARY_FEED_ABSENT,
};
use crate::risk_management::get_admin;
use soroban_sdk::{
    contracterror, contracttype, symbol_short, Address, Env, IntoVal, Map, Symbol, Val, Vec,
};

use crate::amm_twap;

// ---------------------------------------------------------------------------
// TWAP fallback constants
// ---------------------------------------------------------------------------

/// TWAP window used when falling back to AMM pricing.
/// 150 s ≈ 30 ledger closes; sufficient to prevent single-block manipulation.
pub const TWAP_FALLBACK_WINDOW_SECS: u64 = 150;

/// Scale used by the AMM TWAP accumulator (10^18).
pub const TWAP_PRICE_SCALE: u128 = amm_twap::PRICE_SCALE;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during oracle operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum OracleError {
    /// Invalid price (zero or negative)
    InvalidPrice = 1,
    /// Price is too stale (older than threshold)
    StalePrice = 2,
    /// Price deviation exceeds maximum allowed
    PriceDeviationExceeded = 3,
    /// Oracle address is invalid
    InvalidOracle = 4,
    /// Oracle update is paused
    OraclePaused = 5,
    /// Overflow occurred during calculation
    Overflow = 6,
    /// Unauthorized access
    Unauthorized = 7,
    /// Asset not supported
    AssetNotSupported = 8,
    /// Fallback oracle not configured
    FallbackNotConfigured = 9,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Storage keys for oracle-related data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum OracleDataKey {
    /// Latest price feed data for a specific asset
    PriceFeed(Address),
    /// Address of the designated fallback oracle for an asset
    FallbackOracle(Address),
    /// Primary oracle address for an asset
    PrimaryOracle(Address),
    /// Fallback price feed for an asset
    FallbackFeed(Address),
    /// Transient price cache for improved gas efficiency
    PriceCache(Address),
    /// Global oracle safety and operational parameters
    OracleConfig,
    /// Pause switches specifically for oracle updates: Map<Symbol, bool>
    PauseSwitches,
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Price feed data structure
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PriceFeed {
    pub price: i128,
    pub last_updated: u64,
    pub oracle: Address,
    pub decimals: u32,
}

/// Cached price data
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CachedPrice {
    pub price: i128,
    pub cached_at: u64,
    pub ttl: u64,
}

/// Full oracle configuration (admin-managed).
///
/// Stored under [`OracleDataKey::OracleConfig`] and loaded by internal helpers.
/// For the simpler per-call configuration used with [`get_price_with_fallback`],
/// see [`SimplifiedOracleConfig`] (re-exported as [`OracleConfig`]).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FullOracleConfig {
    /// Maximum price deviation in basis points (e.g., 500 = 5%)
    pub max_deviation_bps: i128,
    /// Maximum staleness in seconds
    pub max_staleness_seconds: u64,
    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,
    /// Minimum price sanity check
    pub min_price: i128,
    /// Maximum price sanity check
    pub max_price: i128,
}

/// Price result that indicates whether the TWAP fallback was used
#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceResult {
    /// Price scaled by TWAP_PRICE_SCALE (1e18) when is_twap_fallback is true,
    /// otherwise raw oracle price
    pub price_scaled: u128,
    /// Unix timestamp when the price was last observed
    pub timestamp: u64,
    /// True when the AMM TWAP fallback was used instead of the primary oracle
    pub is_twap_fallback: bool,
}

// ---------------------------------------------------------------------------
// Default configuration
// ---------------------------------------------------------------------------

const DEFAULT_MAX_DEVIATION_BPS: i128 = 500;
const DEFAULT_MAX_STALENESS_SECONDS: u64 = 3600;
const DEFAULT_CACHE_TTL_SECONDS: u64 = 300;
const DEFAULT_MIN_PRICE: i128 = 1;
const DEFAULT_MAX_PRICE: i128 = i128::MAX;

fn get_default_config() -> FullOracleConfig {
    FullOracleConfig {
        max_deviation_bps: DEFAULT_MAX_DEVIATION_BPS,
        max_staleness_seconds: DEFAULT_MAX_STALENESS_SECONDS,
        cache_ttl_seconds: DEFAULT_CACHE_TTL_SECONDS,
        min_price: DEFAULT_MIN_PRICE,
        max_price: DEFAULT_MAX_PRICE,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn get_oracle_config(env: &Env) -> FullOracleConfig {
    let config_key = OracleDataKey::OracleConfig;
    env.storage()
        .persistent()
        .get::<OracleDataKey, FullOracleConfig>(&config_key)
        .unwrap_or_else(get_default_config)
}

fn get_primary_oracle(env: &Env, asset: &Address) -> Option<Address> {
    let key = OracleDataKey::PrimaryOracle(asset.clone());
    env.storage()
        .persistent()
        .get::<OracleDataKey, Address>(&key)
}

fn get_fallback_oracle(env: &Env, asset: &Address) -> Option<Address> {
    let key = OracleDataKey::FallbackOracle(asset.clone());
    env.storage()
        .persistent()
        .get::<OracleDataKey, Address>(&key)
}

fn validate_price(env: &Env, price: i128) -> Result<(), OracleError> {
    if price <= 0 {
        return Err(OracleError::InvalidPrice);
    }
    let config = get_oracle_config(env);
    if price < config.min_price || price > config.max_price {
        return Err(OracleError::InvalidPrice);
    }
    Ok(())
}

fn is_price_stale(env: &Env, last_updated: u64) -> bool {
    let config = get_oracle_config(env);
    let current_time = env.ledger().timestamp();
    if current_time < last_updated {
        return true;
    }
    let age = current_time - last_updated;
    age > config.max_staleness_seconds
}

fn check_price_deviation(env: &Env, new_price: i128, old_price: i128) -> Result<(), OracleError> {
    if old_price == 0 {
        return Ok(());
    }
    let config = get_oracle_config(env);
    let diff = if new_price > old_price {
        new_price
            .checked_sub(old_price)
            .ok_or(OracleError::Overflow)?
    } else {
        old_price
            .checked_sub(new_price)
            .ok_or(OracleError::Overflow)?
    };
    let deviation_bps = diff
        .checked_mul(10000)
        .ok_or(OracleError::Overflow)?
        .checked_div(old_price)
        .ok_or(OracleError::Overflow)?;
    if deviation_bps > config.max_deviation_bps {
        return Err(OracleError::PriceDeviationExceeded);
    }
    Ok(())
}

fn get_cached_price(env: &Env, asset: &Address) -> Option<i128> {
    let cache_key = OracleDataKey::PriceCache(asset.clone());
    if let Some(cached) = env
        .storage()
        .persistent()
        .get::<OracleDataKey, CachedPrice>(&cache_key)
    {
        let current_time = env.ledger().timestamp();
        if current_time >= cached.cached_at
            && current_time <= cached.cached_at.saturating_add(cached.ttl)
        {
            return Some(cached.price);
        }
    }
    None
}

fn cache_price(env: &Env, asset: &Address, price: i128) {
    let config = get_oracle_config(env);
    let cache_key = OracleDataKey::PriceCache(asset.clone());
    let cached = CachedPrice {
        price,
        cached_at: env.ledger().timestamp(),
        ttl: config.cache_ttl_seconds,
    };
    env.storage().persistent().set(&cache_key, &cached);
}

// ---------------------------------------------------------------------------
// Events (internal oracle-specific helpers)
// ---------------------------------------------------------------------------

/// Emit a lightweight staleness alarm for off-chain monitoring.
///
/// This is a low-level operational signal.  The full fallback-used event with
/// price and age fields is emitted by [`try_twap_fallback`] via
/// [`emit_twap_fallback_used`].
fn emit_oracle_stale_event(env: &Env, asset: &Address, age_secs: u64) {
    env.events()
        .publish((symbol_short!("OrcStale"), asset.clone()), age_secs);
}

// ---------------------------------------------------------------------------
// Public: price update
// ---------------------------------------------------------------------------

pub fn update_price_feed(
    env: &Env,
    caller: Address,
    asset: Address,
    price: i128,
    decimals: u32,
    oracle: Address,
) -> Result<i128, OracleError> {
    // Check if oracle updates are paused
    let pause_key = OracleDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<OracleDataKey, Map<Symbol, bool>>(&pause_key)
    {
        if let Some(paused) = pause_map.get(Symbol::new(env, "pause_oracle")) {
            if paused {
                return Err(OracleError::OraclePaused);
            }
        }
    }

    let is_admin = get_admin(env).map(|admin| admin == caller).unwrap_or(false);
    let primary = get_primary_oracle(env, &asset);
    let fallback = get_fallback_oracle(env, &asset);

    let is_primary = primary.map(|p| p == caller).unwrap_or(false);
    let is_fallback = fallback.map(|f| f == caller).unwrap_or(false);

    if !is_admin && !is_primary && !is_fallback {
        return Err(OracleError::Unauthorized);
    }
    if !is_admin && caller != oracle {
        return Err(OracleError::Unauthorized);
    }

    validate_price(env, price)?;

    let feed_key = if is_fallback && !is_primary && !is_admin {
        OracleDataKey::FallbackFeed(asset.clone())
    } else {
        OracleDataKey::PriceFeed(asset.clone())
    };

    let current_feed = env
        .storage()
        .persistent()
        .get::<OracleDataKey, PriceFeed>(&feed_key);

    if let Some(ref feed) = current_feed {
        check_price_deviation(env, price, feed.price)?;
    }

    let timestamp = env.ledger().timestamp();
    let oracle_clone = oracle.clone();
    let new_feed = PriceFeed {
        price,
        last_updated: timestamp,
        oracle: oracle_clone.clone(),
        decimals,
    };

    env.storage().persistent().set(&feed_key, &new_feed);

    if is_admin {
        let primary_key = OracleDataKey::PrimaryOracle(asset.clone());
        env.storage().persistent().set(&primary_key, &oracle);
    }

    cache_price(env, &asset, price);

    emit_price_updated(
        env,
        PriceUpdatedEvent {
            actor: caller,
            asset: asset.clone(),
            price,
            decimals,
            oracle: oracle_clone,
            timestamp,
        },
    );

    Ok(price)
}

// ---------------------------------------------------------------------------
// Public: price retrieval with TWAP fallback
// ---------------------------------------------------------------------------

/// Get price for an asset.
///
/// Resolution order:
/// 1. Cache (if valid TTL)
/// 2. Primary feed (if fresh)
/// 3. AMM TWAP (if primary is stale) — emits [`emit_oracle_stale_event`] and
///    then [`emit_twap_fallback_used`] with the resolved TWAP price and the
///    age of the stale feed.
/// 4. Configured fallback oracle feed (legacy path)
///
/// A [`TwapFallbackUsedEvent`] is emitted **only** when the TWAP path is
/// actually taken (steps 3 or the no-primary-feed variant at step 4 that
/// falls through to TWAP).  It is never emitted on the primary cache or feed
/// happy path.
///
/// [`TwapFallbackUsedEvent`]: crate::events::TwapFallbackUsedEvent
pub fn get_price(env: &Env, asset: &Address) -> Result<i128, OracleError> {
    // 1. Try cache first
    if let Some(cached_price) = get_cached_price(env, asset) {
        return Ok(cached_price);
    }

    // 2. Try primary feed
    let feed_key = OracleDataKey::PriceFeed(asset.clone());
    if let Some(feed) = env
        .storage()
        .persistent()
        .get::<OracleDataKey, PriceFeed>(&feed_key)
    {
        if is_price_stale(env, feed.last_updated) {
            let age = env.ledger().timestamp().saturating_sub(feed.last_updated);
            emit_oracle_stale_event(env, asset, age);

            // 3. AMM TWAP fallback when primary is stale — pass the measured age
            //    so the event carries the exact staleness duration.
            if let Ok(twap_price) = try_twap_fallback(env, asset, age) {
                return Ok(twap_price);
            }

            // 4. Configured fallback oracle (legacy)
            if let Ok(fallback_price) = get_fallback_price(env, asset) {
                return Ok(fallback_price);
            }

            return Err(OracleError::StalePrice);
        }

        cache_price(env, asset, feed.price);
        return Ok(feed.price);
    }

    // No primary feed record at all — try TWAP (primary_age_secs = PRIMARY_FEED_ABSENT),
    // then legacy fallback.
    if let Ok(twap_price) = try_twap_fallback(env, asset, PRIMARY_FEED_ABSENT) {
        return Ok(twap_price);
    }

    get_fallback_price(env, asset)
}

/// Attempt to derive a price from the AMM TWAP accumulator.
///
/// On success this function:
/// 1. Computes the TWAP over [`TWAP_FALLBACK_WINDOW_SECS`].
/// 2. Emits a structured [`TwapFallbackUsedEvent`] with the resolved price
///    and the primary feed's staleness age.
/// 3. Caches the scaled-down price and returns it.
///
/// # Arguments
/// * `primary_age_secs` — Age of the stale primary feed in seconds.  Pass
///   [`PRIMARY_FEED_ABSENT`] when no primary feed record exists.
///
/// # Errors
/// Returns [`OracleError::FallbackNotConfigured`] when the AMM pool has no
/// state (pool never initialised or has insufficient history).  In that case
/// **no event is emitted** because no price was successfully resolved.
///
/// [`TwapFallbackUsedEvent`]: crate::events::TwapFallbackUsedEvent
fn try_twap_fallback(
    env: &Env,
    asset: &Address,
    primary_age_secs: u64,
) -> Result<i128, OracleError> {
    // Guard: check pool has state before calling get_twap (avoids panic on
    // empty pool — the absence of a TwapPoolState is not an error, just means
    // the pool was never initialised).
    if amm_twap::get_pool_state(env, asset).is_none() {
        return Err(OracleError::FallbackNotConfigured);
    }

    // get_twap will panic if there is insufficient history (< MIN_WINDOW_SECS).
    // The pool-state guard above only confirms the pool exists; history depth
    // is not checked here.  That panic is the intended fail-safe: callers must
    // ensure adequate pool history before relying on the TWAP fallback.
    let twap_raw = amm_twap::get_twap(env, asset, TWAP_FALLBACK_WINDOW_SECS);

    // Emit the structured, versioned fallback event **after** a successful
    // TWAP resolution so the event always carries a valid price.
    emit_twap_fallback_used(env, asset, twap_raw, primary_age_secs);

    // Scale down from 1e18 to the protocol's i128 price format (6 decimal places).
    let price = (twap_raw / (TWAP_PRICE_SCALE / 1_000_000)) as i128;
    if price <= 0 {
        return Err(OracleError::InvalidPrice);
    }

    cache_price(env, asset, price);
    Ok(price)
}

/// Read-only view exposing the same AMM TWAP fallback price that
/// [`get_price`]'s resolution order would fall back to for `asset`, at the
/// AMM accumulator's own native scale (`TWAP_PRICE_SCALE`, i.e. 1e18 — the
/// same scale documented on `amm_twap::get_twap`). This is the *pre*-rescale
/// value; `try_twap_fallback`'s further division to 6 decimals for the
/// protocol's `i128` price format is intentionally not applied here, so
/// integrators/monitors see exactly what the AMM accumulator reports.
///
/// Calls the identical `amm_twap::get_twap` code path `try_twap_fallback`
/// uses internally — this is not a reimplementation.
///
/// Returns `None` (never panics) when no snapshot covers the requested
/// `window_secs`: the pool has no recorded state yet, there isn't enough
/// TWAP history for the window, or `window_secs` is below
/// `amm_twap::MIN_WINDOW_SECS`. Integrators/monitors should treat `None` as
/// "the fallback is not currently usable for this window" rather than
/// retrying — it means the data genuinely isn't there yet.
///
/// Pure read: never mutates contract state (unlike `get_price`, which may
/// cache the resolved price).
pub fn get_pool_twap_price(env: &Env, asset: &Address, window_secs: u64) -> Option<u128> {
    if !amm_twap::has_window_coverage(env, asset, window_secs) {
        return None;
    }
    Some(amm_twap::get_twap(env, asset, window_secs))
}

/// Get price from the configured legacy fallback oracle feed.
fn get_fallback_price(env: &Env, asset: &Address) -> Result<i128, OracleError> {
    let fallback_key = OracleDataKey::FallbackOracle(asset.clone());
    if let Some(fallback_oracle) = env
        .storage()
        .persistent()
        .get::<OracleDataKey, Address>(&fallback_key)
    {
        let feed_key = OracleDataKey::FallbackFeed(asset.clone());
        if let Some(feed) = env
            .storage()
            .persistent()
            .get::<OracleDataKey, PriceFeed>(&feed_key)
        {
            if feed.oracle == fallback_oracle && !is_price_stale(env, feed.last_updated) {
                cache_price(env, asset, feed.price);
                return Ok(feed.price);
            }
        }
    }
    Err(OracleError::FallbackNotConfigured)
}

/// Convenience wrapper for the liquidation engine.
/// Returns the collateral price scaled by TWAP_PRICE_SCALE (1e18) if TWAP
/// fallback is active, otherwise the raw oracle price cast to u128.
pub fn get_liquidation_price(env: &Env, collateral_asset: &Address) -> Result<u128, OracleError> {
    let price = get_price(env, collateral_asset)?;
    Ok(price as u128)
}

// ---------------------------------------------------------------------------
// Admin operations
// ---------------------------------------------------------------------------

pub fn set_primary_oracle(
    env: &Env,
    caller: Address,
    asset: Address,
    primary_oracle: Address,
) -> Result<(), OracleError> {
    let admin = get_admin(env).ok_or(OracleError::Unauthorized)?;
    if caller != admin {
        return Err(OracleError::Unauthorized);
    }
    let primary_key = OracleDataKey::PrimaryOracle(asset);
    env.storage()
        .persistent()
        .set(&primary_key, &primary_oracle);
    Ok(())
}

pub fn set_fallback_oracle(
    env: &Env,
    caller: Address,
    asset: Address,
    fallback_oracle: Address,
) -> Result<(), OracleError> {
    crate::admin::require_admin(env, &caller).map_err(|_| OracleError::Unauthorized)?;
    if fallback_oracle == env.current_contract_address() {
        return Err(OracleError::InvalidOracle);
    }
    let fallback_key = OracleDataKey::FallbackOracle(asset);
    env.storage()
        .persistent()
        .set(&fallback_key, &fallback_oracle);
    Ok(())
}

pub fn configure_oracle(
    env: &Env,
    caller: Address,
    config: FullOracleConfig,
) -> Result<(), OracleError> {
    crate::admin::require_admin(env, &caller).map_err(|_| OracleError::Unauthorized)?;
    if config.max_deviation_bps <= 0 || config.max_deviation_bps > 10000 {
        return Err(OracleError::InvalidPrice);
    }
    if config.max_staleness_seconds == 0 {
        return Err(OracleError::InvalidPrice);
    }
    let config_key = OracleDataKey::OracleConfig;
    env.storage().persistent().set(&config_key, &config);
    Ok(())
}

// ---------------------------------------------------------------------------
// ExternalOracle trait and test-support API
// ---------------------------------------------------------------------------

/// A pluggable oracle source used by [`get_price_with_fallback`].
///
/// Implementors return `Some((price_scaled, observation_timestamp))` when a
/// fresh price is available, or `None` on outage.  The `price_scaled` value
/// must be expressed in the same `TWAP_PRICE_SCALE` (1 × 10^18) units used
/// by the AMM TWAP accumulator.
///
/// In production, implementors wrap an on-chain oracle contract call.
/// In tests, [`MockOracle`] provides configurable staleness and outage
/// simulation without network calls.
pub trait ExternalOracle {
    /// Return `(price_scaled, observation_timestamp_secs)`, or `None` on outage.
    fn get_price(&self, env: &Env, asset: &Address) -> Option<(u128, u64)>;
}

/// Simplified oracle configuration used by [`set_oracle_config`] /
/// [`get_price_with_fallback`].  Separate from the full [`OracleConfig`] to
/// keep the test-support surface minimal.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SimplifiedOracleConfig {
    /// Oracle address (informational; not used for auth in this path).
    pub oracle_address: Address,
    /// Maximum age in seconds before a price is considered stale.
    pub max_age_secs: u64,
    /// TWAP window in seconds passed to [`amm_twap::get_twap`] on fallback.
    pub twap_window_secs: u64,
}

// Storage key for the simplified oracle config used by get_price_with_fallback.
fn simplified_config_key() -> Symbol {
    symbol_short!("SmpOrcCfg")
}

/// Persist a [`SimplifiedOracleConfig`] for use by [`get_price_with_fallback`].
///
/// Called from tests and integration harnesses to configure per-asset
/// oracle parameters without going through the full admin flow.
pub fn set_oracle_config(env: &Env, config: &SimplifiedOracleConfig) {
    env.storage()
        .persistent()
        .set(&simplified_config_key(), config);
}

// Re-export SimplifiedOracleConfig under the name used by twap_tests.rs so
// that existing test imports (`use crate::oracle::OracleConfig`) continue to
// resolve without modification.  The full admin-configured OracleConfig is
// available as `FullOracleConfig` for code that needs the extended fields.
#[allow(dead_code)]
pub type OracleConfig = SimplifiedOracleConfig;

/// Resolve a price for `asset` via `oracle`, falling back to the AMM TWAP
/// when the primary price is absent or stale.
///
/// # Resolution
/// 1. Call `oracle.get_price(env, asset)`.
/// 2. If `Some((price, obs_ts))` and `now − obs_ts ≤ max_age_secs` → primary
///    path, returns `PriceResult { is_twap_fallback: false, … }`.
/// 3. Otherwise → TWAP path: calls [`amm_twap::get_twap`] and emits
///    [`TwapFallbackUsedEvent`].  Returns `PriceResult { is_twap_fallback: true, … }`.
///
/// Panics (via [`amm_twap::get_twap`]) if the pool has insufficient history
/// and the primary oracle is also unavailable.
pub fn get_price_with_fallback(
    env: &Env,
    asset: &Address,
    oracle: &dyn ExternalOracle,
) -> PriceResult {
    let config: SimplifiedOracleConfig = env
        .storage()
        .persistent()
        .get(&simplified_config_key())
        .unwrap_or(SimplifiedOracleConfig {
            oracle_address: asset.clone(),
            max_age_secs: DEFAULT_MAX_STALENESS_SECONDS,
            twap_window_secs: TWAP_FALLBACK_WINDOW_SECS,
        });

    let now = env.ledger().timestamp();

    // Attempt primary oracle.
    if let Some((price_scaled, obs_ts)) = oracle.get_price(env, asset) {
        let age = now.saturating_sub(obs_ts);
        if age <= config.max_age_secs {
            // Primary is fresh — return without touching TWAP or emitting an event.
            return PriceResult {
                price_scaled,
                timestamp: obs_ts,
                is_twap_fallback: false,
            };
        }
        // Primary is stale — fall through to TWAP.
        let twap_raw = amm_twap::get_twap(env, asset, config.twap_window_secs);
        emit_twap_fallback_used(env, asset, twap_raw, age);
        return PriceResult {
            price_scaled: twap_raw,
            timestamp: now,
            is_twap_fallback: true,
        };
    }

    // Primary oracle returned None (outage) — fall through to TWAP.
    let twap_raw = amm_twap::get_twap(env, asset, config.twap_window_secs);
    emit_twap_fallback_used(env, asset, twap_raw, PRIMARY_FEED_ABSENT);
    PriceResult {
        price_scaled: twap_raw,
        timestamp: now,
        is_twap_fallback: true,
    }
}
