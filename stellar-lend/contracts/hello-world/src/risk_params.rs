//! Risk parameter module for the StellarLend lending protocol.
//!
//! Stores and validates the four core risk parameters that govern liquidation
//! behaviour.  All ratio/factor fields are expressed in basis points (bps)
//! where `10_000` equals 100%.
//!
//! This module is the single source of truth for risk parameter storage.
//! [`crate::risk_management`] delegates parameter writes here and reads
//! through [`crate::risk_management::RiskConfig`] for convenience.

use soroban_sdk::{contracterror, contracttype, Env};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BASIS_POINTS: i128 = 10_000;

/// Minimum allowed collateral ratio: 100% (a ratio below this is always unsafe).
const MIN_COLLATERAL_RATIO_FLOOR: i128 = BASIS_POINTS;
/// Maximum allowed collateral ratio: 1000%.
const MAX_COLLATERAL_RATIO: i128 = 100_000;

/// Maximum allowed liquidation threshold (must be below min collateral ratio
/// in a well-configured system, but we only enforce an upper bound here).
const MAX_LIQUIDATION_THRESHOLD: i128 = 100_000;

/// Close factor must be in [1, 100%].  Zero is rejected so liquidations can
/// always clear at least one unit of bad debt.
const MIN_CLOSE_FACTOR: i128 = 1;
/// Close factor must be in [1, 100%].
const MAX_CLOSE_FACTOR: i128 = BASIS_POINTS;

/// Liquidation incentive is bounded to [0, 50%].
const MIN_LIQUIDATION_INCENTIVE: i128 = 0;
/// Liquidation incentive is bounded to [0, 50%].
const MAX_LIQUIDATION_INCENTIVE: i128 = 5_000;

