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
//! - Tracked separately per asset in persistent storage under `ReserveDataKey::ReserveBalance`
//!
//! ### Treasury Withdrawal
//! - Admin can withdraw accrued reserves to a pre-configured treasury address
//! - Withdrawals are bounded by the actual reserve balance
//! - Cannot withdraw user funds (collateral or principal)
//! - All withdrawals are logged via events
//! - Follows checks-effects-interactions: balance updated before token transfer
//!
//! ## Storage Layout
//! - `ReserveBalance(asset)` — accumulated reserve per asset (canonical key)
//! - `ReserveFactor(asset)` — reserve factor per asset (basis points)
//! - `TreasuryAddress` — destination address for reserve withdrawals
//!
//! ## Security Invariants
//! - Reserve factor must be between 0 and 5000 bps (0% - 50%)
//! - Only admin can modify reserve factors or withdraw reserves
//! - Withdrawals cannot exceed accrued reserve balance
//! - User funds (collateral, principal) are never accessible via treasury operations
//! - Reserve balance is updated before any external token transfer (CEI pattern)
//! - Withdrawals are blocked when the reserve-withdraw pause switch is active
//! - Treasury address cannot be the contract itself (prevents self-draining)
//! - All state changes emit events for transparency and auditability

#![allow(unused)]
use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};
use crate::prelude::*;
use crate::prelude::*;

use crate::deposit::{DepositDataKey, ProtocolAnalytics};

/// Maximum allowed reserve factor (50% = 5000 basis points).
/// Ensures at least 50% of interest always flows to lenders.
pub const MAX_RESERVE_FACTOR_BPS: i128 = 5000;

/// Default reserve factor (10% = 1000 basis points).
pub const DEFAULT_RESERVE_FACTOR_BPS: i128 = 1000;

/// Basis points scale (100% = 10000 basis points).
pub const BASIS_POINTS_SCALE: i128 = 10_000;

/// Pause-switch key used to block reserve withdrawals.
const PAUSE_RESERVE_WITHDRAW: &str = "pause_reserve";

/// Errors that can occur during reserve and treasury operations.
///
/// Error codes are stable and must never be renumbered; append new variants at the end.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ReserveError {
    /// Caller is not authorized (not admin)
    Unauthorized = 1,
    /// Reserve factor exceeds maximum allowed value (> 5000 bps)
    InvalidReserveFactor = 2,
    /// Withdrawal amount exceeds available reserve balance
    InsufficientReserve = 3,
    /// Invalid or unsupported asset address
    InvalidAsset = 4,
    /// Treasury address is invalid (e.g. equals the contract itself)
    InvalidTreasury = 5,
    /// Withdrawal amount must be greater than zero
    InvalidAmount = 6,
    /// Arithmetic overflow in reserve calculations
    Overflow = 7,
    /// Treasury address not configured before withdrawal
    TreasuryNotSet = 8,
    /// Reserve withdraw operations are currently paused
    ReserveWithdrawPaused = 9,
}

/// Storage keys for reserve and treasury data.
///
/// `ReserveBalance` is the **canonical** per-asset reserve counter; all other
/// modules (repay, lib.rs) must read/write through this module's helpers.
#[contracttype]
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ReserveDataKey {
    /// Accumulated protocol reserve per asset → i128
    ReserveBalance(Option<Address>),
    /// Reserve factor per asset in basis points → i128
    ReserveFactor(Option<Address>),
    /// Destination address for admin reserve withdrawals → Address
    TreasuryAddress,
    /// Aggregate reserve balance across all assets.
    /// Value type: i128
    TotalReservesV1,
    /// Cumulative protocol revenue sourced from reserve accruals.
    /// Value type: i128
    ProtocolRevenueV1,
}

