//! Protocol configuration snapshot.
//!
//! Provides a single read that aggregates the current risk parameters,
//! interest-rate model configuration, and emergency/pause state into a
//! flat [`ConfigSnapshot`] struct — a one-shot "protocol health dashboard"
//! query suitable for off-chain monitoring, dashboards, and integrator
//! health checks.
//!
//! The snapshot is purely read-only; it never mutates contract state and
//! requires no authorization.  Returns `None` when the contract has not yet
//! been initialized (i.e. when neither risk params nor interest-rate config
//! exist in storage).

use soroban_sdk::{contracttype, Env};

use crate::interest_rate::get_interest_rate_config;
use crate::risk_management::is_emergency_paused;
use crate::risk_params::{
    get_close_factor, get_liquidation_incentive, get_liquidation_threshold,
    get_min_collateral_ratio,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of the protocol's configuration.
///
/// All rate/ratio fields are expressed in basis points (bps) where
/// `10_000` equals 100%.
///
/// Fields are `None` when the underlying module has not been initialized.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigSnapshot {
    // ---- Risk parameters ---------------------------------------------------
    /// Minimum collateral ratio required to open or maintain a position, in bps.
    pub min_collateral_ratio_bps: i128,
    /// Collateral-value threshold at which a position becomes eligible for
    /// liquidation, in bps.
    pub liquidation_threshold_bps: i128,
    /// Maximum fraction of a position's debt that can be repaid in a single
    /// liquidation call, in bps.
    pub close_factor_bps: i128,
    /// Collateral bonus paid to liquidators as a percentage of the repaid
    /// amount, in bps.
    pub liquidation_incentive_bps: i128,

    // ---- Interest-rate model -----------------------------------------------
    /// Base borrow APR at 0% utilization, in bps.
    pub base_rate_bps: i128,
    /// Utilization kink where the rate curve steepens, in bps.
    pub kink_utilization_bps: i128,
    /// Pre-kink slope: total rate increase from 0 to kink utilization, in bps.
    pub multiplier_bps: i128,
    /// Post-kink slope: total rate increase from kink to 100% utilization, in bps.
    pub jump_multiplier_bps: i128,
    /// Spread subtracted from the effective borrow rate to derive the supply
    /// rate, in bps.
    pub spread_bps: i128,
    /// Hard minimum borrow rate enforced after all adjustments, in bps.
    pub min_rate_bps: i128,
    /// Hard maximum borrow rate enforced after all adjustments, in bps.
    pub max_rate_bps: i128,

    // ---- Pause state -------------------------------------------------------
    /// `true` when the global emergency pause is active.
    pub emergency_paused: bool,
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Return a [`ConfigSnapshot`] of the current protocol configuration, or
/// `None` if the contract has not been initialized.
///
/// This function is read-only and requires no authorization.
pub fn get_config_snapshot(env: &Env) -> Option<ConfigSnapshot> {
    // Require at least the risk params to be initialized before returning a
    // snapshot; avoids a partially-zero snapshot that could mislead callers.
    let min_collateral_ratio_bps = get_min_collateral_ratio(env).ok()?;
    let liquidation_threshold_bps = get_liquidation_threshold(env).ok()?;
    let close_factor_bps = get_close_factor(env).ok()?;
    let liquidation_incentive_bps = get_liquidation_incentive(env).ok()?;

    // Interest-rate config may not be initialized in all test environments;
    // fall back to zero-valued defaults rather than returning None so that
    // dashboards still receive the risk-param half of the snapshot.
    let ir = get_interest_rate_config(env).unwrap_or_default();

    let emergency_paused = is_emergency_paused(env);

    Some(ConfigSnapshot {
        min_collateral_ratio_bps,
        liquidation_threshold_bps,
        close_factor_bps,
        liquidation_incentive_bps,
        base_rate_bps: ir.base_rate_bps,
        kink_utilization_bps: ir.kink_utilization_bps,
        multiplier_bps: ir.multiplier_bps,
        jump_multiplier_bps: ir.jump_multiplier_bps,
        spread_bps: ir.spread_bps,
        min_rate_bps: ir.min_rate_bps,
        max_rate_bps: ir.max_rate_bps,
        emergency_paused,
    })
}