/// Maximum parameter change per update expressed as a fraction of the current
/// value, in bps.  10% change in one call (1_000 bps).  Matches the paced-rate
/// governance cap documented in `stellar-lend/risk_params.md` (and
/// `docs/deployment.md` / `INITIALIZATION_SECURITY_NOTES.md`).
///
/// Applied uniformly to all four parameters below the MAX_* bounds:
///   `| new_value - current_value | <= current_value * MAX_CHANGE_BPS / 10_000`
const MAX_CHANGE_BPS: i128 = 1_000;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RiskParamsDataKey {
    /// Minimum collateral ratio in bps.
    MinCollateralRatio,
    /// Liquidation threshold in bps.
    LiquidationThreshold,
    /// Close factor in bps.
    CloseFactor,
    /// Liquidation incentive in bps.
    LiquidationIncentive,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by risk-parameter operations.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RiskParamsError {
    /// Contract has not been initialized.
    NotInitialized = 1,
    /// Contract has already been initialized.
    AlreadyInitialized = 2,
    /// A provided parameter is outside its allowed range.
    InvalidParameter = 3,
    /// The supplied collateral ratio is invalid.
    InvalidCollateralRatio = 4,
    /// The supplied liquidation threshold is invalid.
    InvalidLiquidationThreshold = 5,
    /// The supplied close factor is invalid.
    InvalidCloseFactor = 6,
    /// The supplied liquidation incentive is invalid.
    InvalidLiquidationIncentive = 7,
    /// A parameter change exceeds the maximum allowed single-step delta.
    ParameterChangeTooLarge = 8,
    /// Checked arithmetic overflowed.
    Overflow = 9,
    /// Division by zero was prevented.
    DivisionByZero = 10,
    /// The position does not meet the minimum collateral ratio.
    InsufficientCollateralRatio = 11,
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

fn default_min_collateral_ratio() -> i128 {
    15_000 // 150%
}
fn default_liquidation_threshold() -> i128 {
    12_000 // 120%
}
fn default_close_factor() -> i128 {
    5_000 // 50%
}
fn default_liquidation_incentive() -> i128 {
    500 // 5%
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Persist default risk parameters.  Must be called once during contract
/// initialization.
pub fn initialize_risk_params(env: &Env) -> Result<(), RiskParamsError> {
    if env
        .storage()
        .persistent()
        .has(&RiskParamsDataKey::MinCollateralRatio)
    {
        return Err(RiskParamsError::AlreadyInitialized);
    }

    env.storage().persistent().set(
        &RiskParamsDataKey::MinCollateralRatio,
        &default_min_collateral_ratio(),
    );
    env.storage().persistent().set(
        &RiskParamsDataKey::LiquidationThreshold,
        &default_liquidation_threshold(),
    );
    env.storage()
        .persistent()
        .set(&RiskParamsDataKey::CloseFactor, &default_close_factor());
    env.storage().persistent().set(
        &RiskParamsDataKey::LiquidationIncentive,
        &default_liquidation_incentive(),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Read helpers
// ---------------------------------------------------------------------------

/// Return the current minimum collateral ratio in bps.
pub fn get_min_collateral_ratio(env: &Env) -> Result<i128, RiskParamsError> {
    env.storage()
        .persistent()
        .get::<RiskParamsDataKey, i128>(&RiskParamsDataKey::MinCollateralRatio)
        .ok_or(RiskParamsError::NotInitialized)
}

/// Return the current liquidation threshold in bps.
pub fn get_liquidation_threshold(env: &Env) -> Result<i128, RiskParamsError> {
    env.storage()
        .persistent()
        .get::<RiskParamsDataKey, i128>(&RiskParamsDataKey::LiquidationThreshold)
        .ok_or(RiskParamsError::NotInitialized)
}

/// Return the current close factor in bps.
pub fn get_close_factor(env: &Env) -> Result<i128, RiskParamsError> {
    env.storage()
        .persistent()
        .get::<RiskParamsDataKey, i128>(&RiskParamsDataKey::CloseFactor)
        .ok_or(RiskParamsError::NotInitialized)
}

/// Return the current liquidation incentive in bps.
pub fn get_liquidation_incentive(env: &Env) -> Result<i128, RiskParamsError> {
    env.storage()
        .persistent()
        .get::<RiskParamsDataKey, i128>(&RiskParamsDataKey::LiquidationIncentive)
        .ok_or(RiskParamsError::NotInitialized)
}

// ---------------------------------------------------------------------------
// Parameter updates
// ---------------------------------------------------------------------------

/// Validate and store updated risk parameters.
///
/// Any `None` field is left unchanged.  Each provided value is validated
/// independently; an out-of-range value returns the corresponding error before
/// any storage is modified.
pub fn set_risk_params(
    env: &Env,
    min_collateral_ratio: Option<i128>,
    liquidation_threshold: Option<i128>,
    close_factor: Option<i128>,
    liquidation_incentive: Option<i128>,
) -> Result<(), RiskParamsError> {
    if let Some(v) = min_collateral_ratio {
        validate_change(
            env,
            &RiskParamsDataKey::MinCollateralRatio,
            v,
            MIN_COLLATERAL_RATIO_FLOOR,
            MAX_COLLATERAL_RATIO,
        )
        .map_err(|e| match e {
            RiskParamsError::ParameterChangeTooLarge => RiskParamsError::ParameterChangeTooLarge,
            // Pass internal arithmetic faults through unchanged so they can
            // surface as `RiskManagementError::Overflow` at the contract
            // entrypoint instead of being misreported as parameter bounds.
            RiskParamsError::Overflow => RiskParamsError::Overflow,
            RiskParamsError::DivisionByZero => RiskParamsError::DivisionByZero,
            _ => RiskParamsError::InvalidCollateralRatio,
        })?;
        env.storage()
            .persistent()
            .set(&RiskParamsDataKey::MinCollateralRatio, &v);
    }

    if let Some(v) = liquidation_threshold {
        validate_change(
            env,
            &RiskParamsDataKey::LiquidationThreshold,
            v,
            1,
            MAX_LIQUIDATION_THRESHOLD,
        )
        .map_err(|e| match e {
            RiskParamsError::ParameterChangeTooLarge => RiskParamsError::ParameterChangeTooLarge,
            RiskParamsError::Overflow => RiskParamsError::Overflow,
            RiskParamsError::DivisionByZero => RiskParamsError::DivisionByZero,
            _ => RiskParamsError::InvalidLiquidationThreshold,
        })?;
        env.storage()
            .persistent()
            .set(&RiskParamsDataKey::LiquidationThreshold, &v);
    }

    if let Some(v) = close_factor {
        // `validate_change` enforces both the absolute bounds and the paced
        // 10% delta cap.  Without it, `close_factor` could be spiked from its
        // default straight to `MAX_CLOSE_FACTOR` in a single transaction —
        // see `stellar-lend/risk_params.md` "Paced Rate Changes".
        validate_change(
            env,
            &RiskParamsDataKey::CloseFactor,
            v,
            MIN_CLOSE_FACTOR,
            MAX_CLOSE_FACTOR,
        )
        .map_err(|e| match e {
            RiskParamsError::ParameterChangeTooLarge => RiskParamsError::ParameterChangeTooLarge,
            RiskParamsError::Overflow => RiskParamsError::Overflow,
            RiskParamsError::DivisionByZero => RiskParamsError::DivisionByZero,
            _ => RiskParamsError::InvalidCloseFactor,
        })?;
        env.storage()
            .persistent()
            .set(&RiskParamsDataKey::CloseFactor, &v);
    }

    if let Some(v) = liquidation_incentive {
        // `validate_change` enforces both the absolute bounds and the paced
        // 10% delta cap.  Without it, `liquidation_incentive` could similarly
        // jump from default to maximum in one call.
        validate_change(
            env,
            &RiskParamsDataKey::LiquidationIncentive,
            v,
            MIN_LIQUIDATION_INCENTIVE,
            MAX_LIQUIDATION_INCENTIVE,
        )
        .map_err(|e| match e {
            RiskParamsError::ParameterChangeTooLarge => RiskParamsError::ParameterChangeTooLarge,
            RiskParamsError::Overflow => RiskParamsError::Overflow,
            RiskParamsError::DivisionByZero => RiskParamsError::DivisionByZero,
            _ => RiskParamsError::InvalidLiquidationIncentive,
        })?;
        env.storage()
            .persistent()
            .set(&RiskParamsDataKey::LiquidationIncentive, &v);
    }

    Ok(())
}

/// Validate that `new_value` is within `[min, max]` and that the change from
/// the current persisted value does not exceed `MAX_CHANGE_BPS`.
fn validate_change(
    env: &Env,
    key: &RiskParamsDataKey,
    new_value: i128,
    min: i128,
    max: i128,
) -> Result<(), RiskParamsError> {
    if new_value < min || new_value > max {
        return Err(RiskParamsError::InvalidParameter);
    }

    // If already initialized, check the per-call change limit.
    if let Some(current) = env
        .storage()
        .persistent()
        .get::<RiskParamsDataKey, i128>(key)
    {
        let delta = (new_value - current).abs();
        let limit = current
            .checked_mul(MAX_CHANGE_BPS)
            .ok_or(RiskParamsError::Overflow)?
            .checked_div(BASIS_POINTS)
            .ok_or(RiskParamsError::DivisionByZero)?;
        if delta > limit {
            return Err(RiskParamsError::ParameterChangeTooLarge);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Liquidation logic
// ---------------------------------------------------------------------------

/// Return `true` if `collateral_value * 10_000 < debt_value *
/// liquidation_threshold_bps` (position is undercollateralized).
pub fn can_be_liquidated(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<bool, RiskParamsError> {
    if debt_value <= 0 {
        return Ok(false);
    }
    let threshold = get_liquidation_threshold(env)?;
    // collateral_value * 10_000 < debt_value * threshold
    let lhs = collateral_value
        .checked_mul(BASIS_POINTS)
        .ok_or(RiskParamsError::Overflow)?;
    let rhs = debt_value
        .checked_mul(threshold)
        .ok_or(RiskParamsError::Overflow)?;
    Ok(lhs < rhs)
}

/// Return the maximum amount that may be repaid in a single liquidation call:
/// `debt_value * close_factor_bps / 10_000`.
pub fn get_max_liquidatable_amount(
    env: &Env,
    debt_value: i128,
) -> Result<i128, RiskParamsError> {
    let close_factor = get_close_factor(env)?;
    debt_value
        .checked_mul(close_factor)
        .ok_or(RiskParamsError::Overflow)?
        .checked_div(BASIS_POINTS)
        .ok_or(RiskParamsError::DivisionByZero)
}

/// Return the collateral bonus paid to the liquidator:
/// `liquidated_amount * liquidation_incentive_bps / 10_000`.
pub fn get_liquidation_incentive_amount(
    env: &Env,
    liquidated_amount: i128,
) -> Result<i128, RiskParamsError> {
    let incentive = get_liquidation_incentive(env)?;
    liquidated_amount
        .checked_mul(incentive)
        .ok_or(RiskParamsError::Overflow)?
        .checked_div(BASIS_POINTS)
        .ok_or(RiskParamsError::DivisionByZero)
}

/// Require that `collateral_value / debt_value >= min_collateral_ratio`.
///
/// Expressed as: `collateral_value * 10_000 >= debt_value * min_ratio_bps`.
pub fn require_min_collateral_ratio(
    env: &Env,
    collateral_value: i128,
    debt_value: i128,
) -> Result<(), RiskParamsError> {
    if debt_value <= 0 {
        return Ok(());
    }
    let min_ratio = get_min_collateral_ratio(env)?;
    let lhs = collateral_value
        .checked_mul(BASIS_POINTS)
        .ok_or(RiskParamsError::Overflow)?;
    let rhs = debt_value
        .checked_mul(min_ratio)
        .ok_or(RiskParamsError::Overflow)?;
    if lhs < rhs {
        return Err(RiskParamsError::InsufficientCollateralRatio);
    }
    Ok(())
}
