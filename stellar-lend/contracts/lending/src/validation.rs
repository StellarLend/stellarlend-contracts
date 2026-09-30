//! # Input and State Validation Module
//!
//! This module provides comprehensive input validation and state consistency checks
//! for all contract operations. It enforces boundaries at multiple levels:
//!
//! 1. **Parameter Validation**: Type checking, range validation, format verification
//! 2. **Numeric Validation**: Overflow, underflow, zero, negative checks
//! 3. **Asset Validation**: Asset support, configuration, compatibility
//! 4. **State Validation**: Position health, reserve ratios, caps, ceilings
//! 5. **Server Response Validation**: Oracle data, timestamps, signatures
//!
//! ## Design Principles
//!
//! - **Fail fast**: Validate before any state changes
//! - **Clear errors**: Specific error types for each failure mode
//! - **Deterministic**: Same input always produces same validation result
//! - **Auditable**: All validation failures are logged
//!
//! ## Usage
//!
//! ```ignore
//! use crate::validation::{validate_amount, validate_asset_configured};
//!
//! pub fn deposit(env: Env, user: Address, amount: i128) -> Result<(), LendingError> {
//!     // Validate inputs before proceeding
//!     validate_amount(amount)?;
//!     
//!     // ... rest of operation ...
//! }
//! ```

use soroban_sdk::{Address, BytesN, Env};

use crate::DataKey;

/// Maximum valid timestamp deviation from current time (in seconds).
/// Prevents operations with timestamps too far in the future or past.
const MAX_TIMESTAMP_DEVIATION_SECS: u64 = 300; // 5 minutes

/// Maximum price age for oracle data (in seconds).
/// Prices older than this are considered stale.
const MAX_PRICE_AGE_SECS: u64 = 3600; // 1 hour

/// Minimum health factor to maintain (scaled by 10000).
/// Health factor below this indicates liquidatable position.
const MIN_HEALTH_FACTOR: i128 = 10000; // 1.0

/// Basis points denominator (100% = 10000 bps).
const BPS_DENOM: i128 = 10000;

/// Validation error types.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// Amount is zero, negative, or exceeds maximum.
    InvalidAmount = 10001,
    /// Numeric operation would overflow.
    NumericOverflow = 10002,
    /// Numeric operation would underflow.
    NumericUnderflow = 10003,
    /// Asset is not configured in the protocol.
    AssetNotConfigured = 10004,
    /// Asset is not supported for this operation.
    AssetNotSupported = 10005,
    /// Asset addresses do not match expected values.
    AssetMismatch = 10006,
    /// Health factor is below minimum threshold.
    HealthFactorTooLow = 10007,
    /// Operation would exceed configured cap or ceiling.
    CapExceeded = 10008,
    /// Oracle price is stale (timestamp too old).
    StalePriceData = 10009,
    /// Oracle signature is invalid.
    InvalidOracleSignature = 10010,
    /// Timestamp is outside acceptable range.
    InvalidTimestamp = 10011,
    /// Price is outside configured bounds.
    PriceOutOfBounds = 10012,
    /// Required parameter is missing.
    MissingParameter = 10013,
    /// Parameter value is out of acceptable range.
    ParameterOutOfRange = 10014,
    /// State is inconsistent or corrupted.
    InconsistentState = 10015,
    /// Reserve ratio is below safety threshold.
    InsufficientReserves = 10016,
}

/// Validate that an amount is positive and non-zero.
///
/// # Arguments
/// * `amount` - The amount to validate
///
/// # Returns
/// `Ok(())` if valid, error otherwise
///
/// # Errors
/// - `ValidationError::InvalidAmount` - Amount is <= 0
pub fn validate_amount(amount: i128) -> Result<(), ValidationError> {
    if amount <= 0 {
        return Err(ValidationError::InvalidAmount);
    }
    Ok(())
}

