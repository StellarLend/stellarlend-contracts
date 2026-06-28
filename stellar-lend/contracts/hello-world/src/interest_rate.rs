//! Interest-rate model for the hello-world lending contract.
//!
//! The module stores an admin-managed jump-rate configuration and exposes
//! deterministic helpers for utilization, borrow-rate, and supply-rate
//! calculation.  All externally visible rates are expressed in basis points
//! (bps), where `10_000` is 100%.

use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env};

/// Number of basis points representing 100%.
pub const BASIS_POINTS_SCALE: i128 = 10_000;

/// Maximum slope accepted for multiplier parameters (1,000%).
pub const MAX_SLOPE_BPS: i128 = 100_000;

/// Default ceiling that preserves the old effectively unbounded behaviour.
pub const DEFAULT_MAX_RATE_BPS: i128 = i128::MAX;

/// Storage keys used by the interest-rate module.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterestRateDataKey {
    /// Address authorized to mutate the interest-rate configuration.
    Admin,
    /// Persisted [`InterestRateConfig`].
    InterestRateConfig,
    /// Protocol-wide supplied amount used for utilization.
    TotalDeposits,
    /// Protocol-wide borrowed amount used for utilization.
    TotalBorrows,
    /// Additive emergency adjustment in bps.
    EmergencyRateAdjustment,
}

/// Errors returned by interest-rate configuration and math paths.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum InterestRateError {
    /// Interest-rate module was already initialized.
    AlreadyInitialized = 1,
    /// Caller is not the stored interest-rate admin.
    Unauthorized = 2,
    /// A provided parameter is outside its allowed range.
    InvalidParameter = 3,
    /// Checked arithmetic overflowed.
    Overflow = 4,
    /// Division by zero was prevented.
    DivisionByZero = 5,
    /// Interest-rate configuration has not been initialized.
    NotInitialized = 6,
}

/// Admin-configurable jump-rate model parameters.
///
/// Rates are calculated in bps.  `min_rate_bps` and `max_rate_bps` are applied
/// as the final borrow-rate clamp after utilization math and emergency
/// adjustment:
///
/// `effective_borrow_rate = clamp(raw_rate + emergency_adjustment, min_rate_bps, max_rate_bps)`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterestRateConfig {
    /// Base borrow APR at 0 utilization, in bps.
    pub base_rate_bps: i128,
    /// Utilization kink, in bps, exclusive range `(0, 10_000)`.
    pub kink_utilization_bps: i128,
    /// Total pre-kink increase added by the time utilization reaches the kink.
    pub multiplier_bps: i128,
    /// Total post-kink increase added between kink and 100% utilization.
    pub jump_multiplier_bps: i128,
    /// Legacy floor used by older deployments; retained for storage/API compatibility.
    pub rate_floor_bps: i128,
    /// Legacy ceiling used by older deployments; retained for storage/API compatibility.
    pub rate_ceiling_bps: i128,
    /// Spread subtracted from the clamped borrow rate to derive supply rate.
    pub spread_bps: i128,
    /// Hard minimum effective borrow rate, applied last.
    pub min_rate_bps: i128,
    /// Hard maximum effective borrow rate, applied last.
    pub max_rate_bps: i128,
}

impl Default for InterestRateConfig {
    fn default() -> Self {
        Self {
            base_rate_bps: 100,
            kink_utilization_bps: 8_000,
            multiplier_bps: 2_000,
            jump_multiplier_bps: 10_000,
            rate_floor_bps: 0,
            rate_ceiling_bps: DEFAULT_MAX_RATE_BPS,
            spread_bps: 0,
            min_rate_bps: 0,
            max_rate_bps: DEFAULT_MAX_RATE_BPS,
        }
    }
}

/// Initialize the module with default rate parameters and an admin.
///
/// Defaults intentionally preserve previous unclamped behaviour: floor `0` and
/// ceiling `i128::MAX`, until an admin explicitly configures a narrower band.
pub fn initialize_interest_rate_config(env: &Env, admin: Address) -> Result<(), InterestRateError> {
    if env
        .storage()
        .persistent()
        .has(&InterestRateDataKey::InterestRateConfig)
    {
        return Err(InterestRateError::AlreadyInitialized);
    }

    env.storage()
        .persistent()
        .set(&InterestRateDataKey::Admin, &admin);
    env.storage().persistent().set(
        &InterestRateDataKey::InterestRateConfig,
        &InterestRateConfig::default(),
    );
    env.storage()
        .persistent()
        .set(&InterestRateDataKey::EmergencyRateAdjustment, &0_i128);
    Ok(())
}

/// Return the current interest-rate configuration, if initialized.
pub fn get_interest_rate_config(env: &Env) -> Option<InterestRateConfig> {
    env.storage()
        .persistent()
        .get(&InterestRateDataKey::InterestRateConfig)
}

