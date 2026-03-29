//! # Reserve and Treasury Module
//!
//! Manages protocol reserves and treasury operations for the StellarLend lending protocol.
//!
//! ## Overview
//! This module implements the reserve factor mechanism that allocates a portion of protocol
//! interest income to the treasury. The reserve factor determines what percentage of interest
//! accrued from borrowers is retained by the protocol versus distributed to lenders.
//!
//! ## Key Concepts
//!
//! ### Reserve Factor
//! - A percentage (in basis points) of interest income allocated to protocol reserves
//! - Example: 1000 bps (10%) means 10% of interest goes to reserves, 90% to lenders
//! - Configurable per asset by admin
//! - Range: 0 - 5000 bps (0% - 50%)
//!
//! ### Reserve Accrual
//! - Reserves accrue automatically when interest is calculated during repayment
//! - Formula: `reserve_amount = total_interest * reserve_factor / 10000`
//! - Tracked separately per asset in persistent storage
//!
//! ### Treasury Withdrawal
//! - Admin can withdraw accrued reserves to a treasury address
//! - Withdrawals are bounded by the actual reserve balance
//! - Cannot withdraw user funds (collateral or principal)
//! - All withdrawals are logged via events
//!
//! ## Storage Layout
//! - `ReserveBalance(asset)` — accumulated reserve per asset
//! - `ReserveFactor(asset)` — reserve factor per asset (basis points)
//! - `TreasuryAddress` — destination address for reserve withdrawals
//!
//! ## Security Invariants
//! - Reserve factor must be between 0 and 5000 bps (0% - 50%)
//! - Only admin can modify reserve factors or withdraw reserves
//! - Withdrawals cannot exceed accrued reserve balance
//! - User funds (collateral, principal) are never accessible via treasury operations
//! - All state changes emit events for transparency and auditability

#![allow(unused)]
use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

use crate::deposit::DepositDataKey;

/// Maximum allowed reserve factor (50% = 5000 basis points)
/// This ensures that at least 50% of interest always goes to lenders
pub const MAX_RESERVE_FACTOR_BPS: i128 = 5000;

/// Default reserve factor (10% = 1000 basis points)
pub const DEFAULT_RESERVE_FACTOR_BPS: i128 = 1000;

/// Basis points scale (100% = 10000 basis points)
pub const BASIS_POINTS_SCALE: i128 = 10000;

/// Errors that can occur during reserve and treasury operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ReserveError {
    /// Caller is not authorized (not admin)
    Unauthorized = 1,
    /// Reserve factor exceeds maximum allowed value
    InvalidReserveFactor = 2,
    /// Withdrawal amount exceeds available reserve balance
    InsufficientReserve = 3,
    /// Invalid asset address
    InvalidAsset = 4,
    /// Invalid treasury address
    InvalidTreasury = 5,
    /// Withdrawal amount must be greater than zero
    InvalidAmount = 6,
    /// Arithmetic overflow occurred
    Overflow = 7,
    /// Treasury address not configured
    TreasuryNotSet = 8,
}

/// Storage keys for reserve and treasury data
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ReserveDataKey {
    /// Reserve balance per asset: ReserveBalance(asset) -> i128
    /// Tracks accumulated protocol reserves for each asset
    ReserveBalance(Option<Address>),
    /// Reserve factor per asset: ReserveFactor(asset) -> i128
    /// Percentage of interest allocated to reserves (in basis points)
    ReserveFactor(Option<Address>),
    /// Treasury address: TreasuryAddress -> Address
    /// Destination for reserve withdrawals
    TreasuryAddress,
}