/// Validate that an amount is within acceptable range.
///
/// # Arguments
/// * `amount` - The amount to validate
/// * `min` - Minimum acceptable value (inclusive)
/// * `max` - Maximum acceptable value (inclusive)
///
/// # Returns
/// `Ok(())` if valid, error otherwise
///
/// # Errors
/// - `ValidationError::InvalidAmount` - Amount is outside [min, max]
pub fn validate_amount_range(amount: i128, min: i128, max: i128) -> Result<(), ValidationError> {
    if amount < min || amount > max {
        return Err(ValidationError::InvalidAmount);
    }
    Ok(())
}

/// Validate that an addition will not overflow.
///
/// # Arguments
/// * `a` - First operand
/// * `b` - Second operand
///
/// # Returns
/// `Ok(sum)` if no overflow, error otherwise
///
/// # Errors
/// - `ValidationError::NumericOverflow` - Addition would overflow
pub fn validate_add(a: i128, b: i128) -> Result<i128, ValidationError> {
    a.checked_add(b)
        .ok_or(ValidationError::NumericOverflow)
}

/// Validate that a subtraction will not underflow.
///
/// # Arguments
/// * `a` - First operand (minuend)
/// * `b` - Second operand (subtrahend)
///
/// # Returns
/// `Ok(difference)` if no underflow, error otherwise
///
/// # Errors
/// - `ValidationError::NumericUnderflow` - Subtraction would underflow
pub fn validate_sub(a: i128, b: i128) -> Result<i128, ValidationError> {
    a.checked_sub(b)
        .ok_or(ValidationError::NumericUnderflow)
}

/// Validate that a multiplication will not overflow.
///
/// # Arguments
/// * `a` - First operand
/// * `b` - Second operand
///
/// # Returns
/// `Ok(product)` if no overflow, error otherwise
///
/// # Errors
/// - `ValidationError::NumericOverflow` - Multiplication would overflow
pub fn validate_mul(a: i128, b: i128) -> Result<i128, ValidationError> {
    a.checked_mul(b)
        .ok_or(ValidationError::NumericOverflow)
}

/// Validate that a division is safe (no division by zero).
///
/// # Arguments
/// * `a` - Numerator
/// * `b` - Denominator
///
/// # Returns
/// `Ok(quotient)` if safe, error otherwise
///
/// # Errors
/// - `ValidationError::InvalidAmount` - Division by zero
pub fn validate_div(a: i128, b: i128) -> Result<i128, ValidationError> {
    if b == 0 {
        return Err(ValidationError::InvalidAmount);
    }
    a.checked_div(b)
        .ok_or(ValidationError::NumericOverflow)
}

/// Validate that an asset is configured in the protocol.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset address to validate
///
/// # Returns
/// `Ok(())` if asset is configured, error otherwise
///
/// # Errors
/// - `ValidationError::AssetNotConfigured` - Asset has no configuration
pub fn validate_asset_configured(env: &Env, asset: &Address) -> Result<(), ValidationError> {
    let key = DataKey::AssetParams(asset.clone());
    
    if !env.storage().persistent().has(&key) {
        return Err(ValidationError::AssetNotConfigured);
    }
    
    Ok(())
}

/// Validate that two assets match.
///
/// # Arguments
/// * `expected` - The expected asset address
/// * `actual` - The actual asset address provided
///
/// # Returns
/// `Ok(())` if assets match, error otherwise
///
/// # Errors
/// - `ValidationError::AssetMismatch` - Assets do not match
pub fn validate_asset_match(expected: &Address, actual: &Address) -> Result<(), ValidationError> {
    if expected != actual {
        return Err(ValidationError::AssetMismatch);
    }
    Ok(())
}

/// Validate that a health factor is above the minimum threshold.
///
/// # Arguments
/// * `health_factor` - The health factor to validate (scaled by 10000)
///
/// # Returns
/// `Ok(())` if health factor is sufficient, error otherwise
///
/// # Errors
/// - `ValidationError::HealthFactorTooLow` - Health factor below minimum
pub fn validate_health_factor(health_factor: i128) -> Result<(), ValidationError> {
    if health_factor < MIN_HEALTH_FACTOR {
        return Err(ValidationError::HealthFactorTooLow);
    }
    Ok(())
}