/// Update selected interest-rate model parameters.
///
/// `rate_floor`/`rate_ceiling` update the hard clamp band for backwards
/// compatibility with the existing contract entrypoint.  They are stored in
/// both the legacy fields and the new `min_rate_bps`/`max_rate_bps` fields.
pub fn update_interest_rate_config(
    env: &Env,
    admin: Address,
    base_rate: Option<i128>,
    kink: Option<i128>,
    multiplier: Option<i128>,
    jump_multiplier: Option<i128>,
    rate_floor: Option<i128>,
    rate_ceiling: Option<i128>,
    spread: Option<i128>,
) -> Result<(), InterestRateError> {
    require_rate_admin(env, &admin)?;
    let mut config = get_interest_rate_config(env).ok_or(InterestRateError::NotInitialized)?;

    if let Some(v) = base_rate {
        config.base_rate_bps = v;
    }
    if let Some(v) = kink {
        config.kink_utilization_bps = v;
    }
    if let Some(v) = multiplier {
        config.multiplier_bps = v;
    }
    if let Some(v) = jump_multiplier {
        config.jump_multiplier_bps = v;
    }
    if let Some(v) = rate_floor {
        config.rate_floor_bps = v;
        config.min_rate_bps = v;
    }
    if let Some(v) = rate_ceiling {
        config.rate_ceiling_bps = v;
        config.max_rate_bps = v;
    }
    if let Some(v) = spread {
        config.spread_bps = v;
    }

    validate_config(&config)?;
    env.storage()
        .persistent()
        .set(&InterestRateDataKey::InterestRateConfig, &config);
    Ok(())
}

/// Set the protocol totals used by utilization calculation.
///
/// This small helper is useful for tests and for integration paths that update
/// accounting in a module separate from the rate model.
pub fn set_protocol_totals(
    env: &Env,
    total_deposits: i128,
    total_borrows: i128,
) -> Result<(), InterestRateError> {
    if total_deposits < 0 || total_borrows < 0 {
        return Err(InterestRateError::InvalidParameter);
    }
    env.storage()
        .persistent()
        .set(&InterestRateDataKey::TotalDeposits, &total_deposits);
    env.storage()
        .persistent()
        .set(&InterestRateDataKey::TotalBorrows, &total_borrows);
    Ok(())
}

/// Set an emergency additive adjustment, in bps.
///
/// The adjustment is applied before the final `[min_rate_bps, max_rate_bps]`
/// clamp and is bounded to ±100% APR.
pub fn set_emergency_rate_adjustment(
    env: &Env,
    admin: Address,
    adjustment_bps: i128,
) -> Result<(), InterestRateError> {
    require_rate_admin(env, &admin)?;
    if !(-BASIS_POINTS_SCALE..=BASIS_POINTS_SCALE).contains(&adjustment_bps) {
        return Err(InterestRateError::InvalidParameter);
    }
    env.storage().persistent().set(
        &InterestRateDataKey::EmergencyRateAdjustment,
        &adjustment_bps,
    );
    Ok(())
}

/// Calculate utilization in bps from stored protocol totals.
///
/// Formula: `utilization = total_borrows * 10_000 / total_deposits`.
/// If deposits are zero, utilization returns zero.  The value is capped at
/// `10_000` to keep downstream rate math in the expected range.
pub fn calculate_utilization(env: &Env) -> Result<i128, InterestRateError> {
    let total_deposits = env
        .storage()
        .persistent()
        .get::<InterestRateDataKey, i128>(&InterestRateDataKey::TotalDeposits)
        .unwrap_or(0);
    let total_borrows = env
        .storage()
        .persistent()
        .get::<InterestRateDataKey, i128>(&InterestRateDataKey::TotalBorrows)
        .unwrap_or(0);

    if total_deposits <= 0 {
        return Ok(0);
    }
    if total_borrows <= 0 {
        return Ok(0);
    }

    let util = total_borrows
        .checked_mul(BASIS_POINTS_SCALE)
        .ok_or(InterestRateError::Overflow)?
        .checked_div(total_deposits)
        .ok_or(InterestRateError::DivisionByZero)?;
    Ok(clamp_rate(util, 0, BASIS_POINTS_SCALE))
}

/// Calculate the effective borrow rate in bps.
///
/// The final step always clamps the rate to
/// `[InterestRateConfig::min_rate_bps, InterestRateConfig::max_rate_bps]`, so
/// degenerate curve outputs and emergency adjustments cannot escape the
/// configured band.
pub fn calculate_borrow_rate(env: &Env) -> Result<i128, InterestRateError> {
    let utilization_bps = calculate_utilization(env)?;
    let config = get_interest_rate_config(env).unwrap_or_default();
    let emergency_adjustment = env
        .storage()
        .persistent()
        .get::<InterestRateDataKey, i128>(&InterestRateDataKey::EmergencyRateAdjustment)
        .unwrap_or(0);

    compute_borrow_rate(utilization_bps, emergency_adjustment, &config)
}