/// Initialize reserve configuration for an asset
///
/// Sets the default reserve factor for a new asset. Should be called when
/// a new asset is added to the protocol.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset address (None for native asset)
/// * `reserve_factor_bps` - Reserve factor in basis points (0-5000)
///
/// # Errors
/// * `ReserveError::InvalidReserveFactor` - If reserve factor > MAX_RESERVE_FACTOR_BPS
/// * `ReserveError::Overflow` - If arithmetic overflow occurs
///
/// # Security
/// * No authorization check - should be called internally during asset initialization
/// * Validates reserve factor is within acceptable bounds
#[allow(deprecated)]
pub fn initialize_reserve_config(
    env: &Env,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError> {
    // Validate reserve factor
    if !(0..=MAX_RESERVE_FACTOR_BPS).contains(&reserve_factor_bps) {
        return Err(ReserveError::InvalidReserveFactor);
    }

    // Set reserve factor
    let factor_key = ReserveDataKey::ReserveFactor(asset.clone());
    env.storage()
        .persistent()
        .set(&factor_key, &reserve_factor_bps);

    // Initialize reserve balance to zero
    let balance_key = ReserveDataKey::ReserveBalance(asset.clone());
    env.storage().persistent().set(&balance_key, &0i128);

    // Emit initialization event
    let topics = (Symbol::new(env, "reserve_initialized"),);
    env.events().publish(topics, (asset, reserve_factor_bps));

    Ok(())
}

/// Set the reserve factor for an asset (admin only)
///
/// Updates the percentage of interest income allocated to protocol reserves.
/// This affects future interest accruals but does not retroactively change
/// existing reserve balances.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `asset` - The asset address (None for native asset)
/// * `reserve_factor_bps` - New reserve factor in basis points (0-5000)
///
/// # Errors
/// * `ReserveError::Unauthorized` - If caller is not admin
/// * `ReserveError::InvalidReserveFactor` - If reserve factor > MAX_RESERVE_FACTOR_BPS
///
/// # Security
/// * Requires admin authorization
/// * Validates reserve factor bounds
/// * Emits event for transparency
#[allow(deprecated)]
pub fn set_reserve_factor(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError> {
    // Require admin authorization
    caller.require_auth();
    require_admin(env, &caller)?;

    // Validate reserve factor
    if !(0..=MAX_RESERVE_FACTOR_BPS).contains(&reserve_factor_bps) {
        return Err(ReserveError::InvalidReserveFactor);
    }

    // Update reserve factor
    let factor_key = ReserveDataKey::ReserveFactor(asset.clone());
    env.storage()
        .persistent()
        .set(&factor_key, &reserve_factor_bps);

    // Emit event
    let topics = (Symbol::new(env, "reserve_factor_updated"), caller);
    env.events().publish(topics, (asset, reserve_factor_bps));

    Ok(())
}

/// Get the reserve factor for an asset
///
/// Returns the current reserve factor, or the default if not explicitly set.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset address (None for native asset)
///
/// # Returns
/// Reserve factor in basis points (0-5000)
pub fn get_reserve_factor(env: &Env, asset: Option<Address>) -> i128 {
    let factor_key = ReserveDataKey::ReserveFactor(asset);
    env.storage()
        .persistent()
        .get(&factor_key)
        .unwrap_or(DEFAULT_RESERVE_FACTOR_BPS)
}

/// Accrue reserves from interest payment
///
/// Called internally when interest is paid during repayment. Calculates the
/// protocol's share of interest based on the reserve factor and adds it to
/// the reserve balance.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset address (None for native asset)
/// * `interest_amount` - Total interest amount paid
///
/// # Returns
/// Tuple of (reserve_amount, lender_amount) showing the split of interest
///
/// # Errors
/// * `ReserveError::Overflow` - If arithmetic overflow occurs
///
/// # Security
/// * No authorization check - should be called internally during repayment
/// * Uses checked arithmetic to prevent overflow
/// * Emits event for transparency
///
/// # Formula
/// ```text
/// reserve_amount = interest_amount * reserve_factor / 10000
/// lender_amount = interest_amount - reserve_amount
/// ```
#[allow(deprecated)]
pub fn accrue_reserve(
    env: &Env,
    asset: Option<Address>,
    interest_amount: i128,
) -> Result<(i128, i128), ReserveError> {
    if interest_amount <= 0 {
        return Ok((0, 0));
    }

    // Get reserve factor
    let reserve_factor = get_reserve_factor(env, asset.clone());

    // Calculate reserve amount: interest * reserve_factor / 10000
    let reserve_amount = interest_amount
        .checked_mul(reserve_factor)
        .ok_or(ReserveError::Overflow)?
        .checked_div(BASIS_POINTS_SCALE)
        .ok_or(ReserveError::Overflow)?;

    // Calculate lender amount
    let lender_amount = interest_amount
        .checked_sub(reserve_amount)
        .ok_or(ReserveError::Overflow)?;

    // Update reserve balance
    let balance_key = ReserveDataKey::ReserveBalance(asset.clone());
    let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);

    let new_balance = current_balance
        .checked_add(reserve_amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage().persistent().set(&balance_key, &new_balance);

    // Emit event
    let topics = (Symbol::new(env, "reserve_accrued"),);
    env.events()
        .publish(topics, (asset, reserve_amount, new_balance));

    Ok((reserve_amount, lender_amount))
}

/// Get the current reserve balance for an asset
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset address (None for native asset)
///
/// # Returns
/// Current reserve balance
pub fn get_reserve_balance(env: &Env, asset: Option<Address>) -> i128 {
    let balance_key = ReserveDataKey::ReserveBalance(asset);
    env.storage().persistent().get(&balance_key).unwrap_or(0)
}

/// Set the treasury address (admin only)
///
/// Configures the destination address for reserve withdrawals.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `treasury` - The treasury address
///
/// # Errors
/// * `ReserveError::Unauthorized` - If caller is not admin
/// * `ReserveError::InvalidTreasury` - If treasury address is invalid
///
/// # Security
/// * Requires admin authorization
/// * Validates treasury address is not the contract itself
/// * Emits event for transparency
#[allow(deprecated)]
pub fn set_treasury_address(
    env: &Env,
    caller: Address,
    treasury: Address,
) -> Result<(), ReserveError> {
    // Require admin authorization
    caller.require_auth();
    require_admin(env, &caller)?;

    // Validate treasury address
    if treasury == env.current_contract_address() {
        return Err(ReserveError::InvalidTreasury);
    }

    // Set treasury address
    env.storage()
        .persistent()
        .set(&ReserveDataKey::TreasuryAddress, &treasury);

    // Emit event
    let topics = (Symbol::new(env, "treasury_address_set"), caller);
    env.events().publish(topics, treasury);

    Ok(())
}

/// Get the treasury address
///
/// # Arguments
/// * `env` - The Soroban environment
///
/// # Returns
/// Treasury address if set, None otherwise
pub fn get_treasury_address(env: &Env) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&ReserveDataKey::TreasuryAddress)
}

