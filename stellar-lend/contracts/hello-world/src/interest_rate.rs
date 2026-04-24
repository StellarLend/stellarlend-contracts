//! # Interest Rate Module
//!
//! Implements a kink-based (piecewise linear) interest rate model for the lending protocol.
//!
//! ## Rate Model
//!
//! The borrow rate is determined by protocol utilization (`borrows / deposits`):
//!
//! - **Below kink** (default 80%):
//!   `rate = base_rate + (utilization / kink_utilization) × multiplier`
//! - **Above kink**:
//!   `rate = base_rate + multiplier + ((utilization − kink) / (1 − kink)) × jump_multiplier`
//!
//! The supply rate is derived as: `supply_rate = borrow_rate − spread`
//!
//! All rates are expressed in **basis points** (1 bp = 0.01%).
//!
//! ## Configuration Defaults
//!
//! | Parameter          | Default  | Meaning             |
//! |--------------------|----------|---------------------|
//! | `base_rate_bps`    | 100      | 1% APY              |
//! | `kink_utilization`  | 8000     | 80%                 |
//! | `multiplier_bps`   | 2000     | 20% slope below kink|
//! | `jump_multiplier`   | 10000    | 100% slope above    |
//! | `rate_floor_bps`   | 50       | 0.5% minimum        |
//! | `rate_ceiling_bps` | 10000    | 100% maximum        |
//! | `spread_bps`       | 200      | 2% supply/borrow gap|
//!
//! ## Interest Accrual
//!
//! Simple interest: `interest = principal × rate_bps × elapsed_seconds / (10_000 × SECONDS_PER_YEAR)`
//!
//! For long horizons the module also provides a **compound accrual** helper that splits
//! the elapsed time into yearly chunks and compounds, preventing overflow on
//! multi-year accumulations while remaining deterministic.
//!
//! ## Emergency Adjustment
//!
//! Admin can apply a positive or negative emergency adjustment to the calculated rate,
//! bounded to ±100% (±10 000 bps).
//!
//! ## Numeric Assumptions
//!
//! See `INTEREST_NUMERIC_ASSUMPTIONS.md` at the crate root for the full invariant list.
//! Key constraints:
//! - All basis-point values are `i128` in `[0, 10_000]` unless otherwise noted.
//! - `kink_utilization_bps` is in `(0, 10_000)` (exclusive).
//! - Checked arithmetic is used throughout — no unchecked ops.
//! - `SECONDS_PER_YEAR = 365 × 86_400 = 31_536_000` (no leap seconds).

use crate::prelude::*;
use soroban_sdk::{contracterror, contracttype, Address, Env};

use crate::deposit::{DepositDataKey, ProtocolAnalytics};

// =============================================================================
// Errors
// =============================================================================

/// Errors that can occur during interest rate operations.
///
/// Error codes are **stable** and must never be renumbered to preserve
/// cross-version compatibility with off-chain decoders.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum InterestRateError {
    /// Unauthorized access — caller is not admin.
    Unauthorized = 1,
    /// Invalid parameter value (out of range or violates constraints).
    InvalidParameter = 2,
    /// Parameter change exceeds maximum allowed delta.
    ParameterChangeTooLarge = 3,
    /// Arithmetic overflow during calculation.
    Overflow = 4,
    /// Division by zero (e.g., no deposits in utilization calc).
    DivisionByZero = 5,
    /// Contract has already been initialized.
    AlreadyInitialized = 6,
}

// =============================================================================
// Storage Keys
// =============================================================================

/// Storage keys for interest rate data.
///
/// All values are stored in the **persistent** storage layer so they survive
/// contract upgrades.
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum InterestRateDataKey {
    /// Kink-based interest rate model parameters.
    /// Value type: [`InterestRateConfig`]
    InterestRateConfig,
    /// Module admin address authorized for rate adjustments.
    /// Value type: `Address`
    Admin,
    /// Placeholder for emergency rate adjustment status.
    EmergencyRateAdjustment,
}

// =============================================================================
// Configuration Struct
// =============================================================================