/// Calculate the supply rate in bps from the clamped borrow rate.
///
/// Supply-rate calculation intentionally calls [`calculate_borrow_rate`], so it
/// always remains consistent with the same final borrow-rate clamp seen by
/// borrowers.
pub fn calculate_supply_rate(env: &Env) -> Result<i128, InterestRateError> {
    let config = get_interest_rate_config(env).unwrap_or_default();
    let borrow_rate = calculate_borrow_rate(env)?;
    let supply_rate = borrow_rate
        .checked_sub(config.spread_bps)
        .ok_or(InterestRateError::Overflow)?;
    Ok(supply_rate.max(0).max(config.min_rate_bps))
}

/// Pure borrow-rate calculation for a supplied utilization and config.
///
/// Formula:
///
/// - Below kink: `base + utilization * multiplier / kink`
/// - Above kink: `base + multiplier + (utilization - kink) * jump / (10_000 - kink)`
/// - Effective: `clamp(raw + emergency_adjustment, min_rate_bps, max_rate_bps)`
pub fn compute_borrow_rate(
    utilization_bps: i128,
    emergency_adjustment_bps: i128,
    config: &InterestRateConfig,
) -> Result<i128, InterestRateError> {
    validate_config(config)?;
    let utilization = clamp_rate(utilization_bps, 0, BASIS_POINTS_SCALE);

    let raw_rate = if utilization <= config.kink_utilization_bps {
        config
            .base_rate_bps
            .checked_add(
                utilization
                    .checked_mul(config.multiplier_bps)
                    .ok_or(InterestRateError::Overflow)?
                    .checked_div(config.kink_utilization_bps)
                    .ok_or(InterestRateError::DivisionByZero)?,
            )
            .ok_or(InterestRateError::Overflow)?
    } else {
        let post_kink_denominator = BASIS_POINTS_SCALE
            .checked_sub(config.kink_utilization_bps)
            .ok_or(InterestRateError::DivisionByZero)?;
        config
            .base_rate_bps
            .checked_add(config.multiplier_bps)
            .ok_or(InterestRateError::Overflow)?
            .checked_add(
                utilization
                    .checked_sub(config.kink_utilization_bps)
                    .ok_or(InterestRateError::Overflow)?
                    .checked_mul(config.jump_multiplier_bps)
                    .ok_or(InterestRateError::Overflow)?
                    .checked_div(post_kink_denominator)
                    .ok_or(InterestRateError::DivisionByZero)?,
            )
            .ok_or(InterestRateError::Overflow)?
    };

    let adjusted_rate = raw_rate
        .checked_add(emergency_adjustment_bps)
        .ok_or(InterestRateError::Overflow)?;

    Ok(clamp_rate(
        adjusted_rate,
        config.min_rate_bps,
        config.max_rate_bps,
    ))
}

/// Clamp `rate_bps` to the inclusive configured band.
pub fn clamp_rate(rate_bps: i128, min_rate_bps: i128, max_rate_bps: i128) -> i128 {
    rate_bps.max(min_rate_bps).min(max_rate_bps)
}

fn require_rate_admin(env: &Env, caller: &Address) -> Result<(), InterestRateError> {
    caller.require_auth();
    let admin = env
        .storage()
        .persistent()
        .get::<InterestRateDataKey, Address>(&InterestRateDataKey::Admin)
        .ok_or(InterestRateError::NotInitialized)?;
    if &admin != caller {
        return Err(InterestRateError::Unauthorized);
    }
    Ok(())
}

fn validate_config(config: &InterestRateConfig) -> Result<(), InterestRateError> {
    if config.base_rate_bps < 0 || config.base_rate_bps > BASIS_POINTS_SCALE {
        return Err(InterestRateError::InvalidParameter);
    }
    if config.kink_utilization_bps <= 0 || config.kink_utilization_bps >= BASIS_POINTS_SCALE {
        return Err(InterestRateError::InvalidParameter);
    }
    if config.multiplier_bps < 0 || config.multiplier_bps > MAX_SLOPE_BPS {
        return Err(InterestRateError::InvalidParameter);
    }
    if config.jump_multiplier_bps < 0 || config.jump_multiplier_bps > MAX_SLOPE_BPS {
        return Err(InterestRateError::InvalidParameter);
    }
    if config.spread_bps < 0 || config.spread_bps > BASIS_POINTS_SCALE {
        return Err(InterestRateError::InvalidParameter);
    }
    if config.min_rate_bps < 0 || config.max_rate_bps < config.min_rate_bps {
        return Err(InterestRateError::InvalidParameter);
    }
    Ok(())
}

/// Emit a compact rate-configuration update event.
pub fn emit_rate_config_updated(env: &Env, min_rate_bps: i128, max_rate_bps: i128) {
    env.events().publish(
        (symbol_short!("rate"), symbol_short!("clamp")),
        (min_rate_bps, max_rate_bps),
    );
}
