//! # Protocol Constants
//!
//! Canonical basis-point and ratio constants shared across all modules in the
//! `stellarlend-lending` crate.
//!
//! ## Basis Points
//! All percentage values in this protocol are expressed in **basis points (BPS)**
//! where `BPS_SCALE = 10_000` represents 100 %.
//!
//! Using a single named constant instead of the magic literal `10_000` prevents
//! silent mismatches and makes intent explicit at every call site.
//!
//! ## Security Notes
//! - Every parameter accepted from callers **must** be validated against these
//!   bounds before use.
//! - Arithmetic involving BPS values must use `checked_mul` / `checked_div` (or
//!   `I256` equivalents) to prevent overflow/underflow.
//! - Admin-settable parameters are bounded by the `*_MIN` / `*_MAX` constants
//!   defined here; no module should accept values outside these ranges.

/// 100 % expressed in basis points.
///
/// Use this constant whenever dividing or multiplying by the BPS scale so that
/// the intent is clear and a single change propagates everywhere.
pub const BPS_SCALE: i128 = 10_000;

/// Health-factor scale: a health factor of `1.0` is represented as
/// `HEALTH_FACTOR_SCALE`. Values below this threshold indicate a liquidatable
/// position.
pub const HEALTH_FACTOR_SCALE: i128 = BPS_SCALE;

/// Maximum allowed flash-loan fee (10 % = 1 000 bps).
///
/// Kept deliberately below `BPS_SCALE` to protect borrowers from excessive fees.
pub const MAX_FLASH_LOAN_FEE_BPS: i128 = 1_000;

/// Minimum collateral ratio for single-asset borrows (150 %).
pub const MIN_COLLATERAL_RATIO_BPS: i128 = 15_000;

/// Default liquidation threshold (80 %).
pub const DEFAULT_LIQUIDATION_THRESHOLD_BPS: i128 = 8_000;

/// Default close factor (50 %).
pub const DEFAULT_CLOSE_FACTOR_BPS: i128 = 5_000;

/// Default liquidation incentive (10 %).
pub const DEFAULT_LIQUIDATION_INCENTIVE_BPS: i128 = 1_000;