/// Withdraw reserves to treasury (admin only)
///
/// Transfers accrued protocol reserves to the treasury address. The withdrawal
/// amount is bounded by the available reserve balance and cannot access user funds.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address (must be admin)
/// * `asset` - The asset address (None for native asset)
/// * `amount` - Amount to withdraw
///
/// # Returns
/// Actual amount withdrawn
///
/// # Errors
/// * `ReserveError::Unauthorized` - If caller is not admin
/// * `ReserveError::TreasuryNotSet` - If treasury address not configured
/// * `ReserveError::InvalidAmount` - If amount <= 0
/// * `ReserveError::InsufficientReserve` - If amount > reserve balance
/// * `ReserveError::Overflow` - If arithmetic overflow occurs
///
/// # Security
/// * Requires admin authorization
/// * Validates treasury address is configured
/// * Validates withdrawal amount is positive and within bounds
/// * Updates reserve balance before transfer (checks-effects-interactions)
/// * Emits event for transparency
/// * Cannot withdraw user funds (only accrued reserves)
#[allow(deprecated)]
pub fn withdraw_reserve_funds(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, ReserveError> {
    // Require admin authorization
    caller.require_auth();
    require_admin(env, &caller)?;

    // Validate amount
    if amount <= 0 {
        return Err(ReserveError::InvalidAmount);
    }

    // Get treasury address
    let treasury = get_treasury_address(env).ok_or(ReserveError::TreasuryNotSet)?;

    // Get current reserve balance
    let balance_key = ReserveDataKey::ReserveBalance(asset.clone());
    let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);

    // Validate sufficient reserves
    if amount > current_balance {
        return Err(ReserveError::InsufficientReserve);
    }

    // Update reserve balance (checks-effects-interactions pattern)
    let new_balance = current_balance
        .checked_sub(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage().persistent().set(&balance_key, &new_balance);

    // Transfer tokens to treasury
    // Note: In production, this would call the token contract's transfer function
    // For now, we emit an event indicating the transfer should occur
    let topics = (Symbol::new(env, "reserve_withdrawn"), caller);
    env.events().publish(
        topics,
        (asset.clone(), treasury.clone(), amount, new_balance),
    );

    // Transfer tokens to treasury
    #[cfg(not(test))]
    {
        if let Some(ref asset_addr) = asset {
            let token_client = soroban_sdk::token::Client::new(env, asset_addr);
            token_client.transfer(&env.current_contract_address(), &treasury, &amount);
        }
    }

    Ok(amount)
}

/// Helper function to require admin authorization
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - The caller address to check
///
/// # Errors
/// * `ReserveError::Unauthorized` - If caller is not admin
fn require_admin(env: &Env, caller: &Address) -> Result<(), ReserveError> {
    let admin_key = DepositDataKey::Admin;
    let admin = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Address>(&admin_key)
        .ok_or(ReserveError::Unauthorized)?;

    if caller != &admin {
        return Err(ReserveError::Unauthorized);
    }

    Ok(())
}

/// Get reserve statistics for an asset
///
/// Returns comprehensive reserve information for reporting and analytics.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The asset address (None for native asset)
///
/// # Returns
/// Tuple of (reserve_balance, reserve_factor_bps, treasury_address)
pub fn get_reserve_stats(env: &Env, asset: Option<Address>) -> (i128, i128, Option<Address>) {
    let balance = get_reserve_balance(env, asset.clone());
    let factor = get_reserve_factor(env, asset);
    let treasury = get_treasury_address(env);

    (balance, factor, treasury)
}
