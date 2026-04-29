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
//! ## Staleness Configuration
//! Staleness limits can be set at two levels:
//! - **Global** (`configure_oracle`): applies to all assets that do not have a
//!   per-asset override. Default is 3 600 s (1 hour).
//! - **Per-asset** (`set_asset_max_staleness`): overrides the global limit for a
//!   single asset. Useful when different assets have different update cadences
//!   (e.g. a stablecoin oracle updates every 60 s while a long-tail asset updates
//!   every 30 min). A per-asset value of `0` is rejected; call
//!   `clear_asset_max_staleness` to remove the override and fall back to global.
//!
//! ## Trust Model
//! - Only the protocol admin (set at `initialize`) may call `configure_oracle`,
//!   `set_primary_oracle`, `set_fallback_oracle`, `set_asset_max_staleness`, and
//!   `clear_asset_max_staleness`.
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
//! - Prices older than the effective `max_staleness_seconds` (per-asset if set,
//!   otherwise global) are always rejected (`StalePrice`).
//! - Fallback oracle address cannot be the zero address or the contract itself.
//! - Future timestamps in stored feeds are treated as stale (clock-skew guard).

use soroban_sdk::{contracterror, contracttype, Address};

// ── Storage key namespace ────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

/// Errors that can occur during oracle operations.
///
/// # Security
/// All error variants are non-sensitive; they do not leak internal state.
pub use crate::errors::OracleError;

// ─────────────────────────────────────────────────────────────────────────────
// Storage types
// ─────────────────────────────────────────────────────────────────────────────

/// Storage keys for oracle data.
///
/// Keys are versioned by type tag; adding new variants is a non-breaking migration.
/// Existing variants are unchanged so no storage migration is required when
/// upgrading from a version that did not have `AssetStaleness`.
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
    OraclePaused,
    /// Per-asset maximum staleness override (seconds).
    /// When present, takes precedence over the global `Config.max_staleness_seconds`.
    AssetStaleness(Address),
}

// ── Core state structs ───────────────────────────────────────────────────────

/// Snapshot of a user's position in a single asset market.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPosition {
    pub deposited: i128,
    pub borrowed: i128,
}

/// Protocol-level market state for one asset.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketState {
    pub total_deposits: i128,
    pub total_borrows: i128,
    pub reserves: i128,
    pub bad_debt: i128,
    pub is_active: bool,
    pub is_frozen: bool, // frozen during emergency shutdown
}

/// Return the effective max-staleness for `asset`.
///
/// Resolution order:
/// 1. Per-asset override (`AssetStaleness(asset)`) — set via `set_asset_max_staleness`.
/// 2. Global config (`Config.max_staleness_seconds`).
/// 3. Hard-coded default (`DEFAULT_MAX_STALENESS_SECONDS`) when neither is stored.
fn effective_max_staleness(env: &Env, asset: &Address) -> u64 {
    if let Some(per_asset) = env
        .storage()
        .persistent()
        .get::<OracleKey, u64>(&OracleKey::AssetStaleness(asset.clone()))
    {
        return per_asset;
    }
    get_config(env).max_staleness_seconds
}

fn is_stale(env: &Env, asset: &Address, last_updated: u64) -> bool {
    let now = env.ledger().timestamp();
    // Future timestamps are treated as stale (clock skew / manipulation guard).
    if now < last_updated {
        return true;
    }
    let age = now - last_updated;
    age > effective_max_staleness(env, asset)
}

impl MarketState {
    /// Solvency invariant: net assets must never be negative.
    /// net_assets = reserves + total_deposits - total_borrows - bad_debt
    pub fn check_solvency_invariant(&self) -> bool {
        let net = self.reserves + self.total_deposits - self.total_borrows - self.bad_debt;
        net >= 0
    }

    /// Bad-debt invariant: cumulative bad debt must never be negative.
    pub fn check_bad_debt_non_negative(&self) -> bool {
        self.bad_debt >= 0
    }

    /// Reserves invariant: reserves must never be negative.
    pub fn check_reserves_non_negative(&self) -> bool {
        self.reserves >= 0
    }
}

