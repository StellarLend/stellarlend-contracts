//! # Parameter Validation Helpers
//!
//! Centralized logic for enforcing strict bounds on protocol parameters.
//! Prevents governance misconfiguration by rejecting unsafe values.

use crate::constants::{
    BPS_SCALE, MAX_CLOSE_FACTOR_BPS, MAX_FLASH_LOAN_FEE_BPS, MAX_LIQUIDATION_INCENTIVE_BPS,
    MAX_LTV_BPS, MAX_ORACLE_STALENESS_SECONDS, MIN_LTV_BPS, MIN_ORACLE_STALENESS_SECONDS,
};

/// Check if a value is within standard basis point bounds (0 - 10,000).
pub fn is_valid_bps(bps: i128) -> bool {
    bps >= 0 && bps <= BPS_SCALE
}

/// Check if LTV is within protocol-defined bounds.
pub fn is_valid_ltv(ltv: i128) -> bool {
    ltv >= MIN_LTV_BPS && ltv <= MAX_LTV_BPS
}

/// Check if liquidation threshold is valid relative to LTV.
/// Threshold must be strictly greater than LTV to prevent immediate liquidation.
pub fn is_valid_threshold(threshold: i128, ltv: i128) -> bool {
    threshold > ltv && threshold <= BPS_SCALE
}

/// Check if oracle staleness window is within safe operational bounds.
pub fn is_valid_staleness(seconds: u64) -> bool {
    seconds >= MIN_ORACLE_STALENESS_SECONDS && seconds <= MAX_ORACLE_STALENESS_SECONDS
}

/// Check if a cap or ceiling is non-negative.
pub fn is_valid_cap(amount: i128) -> bool {
    amount >= 0
}

/// Check if a multiplier (expressed in BPS) is within reasonable bounds (e.g. 0-200%).
pub fn is_valid_multiplier(multiplier_bps: i128) -> bool {
    multiplier_bps >= 0 && multiplier_bps <= BPS_SCALE * 2
}

/// Check if a utilization kink is within the safe range (0-100% exclusive).
pub fn is_valid_utilization_kink(kink_bps: i128) -> bool {
    kink_bps > 0 && kink_bps < BPS_SCALE
}

/// Check if a liquidation incentive is within the safe protocol range.
pub fn is_valid_liquidation_incentive(bps: i128) -> bool {
    bps >= 0 && bps <= MAX_LIQUIDATION_INCENTIVE_BPS
}

/// Check if a close factor is within the safe protocol range (must be non-zero).
pub fn is_valid_close_factor(bps: i128) -> bool {
    bps > 0 && bps <= MAX_CLOSE_FACTOR_BPS
}

/// Check if a flash loan fee is within the safe protocol range.
pub fn is_valid_flash_loan_fee(bps: i128) -> bool {
    bps >= 0 && bps <= MAX_FLASH_LOAN_FEE_BPS
}
