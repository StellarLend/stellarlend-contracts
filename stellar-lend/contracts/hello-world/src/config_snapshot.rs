//! # Configuration Snapshot Module
//!
//! Provides a read-only view of key protocol configuration parameters
//! for observability, monitoring, and off-chain tooling.
//!
//! ## Response Schema
//!
//! `ConfigSnapshot` contains:
//! - `min_collateral_ratio` — minimum collateral ratio in basis points (e.g. 11000 = 110%)
//! - `liquidation_threshold` — liquidation threshold in basis points (e.g. 10500 = 105%)
//! - `close_factor` — max liquidatable debt per tx in basis points (e.g. 5000 = 50%)
//! - `liquidation_incentive` — liquidator bonus in basis points (e.g. 1000 = 10%)
//! - `emergency_paused` — whether the global emergency pause is active
//! - `base_borrow_rate` — current base borrow rate in basis points
//! - `snapshot_time` — ledger timestamp when snapshot was taken
//!
//! ## Security
//! - All fields are read-only; no state is modified
//! - No sensitive data (admin address, user positions) is included
//! - Safe to call by any address without authorization

use crate::prelude::*;
use crate::risk_management::RiskDataKey;
use crate::risk_params::{RiskParams, RiskParamsDataKey};
use soroban_sdk::{contracttype, Env};

/// Represents a point-in-time snapshot of the protocol's configuration.
///
/// Contains key risk parameters and global state variables required by off-chain
/// tooling, liquidators, and monitoring services.
///
/// # Security
/// - All fields are read-only views of the contract's persistent storage.
/// - No state mutations occur during the creation of this snapshot.
/// - Does not expose sensitive user data or admin-only configuration details.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigSnapshot {
    pub min_collateral_ratio: i128,
    pub liquidation_threshold: i128,
    pub close_factor: i128,
    pub liquidation_incentive: i128,
    pub emergency_paused: bool,
    pub base_borrow_rate: i128,
    pub snapshot_time: u64,
}

/// Retrieves the current configuration snapshot of the protocol.
///
/// Reads persistent storage to assemble a `ConfigSnapshot` containing risk
/// parameters, emergency pause status, and the current base borrow rate.
///
/// # Returns
/// - `Some(ConfigSnapshot)` if the protocol has been initialized and risk parameters exist.
/// - `None` if the protocol is uninitialized.
///
/// # Security
/// - **Authorization:** None required. Safe to call by any address.
/// - **Reentrancy:** Safe. This function is strictly read-only and makes no external cross-contract calls that could trigger reentrancy.
/// - **State Mutation:** Guaranteed read-only. Uses only `get` operations on storage.
pub fn get_config_snapshot(env: &Env) -> Option<ConfigSnapshot> {
    let risk_params = env
        .storage()
        .persistent()
        .get::<RiskParamsDataKey, RiskParams>(&RiskParamsDataKey::RiskParamsConfig)?;

    let emergency_paused = env
        .storage()
        .persistent()
        .get::<RiskDataKey, bool>(&RiskDataKey::EmergencyPause)
        .unwrap_or(false);

    let base_borrow_rate = crate::interest_rate::calculate_borrow_rate(env).unwrap_or(0);

    Some(ConfigSnapshot {
        min_collateral_ratio: risk_params.min_collateral_ratio,
        liquidation_threshold: risk_params.liquidation_threshold,
        close_factor: risk_params.close_factor,
        liquidation_incentive: risk_params.liquidation_incentive,
        emergency_paused,
        base_borrow_rate,
        snapshot_time: env.ledger().timestamp(),
    })
}