fn add_to_protocol_tvl(env: &Env, amount: i128) -> Result<(), ReserveError> {
    let mut analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, ProtocolAnalytics>(&DepositDataKey::ProtocolAnalytics)
        .unwrap_or(ProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    analytics.total_value_locked = analytics
        .total_value_locked
        .checked_add(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage()
        .persistent()
        .set(&DepositDataKey::ProtocolAnalytics, &analytics);

    Ok(())
}

fn sub_from_protocol_tvl(env: &Env, amount: i128) -> Result<(), ReserveError> {
    let mut analytics = env
        .storage()
        .persistent()
        .get::<DepositDataKey, ProtocolAnalytics>(&DepositDataKey::ProtocolAnalytics)
        .unwrap_or(ProtocolAnalytics {
            total_deposits: 0,
            total_borrows: 0,
            total_value_locked: 0,
        });

    analytics.total_value_locked = analytics
        .total_value_locked
        .checked_sub(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage()
        .persistent()
        .set(&DepositDataKey::ProtocolAnalytics, &analytics);

    Ok(())
}

fn add_to_total_reserves(env: &Env, amount: i128) -> Result<(), ReserveError> {
    let current_total = env
        .storage()
        .persistent()
        .get::<ReserveDataKey, i128>(&ReserveDataKey::TotalReservesV1)
        .unwrap_or(0);

    let next_total = current_total
        .checked_add(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage()
        .persistent()
        .set(&ReserveDataKey::TotalReservesV1, &next_total);

    Ok(())
}

fn sub_from_total_reserves(env: &Env, amount: i128) -> Result<(), ReserveError> {
    let current_total = env
        .storage()
        .persistent()
        .get::<ReserveDataKey, i128>(&ReserveDataKey::TotalReservesV1)
        .unwrap_or(0);

    let next_total = current_total
        .checked_sub(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage()
        .persistent()
        .set(&ReserveDataKey::TotalReservesV1, &next_total);

    Ok(())
}

fn add_to_protocol_revenue(env: &Env, amount: i128) -> Result<(), ReserveError> {
    let current_revenue = env
        .storage()
        .persistent()
        .get::<ReserveDataKey, i128>(&ReserveDataKey::ProtocolRevenueV1)
        .unwrap_or(0);

    let next_revenue = current_revenue
        .checked_add(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ProtocolRevenueV1, &next_revenue);

    Ok(())
}

// ─── Initialisation ──────────────────────────────────────────────────────────

/// Initialize reserve configuration for a new asset.
///
/// Sets the initial reserve factor and zeroes the reserve balance. Should be
/// called once per asset when the asset is registered with the protocol.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - Asset address (`None` for native XLM)
/// * `reserve_factor_bps` - Initial reserve factor in basis points (0–5000)
///
/// # Errors
/// * [`ReserveError::InvalidReserveFactor`] — factor outside `[0, MAX_RESERVE_FACTOR_BPS]`
///
/// # Security
/// No authorization check — intended for internal use during asset registration.
pub fn initialize_reserve_config(
    env: &Env,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError> {
    // Validate reserve factor
    if !(0..=MAX_RESERVE_FACTOR_BPS).contains(&reserve_factor_bps) {
        return Err(ReserveError::InvalidReserveFactor);
    }

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ReserveFactor(asset.clone()), &reserve_factor_bps);

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ReserveBalance(asset.clone()), &0i128);

    let topics = (Symbol::new(env, "reserve_initialized"),);
    env.events().publish(topics, (asset, reserve_factor_bps));

    Ok(())
}

// ─── Reserve Factor ───────────────────────────────────────────────────────────

/// Update the reserve factor for an asset (admin only).
///
/// The new factor applies only to **future** interest accruals; existing reserve
/// balances are never modified retroactively.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - Must be the protocol admin
/// * `asset` - Asset address (`None` for native XLM)
/// * `reserve_factor_bps` - New factor in basis points (0–5000)
///
/// # Errors
/// * [`ReserveError::Unauthorized`] — caller is not admin
/// * [`ReserveError::InvalidReserveFactor`] — factor outside valid range
///
/// # Security
/// * Requires Soroban `require_auth` + storage admin verification
/// * Emits `reserve_factor_updated` event for off-chain auditability
pub fn set_reserve_factor(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError> {
    caller.require_auth();
    require_admin(env, &caller)?;

    // Validate reserve factor
    if !(0..=MAX_RESERVE_FACTOR_BPS).contains(&reserve_factor_bps) {
        return Err(ReserveError::InvalidReserveFactor);
    }

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ReserveFactor(asset.clone()), &reserve_factor_bps);

    let topics = (Symbol::new(env, "reserve_factor_updated"), caller);
    env.events().publish(topics, (asset, reserve_factor_bps));

    Ok(())
}

/// Return the current reserve factor for an asset.
///
/// Falls back to [`DEFAULT_RESERVE_FACTOR_BPS`] if no factor has been configured.
///
/// # Returns
/// Reserve factor in basis points (0–5000).
pub fn get_reserve_factor(env: &Env, asset: Option<Address>) -> i128 {
    env.storage()
        .persistent()
        .get(&ReserveDataKey::ReserveFactor(asset))
        .unwrap_or(DEFAULT_RESERVE_FACTOR_BPS)
}

// ─── Accrual ──────────────────────────────────────────────────────────────────

/// Accrue protocol reserves from an interest payment.
///
/// Called by the repay module whenever a borrower pays interest. Splits
/// `interest_amount` between the protocol reserve and lenders according to the
/// asset's current reserve factor.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - Asset address (`None` for native XLM)
/// * `interest_amount` - Total interest paid in this repayment
///
/// # Returns
/// `(reserve_amount, lender_amount)` — the interest split.
///
/// # Errors
/// * [`ReserveError::Overflow`] — arithmetic overflow detected
///
/// # Security
/// * No authorization check — internal function, called only from repay module
/// * All arithmetic is checked; never panics on overflow
///
/// # Formula
/// ```text
/// reserve_amount = interest_amount × reserve_factor ÷ 10_000
/// lender_amount  = interest_amount − reserve_amount
/// ```
pub fn accrue_reserve(
    env: &Env,
    asset: Option<Address>,
    interest_amount: i128,
) -> Result<(i128, i128), ReserveError> {
    if interest_amount <= 0 {
        return Ok((0, 0));
    }

    let reserve_factor = get_reserve_factor(env, asset.clone());

    let reserve_amount = interest_amount
        .checked_mul(reserve_factor)
        .ok_or(ReserveError::Overflow)?
        .checked_div(BASIS_POINTS_SCALE)
        .ok_or(ReserveError::Overflow)?;

    let lender_amount = interest_amount
        .checked_sub(reserve_amount)
        .ok_or(ReserveError::Overflow)?;

    let balance_key = ReserveDataKey::ReserveBalance(asset.clone());
    let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);

    let new_balance = current_balance
        .checked_add(reserve_amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage().persistent().set(&balance_key, &new_balance);
    add_to_total_reserves(env, reserve_amount)?;
    add_to_protocol_revenue(env, reserve_amount)?;
    add_to_protocol_tvl(env, reserve_amount)?;

    let topics = (Symbol::new(env, "reserve_accrued"),);
    env.events()
        .publish(topics, (asset, reserve_amount, new_balance));

    Ok((reserve_amount, lender_amount))
}

// ─── Balance Query ────────────────────────────────────────────────────────────

/// Return the current reserve balance for an asset.
///
/// # Returns
/// Accumulated reserve balance (0 if never accrued).
pub fn get_reserve_balance(env: &Env, asset: Option<Address>) -> i128 {
    env.storage()
        .persistent()
        .get(&ReserveDataKey::ReserveBalance(asset))
        .unwrap_or(0)
}

/// Get aggregate reserves across all assets.
///
/// # Returns
/// Total reserves currently held by the protocol.
pub fn get_total_reserves(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<ReserveDataKey, i128>(&ReserveDataKey::TotalReservesV1)
        .unwrap_or(0)
}

/// Get cumulative protocol revenue from reserve accrual.
///
/// This is cumulative protocol income and is not decreased by treasury withdrawals.
///
/// # Returns
/// Total protocol revenue from reserve accruals.
pub fn get_protocol_revenue(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<ReserveDataKey, i128>(&ReserveDataKey::ProtocolRevenueV1)
        .unwrap_or(0)
}

/// Set the treasury address (admin only)
///
/// All future `withdraw_reserve_funds` calls transfer tokens to this address.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - Must be the protocol admin
/// * `treasury` - New treasury address
///
/// # Errors
/// * [`ReserveError::Unauthorized`] — caller is not admin
/// * [`ReserveError::InvalidTreasury`] — treasury equals the contract address
///
/// # Security
/// * Prevents self-draining by rejecting the contract's own address as treasury
pub fn set_treasury_address(
    env: &Env,
    caller: Address,
    treasury: Address,
) -> Result<(), ReserveError> {
    caller.require_auth();
    require_admin(env, &caller)?;

    if treasury == env.current_contract_address() {
        return Err(ReserveError::InvalidTreasury);
    }

    env.storage()
        .persistent()
        .set(&ReserveDataKey::TreasuryAddress, &treasury);

    let topics = (Symbol::new(env, "treasury_address_set"), caller);
    env.events().publish(topics, treasury);

    Ok(())
}

/// Return the configured treasury address, if any.
pub fn get_treasury_address(env: &Env) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&ReserveDataKey::TreasuryAddress)
}

// ─── Withdrawal ───────────────────────────────────────────────────────────────

/// Transfer accrued reserves to the treasury address (admin only).
///
/// Follows the **checks-effects-interactions** pattern: the reserve balance is
/// decremented in storage before the external token transfer is initiated, so a
/// reentrant call would see an already-reduced balance.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `caller` - Must be the protocol admin
/// * `asset` - Asset to withdraw (`None` for native XLM)
/// * `amount` - Amount to transfer to treasury
///
/// # Returns
/// The amount actually withdrawn.
///
/// # Errors
/// * [`ReserveError::Unauthorized`] — caller is not admin
/// * [`ReserveError::ReserveWithdrawPaused`] — withdraw switch is active
/// * [`ReserveError::TreasuryNotSet`] — no treasury address configured
/// * [`ReserveError::InvalidAmount`] — amount ≤ 0
/// * [`ReserveError::InsufficientReserve`] — amount exceeds reserve balance
/// * [`ReserveError::InvalidAsset`] — native asset address not configured (non-test)
/// * [`ReserveError::Overflow`] — arithmetic overflow
///
/// # Security
/// * Admin auth + storage admin check required
/// * Pause switch `"pause_reserve"` blocks all withdrawals when active
/// * Balance updated **before** token transfer (CEI pattern)
/// * Native asset address resolved from protocol storage (not caller-supplied)
pub fn withdraw_reserve_funds(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, ReserveError> {
    // ── CHECKS ────────────────────────────────────────────────────────────────
    caller.require_auth();
    require_admin(env, &caller)?;

    // Respect the reserve-withdraw pause switch.
    let pause_key = DepositDataKey::PauseSwitches;
    if let Some(pause_map) = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Map<Symbol, bool>>(&pause_key)
    {
        if pause_map
            .get(Symbol::new(env, PAUSE_RESERVE_WITHDRAW))
            .unwrap_or(false)
        {
            return Err(ReserveError::ReserveWithdrawPaused);
        }
    }

    if amount <= 0 {
        return Err(ReserveError::InvalidAmount);
    }

    let treasury = get_treasury_address(env).ok_or(ReserveError::TreasuryNotSet)?;

    let balance_key = ReserveDataKey::ReserveBalance(asset.clone());
    let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);

    if amount > current_balance {
        return Err(ReserveError::InsufficientReserve);
    }

    // ── EFFECTS ───────────────────────────────────────────────────────────────
    let new_balance = current_balance
        .checked_sub(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage().persistent().set(&balance_key, &new_balance);
    sub_from_total_reserves(env, amount)?;
    sub_from_protocol_tvl(env, amount)?;

    // ── INTERACTIONS ──────────────────────────────────────────────────────────
    let topics = (Symbol::new(env, "reserve_withdrawn"), caller);
    env.events()
        .publish(topics, (asset.clone(), treasury.clone(), amount, new_balance));

    // Resolve the effective token contract address: use the supplied address for
    // SRC-20 tokens, or fall back to the stored native-asset address for XLM.
    #[cfg(not(test))]
    {
        let effective_addr: Address = match &asset {
            Some(addr) => addr.clone(),
            None => env
                .storage()
                .persistent()
                .get::<DepositDataKey, Address>(&DepositDataKey::NativeAssetAddress)
                .ok_or(ReserveError::InvalidAsset)?,
        };
        let token_client = soroban_sdk::token::Client::new(env, &effective_addr);
        token_client.transfer(&env.current_contract_address(), &treasury, &amount);
    }

    Ok(amount)
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), ReserveError> {
    crate::admin::require_admin(env, caller).map_err(|_| ReserveError::Unauthorized)
}