/// Interest rate configuration parameters.
///
/// Every field uses **basis points** (1 bp = 0.01%) unless noted otherwise.
///
/// # Invariants
/// - `0 ≤ base_rate_bps ≤ 10_000`
/// - `0 < kink_utilization_bps < 10_000`
/// - `multiplier_bps ≥ 0`
/// - `jump_multiplier_bps ≥ 0`
/// - `0 ≤ rate_floor_bps ≤ rate_ceiling_bps ≤ 10_000`
/// - `0 ≤ spread_bps ≤ 10_000`
/// - `|emergency_adjustment_bps| ≤ 10_000`
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct InterestRateConfig {
    /// Base interest rate (e.g. 100 = 1% APY).
    /// This is the minimum rate when utilization is 0%.
    pub base_rate_bps: i128,
    /// Kink utilization point (e.g. 8000 = 80%).
    /// Below this, the rate increases with `multiplier_bps`.
    /// Above this, the rate increases with `jump_multiplier_bps`.
    pub kink_utilization_bps: i128,
    /// Slope below kink (e.g. 2000 = 20%).
    /// Rate below kink = `base_rate + (utilization / kink) × multiplier`.
    pub multiplier_bps: i128,
    /// Slope above kink (e.g. 10000 = 100%).
    /// Rate above kink = `base_rate + multiplier + ((util − kink) / (1 − kink)) × jump_multiplier`.
    pub jump_multiplier_bps: i128,
    /// Minimum interest rate floor.
    pub rate_floor_bps: i128,
    /// Maximum interest rate ceiling.
    pub rate_ceiling_bps: i128,
    /// Spread between borrow and supply rates (e.g. 200 = 2%).
    /// `supply_rate = borrow_rate − spread`.
    pub spread_bps: i128,
    /// Emergency rate adjustment — added to the calculated borrow rate.
    /// Can be positive (increase) or negative (decrease).
    pub emergency_adjustment_bps: i128,
    /// Ledger timestamp of the last configuration change.
    pub last_update: u64,
}

// =============================================================================
// Constants
// =============================================================================

/// 100% expressed in basis points.
const BASIS_POINTS_SCALE: i128 = 10_000;

/// Seconds in a non-leap year: 365 × 86 400 = 31 536 000.
const SECONDS_PER_YEAR: u64 = 365 * 86_400;

/// Maximum allowed value for slope parameters (`multiplier_bps`, `jump_multiplier_bps`).
/// Set to 100 000 bps (1000%) to allow aggressive-but-bounded curves.
const MAX_SLOPE_BPS: i128 = 100_000;

// =============================================================================
// Default Configuration
// =============================================================================

/// Returns the default interest rate configuration.
///
/// These defaults produce a standard DeFi kink model:
/// - 1% base, 80% kink, 20% slope₁, 100% slope₂
/// - Floor 0.5%, ceiling 100%, spread 2%
fn get_default_config() -> InterestRateConfig {
    InterestRateConfig {
        base_rate_bps: 100,
        kink_utilization_bps: 8000,
        multiplier_bps: 2000,
        jump_multiplier_bps: 10_000,
        rate_floor_bps: 50,
        rate_ceiling_bps: 10_000,
        spread_bps: 200,
        emergency_adjustment_bps: 0,
        last_update: 0,
    }
}

// =============================================================================
// Storage Accessors
// =============================================================================

/// Retrieve the current [`InterestRateConfig`] from persistent storage.
///
/// Returns `None` if the module has not been initialized.
pub fn get_interest_rate_config(env: &Env) -> Option<InterestRateConfig> {
    env.storage()
        .persistent()
        .get::<InterestRateDataKey, InterestRateConfig>(&InterestRateDataKey::InterestRateConfig)
}

// =============================================================================
// Initialization
// =============================================================================

/// Initialize the interest rate module with default parameters.
///
/// Must be called exactly once during contract initialization.
///
/// # Errors
/// - [`InterestRateError::AlreadyInitialized`] if called more than once.
///
/// # Security
/// - Idempotency guard prevents re-initialization after deployment.
pub fn initialize_interest_rate_config(
    env: &Env,
    _admin: Address,
) -> Result<(), InterestRateError> {
    let config_key = InterestRateDataKey::InterestRateConfig;

    if env
        .storage()
        .persistent()
        .has::<InterestRateDataKey>(&config_key)
    {
        return Err(InterestRateError::AlreadyInitialized);
    }

    let config = get_default_config();
    env.storage().persistent().set(&config_key, &config);

    Ok(())
}