/// Validate that a timestamp is within acceptable range of current time.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `timestamp` - The timestamp to validate
///
/// # Returns
/// `Ok(())` if timestamp is acceptable, error otherwise
///
/// # Errors
/// - `ValidationError::InvalidTimestamp` - Timestamp is too old or too far in future
pub fn validate_timestamp(env: &Env, timestamp: u64) -> Result<(), ValidationError> {
    let current = env.ledger().timestamp();
    
    let deviation = if timestamp > current {
        timestamp - current
    } else {
        current - timestamp
    };
    
    if deviation > MAX_TIMESTAMP_DEVIATION_SECS {
        return Err(ValidationError::InvalidTimestamp);
    }
    
    Ok(())
}

/// Validate that oracle price data is fresh (not stale).
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `price_timestamp` - The timestamp of the price data
///
/// # Returns
/// `Ok(())` if price is fresh, error otherwise
///
/// # Errors
/// - `ValidationError::StalePriceData` - Price timestamp is too old
pub fn validate_price_freshness(env: &Env, price_timestamp: u64) -> Result<(), ValidationError> {
    let current = env.ledger().timestamp();
    
    if price_timestamp > current {
        // Price from the future is invalid
        return Err(ValidationError::InvalidTimestamp);
    }
    
    let age = current - price_timestamp;
    
    if age > MAX_PRICE_AGE_SECS {
        return Err(ValidationError::StalePriceData);
    }
    
    Ok(())
}

/// Validate that a price is within configured bounds.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset address
/// * `price` - The price to validate
///
/// # Returns
/// `Ok(())` if price is within bounds, error otherwise
///
/// # Errors
/// - `ValidationError::PriceOutOfBounds` - Price exceeds min/max bounds
pub fn validate_price_bounds(
    env: &Env,
    asset: &Address,
    price: i128,
) -> Result<(), ValidationError> {
    // Get configured price bounds
    let min_key = DataKey::PriceMin(asset.clone());
    let max_key = DataKey::PriceMax(asset.clone());
    
    let min_price: Option<i128> = env.storage().persistent().get(&min_key);
    let max_price: Option<i128> = env.storage().persistent().get(&max_key);
    
    // If bounds are configured, validate against them
    if let Some(min) = min_price {
        if price < min {
            return Err(ValidationError::PriceOutOfBounds);
        }
    }
    
    if let Some(max) = max_price {
        if price > max {
            return Err(ValidationError::PriceOutOfBounds);
        }
    }
    
    Ok(())
}

/// Validate that an oracle signature is valid.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `message` - The signed message
/// * `signature` - The signature to verify
/// * `pubkey` - The expected public key
///
/// # Returns
/// `Ok(())` if signature is valid, error otherwise
///
/// # Errors
/// - `ValidationError::InvalidOracleSignature` - Signature verification failed
pub fn validate_oracle_signature(
    env: &Env,
    message: &BytesN<32>,
    signature: &BytesN<64>,
    pubkey: &BytesN<32>,
) -> Result<(), ValidationError> {
    // Verify Ed25519 signature
    env.crypto()
        .ed25519_verify(pubkey, message, signature);
    
    // Note: ed25519_verify panics on failure in Soroban
    // If we reach here, signature is valid
    Ok(())
}

/// Validate that a basis points value is within valid range [0, 10000].
///
/// # Arguments
/// * `bps` - The basis points value
///
/// # Returns
/// `Ok(())` if valid, error otherwise
///
/// # Errors
/// - `ValidationError::ParameterOutOfRange` - BPS outside [0, 10000]
pub fn validate_bps(bps: i128) -> Result<(), ValidationError> {
    if bps < 0 || bps > BPS_DENOM {
        return Err(ValidationError::ParameterOutOfRange);
    }
    Ok(())
}

