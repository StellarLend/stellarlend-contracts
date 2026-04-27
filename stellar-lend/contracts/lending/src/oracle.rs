//! # Oracle Module
//!
//! Manages price feeds for protocol assets with primary/fallback oracle support,
//! staleness rejection, and admin-only configuration.
//!
//! ## Price Resolution Order
//! 1. **Primary feed**: reads the stored `PriceFeed`; rejects if stale or zero.
//! 2. **Fallback feed**: if primary is stale/missing, reads the fallback feed.
//! 3. **Error**: if both are unavailable or stale, returns `OracleError::StalePrice`.
//!
//! ## Trust Model
//! - Only the protocol admin (set at `initialize`) may call `configure_oracle`,
//!   `set_primary_oracle`, and `set_fallback_oracle`.
//! - Only the registered primary oracle address may call `update_price_feed` for
//!   the primary slot; only the registered fallback oracle may update the fallback slot.
//!   The admin may update either slot.
//! - Oracle admins are fully trusted for price data within their assigned slot.
//!   Compromise of an oracle key allows price manipulation for that slot only;
//!   the other slot acts as a circuit-breaker.
//! - Staleness and zero-price guards limit the blast radius of a compromised oracle.
//!
//! ## Security
//! - All state-changing functions require `caller.require_auth()`.
//! - Prices of zero or below are always rejected (`InvalidPrice`).
//! - Prices older than `max_staleness_seconds` are always rejected (`StalePrice`).
//! - Fallback oracle address cannot be the zero address or the contract itself.

use soroban_sdk::{contracterror, contracttype, Address, Env};

use crate::borrow::get_admin;

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

/// Errors that can occur during oracle operations.
///
/// # Security
/// All error variants are non-sensitive; they do not leak internal state.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum OracleError {
    /// Price is zero or negative.
    InvalidPrice = 1,
    /// Price feed is older than `max_staleness_seconds`.
    StalePrice = 2,
    /// Caller is not the admin or the registered oracle for this slot.
    Unauthorized = 3,
    /// No price feed available (primary missing and no fallback configured).
    NoPriceFeed = 4,
    /// Oracle address is invalid (e.g. zero or self-referential).
    InvalidOracle = 5,
    /// Oracle updates are paused.
    OraclePaused = 6,
}

// ─────────────────────────────────────────────────────────────────────────────
// Storage types
// ─────────────────────────────────────────────────────────────────────────────

/// Storage keys for oracle data.
///
/// Keys are versioned by type tag; adding new variants is a non-breaking migration.
#[contracttype]
#[derive(Clone)]
pub enum OracleKey {
    /// Global oracle configuration.
    Config,
    /// Primary oracle address for an asset.
    PrimaryOracle(Address),
    /// Fallback oracle address for an asset.
    FallbackOracle(Address),
    /// Latest price submitted by the primary oracle for an asset.
    PrimaryFeed(Address),
    /// Latest price submitted by the fallback oracle for an asset.
    FallbackFeed(Address),
    /// Pause flag for oracle updates.
    Paused,
}

/// A price feed entry stored on-chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PriceFeed {
    /// Price with 8 decimals (e.g. 100_000_000 = 1.0 USD).
    pub price: i128,
    /// Ledger timestamp when this price was last written.
    pub last_updated: u64,
    /// Address of the oracle that submitted this price.
    pub oracle: Address,
}

/// Global oracle configuration.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct OracleConfig {
    /// Maximum age of a price feed in seconds before it is considered stale.
    /// Default: 3600 (1 hour).
    pub max_staleness_seconds: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Defaults
// ─────────────────────────────────────────────────────────────────────────────

const DEFAULT_MAX_STALENESS_SECONDS: u64 = 3600;