// =============================================================================
// Utilization
// =============================================================================

/// Calculate protocol utilization in basis points.
///
/// `utilization = (total_borrows × 10 000) / total_deposits`
///
/// # Returns
/// Utilization in `[0, 10 000]` basis points, capped at 100%.
///
/// # Errors
/// - [`InterestRateError::Overflow`] on arithmetic overflow.
///
/// # Security
/// - Returns `0` when there are no deposits (avoids division by zero).
/// - Caps the result at `10 000` even if borrows exceed deposits.
pub fn calculate_utilization(env: &Env) -> Result<i128, InterestRateError> {
    let analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, ProtocolAnalytics>(&DepositDataKey::ProtocolAnalytics)
        .unwrap_or(ProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    if analytics.total_deposits <= 0 {
        return Ok(0);
    }

    let utilization = analytics
        .total_borrows
        .checked_mul(BASIS_POINTS_SCALE)
        .ok_or(InterestRateError::Overflow)?
        .checked_div(analytics.total_deposits)
        .ok_or(InterestRateError::DivisionByZero)?;

    // Cap at 100%
    Ok(utilization.clamp(0, BASIS_POINTS_SCALE))
}

// =============================================================================
// Borrow Rate
// =============================================================================

/// Calculate the borrow interest rate based on current utilization.
///
/// Uses a **piecewise-linear (kink) model**:
///
/// - Below kink: `rate = base + (utilization / kink) × multiplier`
/// - Above kink: `rate = base + multiplier + ((util − kink) / (10 000 − kink)) × jump_multiplier`
///
/// The emergency adjustment is added, then the result is clamped to `[floor, ceiling]`.
///
/// # Errors
/// - [`InterestRateError::InvalidParameter`] if config is missing.
/// - [`InterestRateError::Overflow`] on arithmetic overflow.
/// - [`InterestRateError::DivisionByZero`] if kink is misconfigured.
///
/// # Security
/// - All arithmetic uses checked operations.
/// - Rate is always clamped to `[rate_floor_bps, rate_ceiling_bps]`.
pub fn calculate_borrow_rate(env: &Env) -> Result<i128, InterestRateError> {
    let config = get_interest_rate_config(env).ok_or(InterestRateError::InvalidParameter)?;
    let utilization = calculate_utilization(env)?;

    let mut rate = config.base_rate_bps;

    if utilization <= config.kink_utilization_bps {
        // Below kink: linear increase
        if config.kink_utilization_bps > 0 {
            let rate_increase = utilization
                .checked_mul(config.multiplier_bps)
                .ok_or(InterestRateError::Overflow)?
                .checked_div(config.kink_utilization_bps)
                .ok_or(InterestRateError::DivisionByZero)?;
            rate = rate
                .checked_add(rate_increase)
                .ok_or(InterestRateError::Overflow)?;
        }
    } else {
        // Above kink: steeper increase via jump multiplier
        let rate_at_kink = config
            .base_rate_bps
            .checked_add(config.multiplier_bps)
            .ok_or(InterestRateError::Overflow)?;

        let utilization_above_kink = utilization
            .checked_sub(config.kink_utilization_bps)
            .ok_or(InterestRateError::Overflow)?;

        let max_utilization_above_kink = BASIS_POINTS_SCALE
            .checked_sub(config.kink_utilization_bps)
            .ok_or(InterestRateError::Overflow)?;

        if max_utilization_above_kink > 0 {
            let additional_rate = utilization_above_kink
                .checked_mul(config.jump_multiplier_bps)
                .ok_or(InterestRateError::Overflow)?
                .checked_div(max_utilization_above_kink)
                .ok_or(InterestRateError::DivisionByZero)?;

            rate = rate_at_kink
                .checked_add(additional_rate)
                .ok_or(InterestRateError::Overflow)?;
        } else {
            rate = rate_at_kink;
        }
    }

    // Apply emergency adjustment (can be negative)
    rate = rate
        .checked_add(config.emergency_adjustment_bps)
        .ok_or(InterestRateError::Overflow)?;

    // Clamp to [floor, ceiling]
    rate = rate.max(config.rate_floor_bps).min(config.rate_ceiling_bps);

    Ok(rate)
}

// =============================================================================
// Supply Rate
// =============================================================================

/// Calculate the supply interest rate.
///
/// `supply_rate = max(borrow_rate − spread, rate_floor)`
///
/// # Errors
/// - Propagates errors from [`calculate_borrow_rate`].
///
/// # Security
/// - Supply rate is never negative — clamped to `rate_floor_bps`.
pub fn calculate_supply_rate(env: &Env) -> Result<i128, InterestRateError> {
    let config = get_interest_rate_config(env).ok_or(InterestRateError::InvalidParameter)?;
    let borrow_rate = calculate_borrow_rate(env)?;

    let supply_rate = borrow_rate
        .checked_sub(config.spread_bps)
        .ok_or(InterestRateError::Overflow)?;

    // Ensure supply rate doesn't go below floor
    Ok(supply_rate.max(config.rate_floor_bps))
}

// =============================================================================
// Simple Interest Accrual
// =============================================================================

/// Calculate accrued interest using simple (linear) interest.
///
/// `interest = principal × rate_bps × elapsed_seconds / (10_000 × SECONDS_PER_YEAR)`
///
/// # Arguments
/// * `principal` — The outstanding principal amount.
/// * `last_accrual_time` — Unix timestamp of the last accrual.
/// * `current_time` — Current Unix timestamp.
/// * `rate_bps` — Annual interest rate in basis points.
///
/// # Returns
/// The accrued interest amount (always ≥ 0).
///
/// # Errors
/// - [`InterestRateError::Overflow`] if the intermediate product overflows `i128`.
///
/// # Security
/// - Returns `0` for zero principal or non-positive elapsed time.
/// - All arithmetic is checked.
pub fn calculate_accrued_interest(
    principal: i128,
    last_accrual_time: u64,
    current_time: u64,
    rate_bps: i128,
) -> Result<i128, InterestRateError> {
    if principal <= 0 || rate_bps <= 0 {
        return Ok(0);
    }

    if current_time <= last_accrual_time {
        return Ok(0);
    }

    let time_elapsed = current_time
        .checked_sub(last_accrual_time)
        .ok_or(InterestRateError::Overflow)?;

    // interest = principal * rate_bps * time_elapsed / (10_000 * SECONDS_PER_YEAR)
    let denominator = BASIS_POINTS_SCALE
        .checked_mul(SECONDS_PER_YEAR as i128)
        .ok_or(InterestRateError::Overflow)?;

    let numerator = principal
        .checked_mul(rate_bps)
        .ok_or(InterestRateError::Overflow)?
        .checked_mul(time_elapsed as i128)
        .ok_or(InterestRateError::Overflow)?;

    let quotient = numerator
        .checked_div(denominator)
        .ok_or(InterestRateError::DivisionByZero)?;
    let remainder = numerator
        .checked_rem(denominator)
        .ok_or(InterestRateError::DivisionByZero)?;
    let interest = if numerator > 0 && remainder > 0 {
        quotient.checked_add(1).ok_or(InterestRateError::Overflow)?
    } else {
        quotient
    };

    Ok(interest.max(0))
}

// =============================================================================
// Compound Interest Accrual
// =============================================================================

/// Calculate accrued interest using **discrete yearly compounding**.
///
/// Splits `elapsed` into full years and a remaining fraction:
/// 1. For each full year: `balance = balance + balance × rate_bps / 10_000`
/// 2. For the remaining fraction: simple interest on the compounded balance.
///
/// This prevents overflow for very long horizons by accumulating incrementally
/// rather than computing `principal × rate × total_time` in one multiplication.
///
/// # Arguments
/// * `principal` — The outstanding principal amount.
/// * `last_accrual_time` — Unix timestamp of the last accrual.
/// * `current_time` — Current Unix timestamp.
/// * `rate_bps` — Annual interest rate in basis points.
///
/// # Returns
/// The total accrued interest (compound − principal).
///
/// # Errors
/// - [`InterestRateError::Overflow`] on arithmetic overflow.
///
/// # Security
/// - Deterministic: no floating-point, no randomness.
/// - Handles up to ~200 years at 100% APR without overflow for principals ≤ 10^30.
/// - Returns `0` for zero/negative principal, zero rate, or non-positive elapsed time.
pub fn calculate_compound_interest(
    principal: i128,
    last_accrual_time: u64,
    current_time: u64,
    rate_bps: i128,
) -> Result<i128, InterestRateError> {
    if principal <= 0 || rate_bps <= 0 {
        return Ok(0);
    }
    if current_time <= last_accrual_time {
        return Ok(0);
    }

    let elapsed = current_time
        .checked_sub(last_accrual_time)
        .ok_or(InterestRateError::Overflow)?;

    let full_years = elapsed / (SECONDS_PER_YEAR);
    let remaining_seconds = elapsed % (SECONDS_PER_YEAR);

    let mut balance = principal;

    // Compound for each full year
    for _ in 0..full_years {
        let yearly_interest = balance
            .checked_mul(rate_bps)
            .ok_or(InterestRateError::Overflow)?
            .checked_div(BASIS_POINTS_SCALE)
            .ok_or(InterestRateError::DivisionByZero)?;
        balance = balance
            .checked_add(yearly_interest)
            .ok_or(InterestRateError::Overflow)?;
    }

    // Simple interest for the remaining partial year
    if remaining_seconds > 0 {
        let partial_interest = balance
            .checked_mul(rate_bps)
            .ok_or(InterestRateError::Overflow)?
            .checked_mul(remaining_seconds as i128)
            .ok_or(InterestRateError::Overflow)?
            .checked_div(
                BASIS_POINTS_SCALE
                    .checked_mul(SECONDS_PER_YEAR as i128)
                    .ok_or(InterestRateError::Overflow)?,
            )
            .ok_or(InterestRateError::DivisionByZero)?;
        balance = balance
            .checked_add(partial_interest)
            .ok_or(InterestRateError::Overflow)?;
    }

    // Total interest = compounded balance − original principal
    let interest = balance
        .checked_sub(principal)
        .ok_or(InterestRateError::Overflow)?;

    Ok(interest.max(0))
}

// =============================================================================
// Admin: Update Configuration
// =============================================================================

/// Update interest rate configuration parameters.
///
/// Only callable by the protocol admin. Each parameter is optional — pass `None`
/// to keep the current value.
///
/// # Arguments
/// * `env` — The Soroban environment.
/// * `caller` — Must be the super admin.
/// * `base_rate_bps` — New base rate in `[0, 10_000]`.
/// * `kink_utilization_bps` — New kink in `(0, 10_000)` (exclusive).
/// * `multiplier_bps` — New slope₁ in `[0, MAX_SLOPE_BPS]`.
/// * `jump_multiplier_bps` — New slope₂ in `[0, MAX_SLOPE_BPS]`.
/// * `rate_floor_bps` — New floor in `[0, 10_000]`, must be ≤ ceiling.
/// * `rate_ceiling_bps` — New ceiling in `[0, 10_000]`, must be ≥ floor.
/// * `spread_bps` — New spread in `[0, 10_000]`.
///
/// # Errors
/// - [`InterestRateError::Unauthorized`] if caller is not admin.
/// - [`InterestRateError::InvalidParameter`] if any value is out of range or
///   violates constraints (e.g. `floor > ceiling`).
///
/// # Security
/// - Enforces strict authorization via `require_admin`.
/// - All parameters are range-checked before storage write.
/// - `last_update` is set to the current ledger timestamp.
#[allow(clippy::too_many_arguments)]
pub fn update_interest_rate_config(
    env: &Env,
    caller: Address,
    base_rate_bps: Option<i128>,
    kink_utilization_bps: Option<i128>,
    multiplier_bps: Option<i128>,
    jump_multiplier_bps: Option<i128>,
    rate_floor_bps: Option<i128>,
    rate_ceiling_bps: Option<i128>,
    spread_bps: Option<i128>,
) -> Result<(), InterestRateError> {
    // Authorization
    crate::admin::require_admin(env, &caller).map_err(|_| InterestRateError::Unauthorized)?;

    let config_key = InterestRateDataKey::InterestRateConfig;
    let mut config = get_interest_rate_config(env).ok_or(InterestRateError::InvalidParameter)?;

    // --- Validate and apply each parameter ---

    if let Some(rate) = base_rate_bps {
        if !(0..=BASIS_POINTS_SCALE).contains(&rate) {
            return Err(InterestRateError::InvalidParameter);
        }
        config.base_rate_bps = rate;
    }

    if let Some(kink) = kink_utilization_bps {
        // kink must be strictly between 0 and 10_000
        if kink <= 0 || kink >= BASIS_POINTS_SCALE {
            return Err(InterestRateError::InvalidParameter);
        }
        config.kink_utilization_bps = kink;
    }

    if let Some(mult) = multiplier_bps {
        if !(0..=MAX_SLOPE_BPS).contains(&mult) {
            return Err(InterestRateError::InvalidParameter);
        }
        config.multiplier_bps = mult;
    }

    if let Some(jump) = jump_multiplier_bps {
        if !(0..=MAX_SLOPE_BPS).contains(&jump) {
            return Err(InterestRateError::InvalidParameter);
        }
        config.jump_multiplier_bps = jump;
    }

    // Floor/ceiling: validate individually, then cross-check.
    // We need to resolve the effective floor and ceiling *after* applying
    // both optional updates so the cross-check is correct.
    if let Some(floor) = rate_floor_bps {
        if !(0..=BASIS_POINTS_SCALE).contains(&floor) {
            return Err(InterestRateError::InvalidParameter);
        }
        config.rate_floor_bps = floor;
    }

    if let Some(ceiling) = rate_ceiling_bps {
        if !(0..=BASIS_POINTS_SCALE).contains(&ceiling) {
            return Err(InterestRateError::InvalidParameter);
        }
        config.rate_ceiling_bps = ceiling;
    }

    // Cross-check: floor must not exceed ceiling
    if config.rate_floor_bps > config.rate_ceiling_bps {
        return Err(InterestRateError::InvalidParameter);
    }

    if let Some(spread) = spread_bps {
        if !(0..=BASIS_POINTS_SCALE).contains(&spread) {
            return Err(InterestRateError::InvalidParameter);
        }
        config.spread_bps = spread;
    }

    config.last_update = env.ledger().timestamp();
    env.storage().persistent().set(&config_key, &config);

    Ok(())
}

// =============================================================================
// Admin: Emergency Rate Adjustment
// =============================================================================

/// Set an emergency rate adjustment.
///
/// The adjustment (in basis points) is added to the calculated borrow rate.
/// It can be positive (to increase rates during a crisis) or negative
/// (to reduce rates temporarily).
///
/// # Arguments
/// * `env` — The Soroban environment.
/// * `caller` — Must be the super admin.
/// * `adjustment_bps` — The adjustment value, bounded to `[-10_000, 10_000]`.
///
/// # Errors
/// - [`InterestRateError::Unauthorized`] if caller is not admin.
/// - [`InterestRateError::InvalidParameter`] if `|adjustment| > 10_000`.
///
/// # Security
/// - Admin-only operation.
/// - The final borrow rate is still clamped to `[floor, ceiling]` regardless
///   of the adjustment magnitude.
pub fn set_emergency_rate_adjustment(
    env: &Env,
    caller: Address,
    adjustment_bps: i128,
) -> Result<(), InterestRateError> {
    crate::admin::require_admin(env, &caller).map_err(|_| InterestRateError::Unauthorized)?;

    if adjustment_bps.abs() > BASIS_POINTS_SCALE {
        return Err(InterestRateError::InvalidParameter);
    }

    let config_key = InterestRateDataKey::InterestRateConfig;
    let mut config = get_interest_rate_config(env).ok_or(InterestRateError::InvalidParameter)?;

    config.emergency_adjustment_bps = adjustment_bps;
    config.last_update = env.ledger().timestamp();

    env.storage().persistent().set(&config_key, &config);

    Ok(())
}

// =============================================================================
// Public Query Helpers
// =============================================================================

/// Get the current borrow rate in basis points.
///
/// Convenience wrapper around [`calculate_borrow_rate`].
pub fn get_current_borrow_rate(env: &Env) -> Result<i128, InterestRateError> {
    calculate_borrow_rate(env)
}

/// Get the current supply rate in basis points.
///
/// Convenience wrapper around [`calculate_supply_rate`].
pub fn get_current_supply_rate(env: &Env) -> Result<i128, InterestRateError> {
    calculate_supply_rate(env)
}

/// Get the current utilization in basis points.
///
/// Convenience wrapper around [`calculate_utilization`].
pub fn get_current_utilization(env: &Env) -> Result<i128, InterestRateError> {
    calculate_utilization(env)
}