/// Validate that an operation will not exceed a configured cap.
///
/// # Arguments
/// * `current` - Current amount
/// * `addition` - Amount to add
/// * `cap` - Maximum allowed total
///
/// # Returns
/// `Ok(())` if within cap, error otherwise
///
/// # Errors
/// - `ValidationError::CapExceeded` - Operation would exceed cap
pub fn validate_cap(current: i128, addition: i128, cap: i128) -> Result<(), ValidationError> {
    if cap <= 0 {
        // Cap of 0 or negative means unlimited
        return Ok(());
    }
    
    let new_total = validate_add(current, addition)?;
    
    if new_total > cap {
        return Err(ValidationError::CapExceeded);
    }
    
    Ok(())
}

/// Validate that reserve ratio is above safety threshold.
///
/// # Arguments
/// * `reserves` - Total reserves
/// * `liabilities` - Total liabilities
/// * `min_ratio_bps` - Minimum reserve ratio in basis points
///
/// # Returns
/// `Ok(())` if reserve ratio is sufficient, error otherwise
///
/// # Errors
/// - `ValidationError::InsufficientReserves` - Reserve ratio below minimum
pub fn validate_reserve_ratio(
    reserves: i128,
    liabilities: i128,
    min_ratio_bps: i128,
) -> Result<(), ValidationError> {
    if liabilities == 0 {
        // No liabilities, reserves are sufficient
        return Ok(());
    }
    
    // Calculate reserve ratio: (reserves / liabilities) * 10000
    let ratio = validate_mul(reserves, BPS_DENOM)?;
    let ratio_bps = validate_div(ratio, liabilities)?;
    
    if ratio_bps < min_ratio_bps {
        return Err(ValidationError::InsufficientReserves);
    }
    
    Ok(())
}

/// Validate that a user's position is consistent (no state corruption).
///
/// # Arguments
/// * `collateral` - User's collateral balance
/// * `debt` - User's debt balance
///
/// # Returns
/// `Ok(())` if position is consistent, error otherwise
///
/// # Errors
/// - `ValidationError::InconsistentState` - Position has negative values
pub fn validate_position_consistency(
    collateral: i128,
    debt: i128,
) -> Result<(), ValidationError> {
    // Collateral and debt should never be negative
    if collateral < 0 || debt < 0 {
        return Err(ValidationError::InconsistentState);
    }
    
    Ok(())
}

/// Comprehensive validation for deposit operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset being deposited
/// * `amount` - The deposit amount
/// * `current_total` - Current total deposits
/// * `deposit_cap` - Maximum allowed deposits
///
/// # Returns
/// `Ok(())` if all validations pass, error otherwise
pub fn validate_deposit(
    env: &Env,
    asset: &Address,
    amount: i128,
    current_total: i128,
    deposit_cap: i128,
) -> Result<(), ValidationError> {
    validate_amount(amount)?;
    validate_asset_configured(env, asset)?;
    validate_cap(current_total, amount, deposit_cap)?;
    Ok(())
}

/// Comprehensive validation for withdrawal operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset being withdrawn
/// * `amount` - The withdrawal amount
/// * `current_balance` - Current user balance
/// * `health_factor_after` - Health factor after withdrawal
///
/// # Returns
/// `Ok(())` if all validations pass, error otherwise
pub fn validate_withdrawal(
    env: &Env,
    asset: &Address,
    amount: i128,
    current_balance: i128,
    health_factor_after: i128,
) -> Result<(), ValidationError> {
    validate_amount(amount)?;
    validate_asset_configured(env, asset)?;
    
    // Cannot withdraw more than balance
    if amount > current_balance {
        return Err(ValidationError::InvalidAmount);
    }
    
    validate_health_factor(health_factor_after)?;
    Ok(())
}