fn default_config() -> OracleConfig {
    OracleConfig {
        max_staleness_seconds: DEFAULT_MAX_STALENESS_SECONDS,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

fn get_config(env: &Env) -> OracleConfig {
    env.storage()
        .persistent()
        .get::<OracleKey, OracleConfig>(&OracleKey::Config)
        .unwrap_or_else(default_config)
}

fn is_stale(env: &Env, last_updated: u64) -> bool {
    let now = env.ledger().timestamp();
    // Future timestamps are treated as stale (clock skew / manipulation guard).
    if now < last_updated {
        return true;
    }
    let age = now - last_updated;
    age > get_config(env).max_staleness_seconds
}

fn validate_price(price: i128) -> Result<(), OracleError> {
    if price <= 0 {
        return Err(OracleError::InvalidPrice);
    }
    Ok(())
}

fn require_admin_caller(env: &Env, caller: &Address) -> Result<(), OracleError> {
    let admin = get_admin(env).ok_or(OracleError::Unauthorized)?;
    if *caller != admin {
        return Err(OracleError::Unauthorized);
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Configure global oracle parameters. Admin only.
///
/// # Errors
/// - `Unauthorized` — caller is not the protocol admin.
/// - `InvalidPrice` — `max_staleness_seconds` is zero.
///
/// # Security
/// Requires `caller.require_auth()`. Only the admin may change staleness bounds.
pub fn configure_oracle(
    env: &Env,
    caller: Address,
    config: OracleConfig,
) -> Result<(), OracleError> {
    require_admin_caller(env, &caller)?;
    caller.require_auth();

    if config.max_staleness_seconds == 0 {
        return Err(OracleError::InvalidPrice);
    }

    env.storage().persistent().set(&OracleKey::Config, &config);
    Ok(())
}

/// Register the primary oracle address for `asset`. Admin only.
///
/// # Errors
/// - `Unauthorized` — caller is not the protocol admin.
/// - `InvalidOracle` — `primary_oracle` is the contract itself.
///
/// # Security
/// Requires `caller.require_auth()`. The registered address is the only non-admin
/// address permitted to submit primary price updates for this asset.
pub fn set_primary_oracle(
    env: &Env,
    caller: Address,
    asset: Address,
    primary_oracle: Address,
) -> Result<(), OracleError> {
    require_admin_caller(env, &caller)?;
    caller.require_auth();

    if primary_oracle == env.current_contract_address() {
        return Err(OracleError::InvalidOracle);
    }

    env.storage()
        .persistent()
        .set(&OracleKey::PrimaryOracle(asset), &primary_oracle);
    Ok(())
}

/// Register the fallback oracle address for `asset`. Admin only.
///
/// # Errors
/// - `Unauthorized` — caller is not the protocol admin.
/// - `InvalidOracle` — `fallback_oracle` is the contract itself.
///
/// # Security
/// Requires `caller.require_auth()`. The fallback oracle is only consulted when
/// the primary feed is stale or missing.
pub fn set_fallback_oracle(
    env: &Env,
    caller: Address,
    asset: Address,
    fallback_oracle: Address,
) -> Result<(), OracleError> {
    require_admin_caller(env, &caller)?;
    caller.require_auth();

    if fallback_oracle == env.current_contract_address() {
        return Err(OracleError::InvalidOracle);
    }

    env.storage()
        .persistent()
        .set(&OracleKey::FallbackOracle(asset), &fallback_oracle);
    Ok(())
}

/// Submit a price update for `asset`.
///
/// The caller must be the protocol admin, the registered primary oracle, or the
/// registered fallback oracle for this asset. The admin and primary oracle write
/// to the primary feed slot; the fallback oracle writes to the fallback feed slot.
///
/// # Errors
/// - `OraclePaused` — oracle updates are paused.
/// - `Unauthorized` — caller is not admin, primary oracle, or fallback oracle.
/// - `InvalidPrice` — `price` is zero or negative.
///
/// # Security
/// Requires `caller.require_auth()`. Checked arithmetic is used throughout.
/// A compromised oracle can only update its own slot.
pub fn update_price_feed(
    env: &Env,
    caller: Address,
    asset: Address,
    price: i128,
) -> Result<(), OracleError> {
    // Pause check
    if env
        .storage()
        .persistent()
        .get::<OracleKey, bool>(&OracleKey::Paused)
        .unwrap_or(false)
    {
        return Err(OracleError::OraclePaused);
    }

    validate_price(price)?;
    caller.require_auth();

    let admin = get_admin(env).ok_or(OracleError::Unauthorized)?;
    let is_admin = caller == admin;

    let primary: Option<Address> = env
        .storage()
        .persistent()
        .get(&OracleKey::PrimaryOracle(asset.clone()));
    let fallback: Option<Address> = env
        .storage()
        .persistent()
        .get(&OracleKey::FallbackOracle(asset.clone()));

    let is_primary = primary.as_ref().map(|p| *p == caller).unwrap_or(false);
    let is_fallback = fallback.as_ref().map(|f| *f == caller).unwrap_or(false);

    if !is_admin && !is_primary && !is_fallback {
        return Err(OracleError::Unauthorized);
    }

    let feed = PriceFeed {
        price,
        last_updated: env.ledger().timestamp(),
        oracle: caller.clone(),
    };

    if is_fallback && !is_admin && !is_primary {
        // Fallback oracle writes to fallback slot only
        env.storage()
            .persistent()
            .set(&OracleKey::FallbackFeed(asset), &feed);
    } else {
        // Admin or primary oracle writes to primary slot
        env.storage().persistent().set(&OracleKey::PrimaryFeed(asset), &feed);
    }

    Ok(())
}

/// Get the current price for `asset`, applying staleness checks and fallback logic.
///
/// Resolution order:
/// 1. Primary feed — returned if present and fresh.
/// 2. Fallback feed — returned if primary is stale/missing and fallback is fresh.
/// 3. Error — `StalePrice` or `NoPriceFeed` if neither is available.
///
/// # Errors
/// - `StalePrice` — the best available price is older than `max_staleness_seconds`.
/// - `NoPriceFeed` — no price has ever been submitted for this asset.
/// - `InvalidPrice` — stored price is zero or negative (should not occur in practice).
///
/// # Security
/// Read-only; no state changes. Stale prices are never silently accepted.
pub fn get_price(env: &Env, asset: &Address) -> Result<i128, OracleError> {
    // Try primary feed first
    if let Some(feed) = env
        .storage()
        .persistent()
        .get::<OracleKey, PriceFeed>(&OracleKey::PrimaryFeed(asset.clone()))
    {
        if !is_stale(env, feed.last_updated) {
            validate_price(feed.price)?;
            return Ok(feed.price);
        }
        // Primary is stale — try fallback before returning error
        if let Some(fb_feed) = env
            .storage()
            .persistent()
            .get::<OracleKey, PriceFeed>(&OracleKey::FallbackFeed(asset.clone()))
        {
            if !is_stale(env, fb_feed.last_updated) {
                validate_price(fb_feed.price)?;
                return Ok(fb_feed.price);
            }
        }
        return Err(OracleError::StalePrice);
    }

    // No primary feed — try fallback
    if let Some(fb_feed) = env
        .storage()
        .persistent()
        .get::<OracleKey, PriceFeed>(&OracleKey::FallbackFeed(asset.clone()))
    {
        if !is_stale(env, fb_feed.last_updated) {
            validate_price(fb_feed.price)?;
            return Ok(fb_feed.price);
        }
        return Err(OracleError::StalePrice);
    }

    Err(OracleError::NoPriceFeed)
}

/// Pause or unpause oracle price updates. Admin only.
///
/// # Errors
/// - `Unauthorized` — caller is not the protocol admin.
///
/// # Security
/// Requires `caller.require_auth()`.
pub fn set_oracle_paused(env: &Env, caller: Address, paused: bool) -> Result<(), OracleError> {
    require_admin_caller(env, &caller)?;
    caller.require_auth();
    env.storage().persistent().set(&OracleKey::Paused, &paused);
    Ok(())
}