/// Result returned by view functions for off-chain consumption.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolReport {
    pub asset: Address,
    pub total_deposits: i128,
    pub total_borrows: i128,
    pub reserves: i128,
    pub bad_debt: i128,
    pub utilisation_bps: i128, // borrows / deposits * 10_000
    pub is_solvent: bool,
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
        .get::<OracleKey, bool>(&OracleKey::OraclePaused)
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
        env.storage()
            .persistent()
            .set(&OracleKey::PrimaryFeed(asset), &feed);
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
        if !is_stale(env, asset, feed.last_updated) {
            validate_price(feed.price)?;
            return Ok(feed.price);
        }
        // Primary is stale — try fallback before returning error
        if let Some(fb_feed) = env
            .storage()
            .persistent()
            .get::<OracleKey, PriceFeed>(&OracleKey::FallbackFeed(asset.clone()))
        {
            if !is_stale(env, asset, fb_feed.last_updated) {
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
        if !is_stale(env, asset, fb_feed.last_updated) {
            validate_price(fb_feed.price)?;
            return Ok(fb_feed.price);
        }
        return Err(OracleError::StalePrice);
    }

    Err(OracleError::NoPriceFeed)
}

/// Set a per-asset maximum staleness override for `asset`. Admin only.
///
/// When set, this value takes precedence over the global `OracleConfig.max_staleness_seconds`
/// for staleness checks on `asset`. This allows tighter or looser bounds per asset
/// depending on its oracle update cadence.
///
/// # Arguments
/// * `caller` — Must be the protocol admin.
/// * `asset`  — The asset address to configure.
/// * `max_staleness_seconds` — Maximum age in seconds. Must be > 0.
///
/// # Errors
/// - `Unauthorized`  — caller is not the protocol admin.
/// - `InvalidPrice`  — `max_staleness_seconds` is zero (reuses `InvalidPrice` for
///   consistency with `configure_oracle`; semantically means "invalid parameter").
///
/// # Storage
/// Writes `OracleKey::AssetStaleness(asset)` → `u64`. No existing keys are
/// modified, so no migration is required.
///
/// # Security
/// Requires `caller.require_auth()`. Only the admin may tighten or loosen
/// per-asset staleness bounds.
pub fn set_asset_max_staleness(
    env: &Env,
    caller: Address,
    asset: Address,
    max_staleness_seconds: u64,
) -> Result<(), OracleError> {
    require_admin_caller(env, &caller)?;
    caller.require_auth();

    if max_staleness_seconds == 0 {
        return Err(OracleError::InvalidPrice);
    }

    env.storage()
        .persistent()
        .set(&OracleKey::AssetStaleness(asset), &max_staleness_seconds);
    Ok(())
}

/// Remove the per-asset staleness override for `asset`, reverting to the global config. Admin only.
///
/// After this call, `get_price` for `asset` will use `OracleConfig.max_staleness_seconds`
/// (or the hard-coded default if no global config has been set).
///
/// # Errors
/// - `Unauthorized` — caller is not the protocol admin.
///
/// # Security
/// Requires `caller.require_auth()`.
pub fn clear_asset_max_staleness(
    env: &Env,
    caller: Address,
    asset: Address,
) -> Result<(), OracleError> {
    require_admin_caller(env, &caller)?;
    caller.require_auth();

    env.storage()
        .persistent()
        .remove(&OracleKey::AssetStaleness(asset));
    Ok(())
}

/// Return the effective max-staleness for `asset` in seconds.
///
/// Returns the per-asset override if one has been set via `set_asset_max_staleness`,
/// otherwise returns the global `OracleConfig.max_staleness_seconds` (or the
/// hard-coded default of 3 600 s if no global config has been stored).
///
/// This is a read-only helper for frontends and monitoring tools.
pub fn get_asset_max_staleness(env: &Env, asset: &Address) -> u64 {
    effective_max_staleness(env, asset)
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
    env.storage().persistent().set(&OracleKey::OraclePaused, &paused);
    Ok(())
}