/// Comprehensive validation for borrow operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset being borrowed
/// * `amount` - The borrow amount
/// * `current_total_debt` - Current total debt for this asset
/// * `borrow_cap` - Maximum allowed borrows
/// * `health_factor_after` - Health factor after borrow
///
/// # Returns
/// `Ok(())` if all validations pass, error otherwise
pub fn validate_borrow(
    env: &Env,
    asset: &Address,
    amount: i128,
    current_total_debt: i128,
    borrow_cap: i128,
    health_factor_after: i128,
) -> Result<(), ValidationError> {
    validate_amount(amount)?;
    validate_asset_configured(env, asset)?;
    validate_cap(current_total_debt, amount, borrow_cap)?;
    validate_health_factor(health_factor_after)?;
    Ok(())
}

/// Comprehensive validation for repay operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset being repaid
/// * `amount` - The repay amount
/// * `current_debt` - Current user debt
///
/// # Returns
/// `Ok(())` if all validations pass, error otherwise
pub fn validate_repay(
    env: &Env,
    asset: &Address,
    amount: i128,
    current_debt: i128,
) -> Result<(), ValidationError> {
    validate_amount(amount)?;
    validate_asset_configured(env, asset)?;
    
    // Cannot repay more than debt (unless it's a rounding error tolerance)
    if amount > current_debt + 1 {
        return Err(ValidationError::InvalidAmount);
    }
    
    Ok(())
}

/// Comprehensive validation for liquidation operations.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `debt_asset` - The debt asset
/// * `collateral_asset` - The collateral asset
/// * `repay_amount` - Amount being repaid
/// * `borrower_health_factor` - Borrower's health factor
///
/// # Returns
/// `Ok(())` if all validations pass, error otherwise
pub fn validate_liquidation(
    env: &Env,
    debt_asset: &Address,
    collateral_asset: &Address,
    repay_amount: i128,
    borrower_health_factor: i128,
) -> Result<(), ValidationError> {
    validate_amount(repay_amount)?;
    validate_asset_configured(env, debt_asset)?;
    validate_asset_configured(env, collateral_asset)?;
    
    // Borrower must be unhealthy to be liquidated
    if borrower_health_factor >= MIN_HEALTH_FACTOR {
        return Err(ValidationError::HealthFactorTooLow);
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Env,
    };

    #[test]
    fn test_validate_amount_accepts_positive() {
        assert!(validate_amount(100).is_ok());
        assert!(validate_amount(1).is_ok());
        assert!(validate_amount(i128::MAX).is_ok());
    }

    #[test]
    fn test_validate_amount_rejects_zero_and_negative() {
        assert_eq!(validate_amount(0), Err(ValidationError::InvalidAmount));
        assert_eq!(validate_amount(-1), Err(ValidationError::InvalidAmount));
        assert_eq!(
            validate_amount(i128::MIN),
            Err(ValidationError::InvalidAmount)
        );
    }

    #[test]
    fn test_validate_amount_range() {
        assert!(validate_amount_range(50, 0, 100).is_ok());
        assert!(validate_amount_range(0, 0, 100).is_ok());
        assert!(validate_amount_range(100, 0, 100).is_ok());
        assert_eq!(
            validate_amount_range(101, 0, 100),
            Err(ValidationError::InvalidAmount)
        );
        assert_eq!(
            validate_amount_range(-1, 0, 100),
            Err(ValidationError::InvalidAmount)
        );
    }

    #[test]
    fn test_validate_add_detects_overflow() {
        assert!(validate_add(100, 200).is_ok());
        assert_eq!(validate_add(100, 200).unwrap(), 300);
        assert_eq!(
            validate_add(i128::MAX, 1),
            Err(ValidationError::NumericOverflow)
        );
    }

    #[test]
    fn test_validate_sub_detects_underflow() {
        assert!(validate_sub(200, 100).is_ok());
        assert_eq!(validate_sub(200, 100).unwrap(), 100);
        assert_eq!(
            validate_sub(i128::MIN, 1),
            Err(ValidationError::NumericUnderflow)
        );
    }

    #[test]
    fn test_validate_mul_detects_overflow() {
        assert!(validate_mul(10, 20).is_ok());
        assert_eq!(validate_mul(10, 20).unwrap(), 200);
        assert_eq!(
            validate_mul(i128::MAX, 2),
            Err(ValidationError::NumericOverflow)
        );
    }

    #[test]
    fn test_validate_div_prevents_division_by_zero() {
        assert!(validate_div(100, 2).is_ok());
        assert_eq!(validate_div(100, 2).unwrap(), 50);
        assert_eq!(validate_div(100, 0), Err(ValidationError::InvalidAmount));
    }

    #[test]
    fn test_validate_health_factor() {
        assert!(validate_health_factor(10000).is_ok());
        assert!(validate_health_factor(15000).is_ok());
        assert_eq!(
            validate_health_factor(9999),
            Err(ValidationError::HealthFactorTooLow)
        );
        assert_eq!(
            validate_health_factor(0),
            Err(ValidationError::HealthFactorTooLow)
        );
    }

    #[test]
    fn test_validate_bps() {
        assert!(validate_bps(0).is_ok());
        assert!(validate_bps(5000).is_ok());
        assert!(validate_bps(10000).is_ok());
        assert_eq!(
            validate_bps(-1),
            Err(ValidationError::ParameterOutOfRange)
        );
        assert_eq!(
            validate_bps(10001),
            Err(ValidationError::ParameterOutOfRange)
        );
    }

    #[test]
    fn test_validate_cap_enforces_limit() {
        assert!(validate_cap(100, 50, 200).is_ok());
        assert_eq!(validate_cap(100, 150, 200), Err(ValidationError::CapExceeded));
        
        // Cap of 0 means unlimited
        assert!(validate_cap(100, 1000000, 0).is_ok());
    }

    #[test]
    fn test_validate_reserve_ratio() {
        // 100% reserve ratio (10000 bps)
        assert!(validate_reserve_ratio(100, 100, 10000).is_ok());
        
        // 150% reserve ratio
        assert!(validate_reserve_ratio(150, 100, 10000).is_ok());
        
        // 50% reserve ratio - fails if minimum is 100%
        assert_eq!(
            validate_reserve_ratio(50, 100, 10000),
            Err(ValidationError::InsufficientReserves)
        );
        
        // No liabilities - always passes
        assert!(validate_reserve_ratio(50, 0, 10000).is_ok());
    }

    #[test]
    fn test_validate_position_consistency() {
        assert!(validate_position_consistency(100, 50).is_ok());
        assert!(validate_position_consistency(0, 0).is_ok());
        assert_eq!(
            validate_position_consistency(-1, 50),
            Err(ValidationError::InconsistentState)
        );
        assert_eq!(
            validate_position_consistency(100, -1),
            Err(ValidationError::InconsistentState)
        );
    }

    #[test]
    fn test_validate_timestamp() {
        let env = Env::default();
        let current = env.ledger().timestamp();
        
        // Current timestamp is valid
        assert!(validate_timestamp(&env, current).is_ok());
        
        // Within tolerance
        assert!(validate_timestamp(&env, current + 100).is_ok());
        assert!(validate_timestamp(&env, current - 100).is_ok());
        
        // Outside tolerance
        assert_eq!(
            validate_timestamp(&env, current + MAX_TIMESTAMP_DEVIATION_SECS + 1),
            Err(ValidationError::InvalidTimestamp)
        );
    }

    #[test]
    fn test_validate_price_freshness() {
        let env = Env::default();
        let current = env.ledger().timestamp();
        
        // Fresh price is valid
        assert!(validate_price_freshness(&env, current).is_ok());
        assert!(validate_price_freshness(&env, current - 100).is_ok());
        
        // Stale price is invalid
        assert_eq!(
            validate_price_freshness(&env, current - MAX_PRICE_AGE_SECS - 1),
            Err(ValidationError::StalePriceData)
        );
        
        // Future price is invalid
        assert_eq!(
            validate_price_freshness(&env, current + 100),
            Err(ValidationError::InvalidTimestamp)
        );
    }
}
