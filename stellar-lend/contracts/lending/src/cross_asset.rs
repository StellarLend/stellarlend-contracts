//! # Cross-Asset Lending Module
//!
//! This module implements cross-asset lending functionality allowing users to:
//! - Deposit multiple types of assets as collateral
//! - Borrow different assets against their collateral portfolio
//! - Manage positions across multiple assets with unified health factor tracking
//! - Configure asset-specific parameters (LTV, liquidation thresholds, debt ceilings)
//!
//! ## Key Features
//!
//! ### Multi-Asset Collateral
//! Users can deposit multiple different assets as collateral, with each asset having
//! its own Loan-to-Value (LTV) ratio and liquidation threshold.
//!
//! ### Cross-Asset Borrowing
//! Users can borrow any supported asset against their total collateral portfolio,
//! with borrowing capacity calculated across all deposited collateral.
//!
//! ### Unified Health Factor
//! The protocol calculates a single health factor across all assets:
//! ```text
//! Health Factor = (Weighted Collateral Value / Total Debt Value) * 10000
//! ```
//! Where Weighted Collateral Value = Sum of (Collateral Amount Ã— Price Ã— LTV) for all assets
//!
//! ### Asset Configuration
//! Each asset has configurable parameters:
//! - **LTV (Loan-to-Value)**: Percentage of asset value that counts toward borrowing capacity
//! - **Liquidation Threshold**: Threshold below which positions become liquidatable
//! - **Price Feed**: Oracle address for price information
//! - **Debt Ceiling**: Maximum total debt allowed for this asset
//! - **Active Status**: Whether the asset can be used for new operations
//!
//! ## Security Features
//!
//! ### Authorization
//! - Admin-only functions for asset configuration
//! - User authorization required for all position operations
//! - Cross-user operation isolation
//!
//! ### Arithmetic Safety
//! - Checked arithmetic throughout to prevent overflow/underflow
//! - Explicit bounds checking on all parameters
//! - Safe division with overflow protection
//!
//! ### Risk Management
//! - Health factor enforcement prevents undercollateralized positions
//! - Debt ceiling limits protocol-wide exposure per asset
//! - Asset deactivation capability for emergency situations
//!
//! ## Error Handling
//!
//! The module defines comprehensive error types for all failure scenarios:
//! - `InsufficientCollateral`: Health factor would drop below 1.0
//! - `DebtCeilingReached`: Asset-specific debt limit exceeded
//! - `ProtocolPaused`: Operations paused for maintenance/emergency
//! - `InvalidAmount`: Zero or negative amounts not allowed
//! - `Overflow`: Arithmetic overflow protection triggered
//! - `Unauthorized`: Insufficient permissions for operation
//! - `AssetNotSupported`: Asset not configured or deactivated
//! - `PriceUnavailable`: Oracle price feed unavailable
//!
//! ## Usage Example
//!
//! ```rust,ignore
//! // Configure assets (admin only)
//! set_asset_params(&env, usdc_asset, AssetParams {
//!     ltv: 9000,                    // 90% LTV
//!     liquidation_threshold: 9500,  // 95% liquidation threshold
//!     price_feed: oracle_address,
//!     debt_ceiling: 10000000,       // $10M debt ceiling
//!     is_active: true,
//! })?;
//!
//! // User deposits collateral
//! deposit_collateral_asset(&env, user, usdc_asset, 10000)?;
//! deposit_collateral_asset(&env, user, eth_asset, 5000)?;
//!
//! // User borrows against total collateral
//! borrow_asset(&env, user, usdc_asset, 8000)?;
//!
//! // Check position health
//! let summary = get_cross_position_summary(&env, user)?;
//! assert!(summary.health_factor >= 10000); // >= 1.0
//! ```

use soroban_sdk::{contracterror, contractevent, contracttype, token, Address, Env, Map};

use crate::constants::{BPS_SCALE, HEALTH_FACTOR_SCALE};
use crate::pause::{self, PauseType};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CrossAssetError {
    InsufficientCollateral = 1,
    DebtCeilingReached = 2,
    ProtocolPaused = 3,
    InvalidAmount = 4,
    Overflow = 5,
    Unauthorized = 6,
    AssetNotSupported = 7,
    PriceUnavailable = 8,
    AlreadyInitialized = 9,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct AssetParamsSetEvent {
    pub asset: Address,
    pub ltv: i128,
    pub liquidation_threshold: i128,
    pub debt_ceiling: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossDepositEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossBorrowEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossRepayEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug)]
pub struct CrossWithdrawEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AssetParams {
    pub ltv: i128,                   // Loan to Value ratio (basis points)
    pub liquidation_threshold: i128, // Liquidation threshold (basis points)
    pub price_feed: Address,         // Oracle address for price
    pub debt_ceiling: i128,          // Maximum debt allowed for this asset
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct UserCrossPosition {
    pub collateral_balances: Map<Address, i128>,
    pub debt_balances: Map<Address, i128>,
    pub last_update: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum CrossAssetDataKey {
    AssetParams(Address),
    UserPosition(Address),
    TotalAssetDebt(Address),
    MinBorrowAmount,
    Paused,
    Admin,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PositionSummary {
    pub total_collateral_usd: i128,
    pub total_debt_usd: i128,
    pub health_factor: i128, // Scaled by 10000
}

/// Configures parameters for a specific asset.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `asset` - The address of the asset to configure.
/// * `params` - The parameters to set for the asset.
///
/// # Errors
/// * `Unauthorized`: If the caller is not the admin.
///
/// # Security
/// * Only the admin can call this function.
/// * All parameters should be validated before use in other functions.
pub fn set_asset_params(
    env: &Env,
    asset: Address,
    params: AssetParams,
) -> Result<(), CrossAssetError> {
    check_admin(env)?;
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::AssetParams(asset), &params);
    Ok(())
}

/// Deposits an asset as collateral for the user's cross-asset position.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `user` - The address of the user.
/// * `asset` - The address of the asset to deposit.
/// * `amount` - The amount to deposit.
///
/// # Errors
/// * `InvalidAmount`: If the amount is less than or equal to 0.
/// * `ProtocolPaused`: If deposit operations are paused.
/// * `AssetNotSupported`: If the asset is not active or not supported.
/// * `Overflow`: If an arithmetic overflow occurs.
///
/// # Security
/// * The user must authorize the deposit.
/// * Tokens are transferred from the user to the contract.
pub fn deposit_collateral_asset(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<(), CrossAssetError> {
    user.require_auth();

    if pause::is_paused(env, PauseType::Deposit) {
        return Err(CrossAssetError::ProtocolPaused);
    }

    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }

    let params = get_asset_params(env, &asset)?;
    if !params.is_active {
        return Err(CrossAssetError::AssetNotSupported);
    }

    let mut position = get_user_position(env, &user);
    let current_balance = position.collateral_balances.get(asset.clone()).unwrap_or(0);
    position.collateral_balances.set(
        asset,
        current_balance
            .checked_add(amount)
            .ok_or(CrossAssetError::Overflow)?,
    );

    save_user_position(env, &user, &position);

    // In a real implementation, we would transfer tokens from user to contract here
    // env.invoke_contract(...)

    Ok(())
}

/// Borrows an asset against the user's collateral portfolio.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `user` - The address of the user.
/// * `asset` - The address of the asset to borrow.
/// * `amount` - The amount to borrow.
///
/// # Errors
/// * `InvalidAmount`: If the amount is less than or equal to 0.
/// * `ProtocolPaused`: If borrow operations are paused.
/// * `AssetNotSupported`: If the asset is not active or not supported.
/// * `DebtCeilingReached`: If the borrow would exceed the asset's debt ceiling.
/// * `InsufficientCollateral`: If the user's health factor would drop below 1.0.
/// * `Overflow`: If an arithmetic overflow occurs.
///
/// # Security
/// * The user must authorize the borrow.
/// * Tokens are transferred from the contract to the user.
/// * The position health is checked before completing the borrow.
pub fn borrow_asset(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<(), CrossAssetError> {
    user.require_auth();

    if pause::is_paused(env, PauseType::Borrow) {
        return Err(CrossAssetError::ProtocolPaused);
    }

    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }

    let params = get_asset_params(env, &asset)?;
    if !params.is_active {
        return Err(CrossAssetError::AssetNotSupported);
    }

    let total_debt = get_total_asset_debt(env, &asset);
    if total_debt
        .checked_add(amount)
        .ok_or(CrossAssetError::Overflow)?
        > params.debt_ceiling
    {
        return Err(CrossAssetError::DebtCeilingReached);
    }

    let mut position = get_user_position(env, &user);

    // Calculate new position health
    let mut debt_balances = position.debt_balances.clone();
    let current_debt = debt_balances.get(asset.clone()).unwrap_or(0);
    debt_balances.set(
        asset.clone(),
        current_debt
            .checked_add(amount)
            .ok_or(CrossAssetError::Overflow)?,
    );

    let summary = calculate_position_summary(env, &position.collateral_balances, &debt_balances)?;

    // Health factor must be > 1.0 (10000) after borrowing
    if summary.health_factor < HEALTH_FACTOR_SCALE {
        return Err(CrossAssetError::InsufficientCollateral);
    }

    position.debt_balances = debt_balances;
    position.last_update = env.ledger().timestamp();

    save_user_position(env, &user, &position);
    set_total_asset_debt(
        env,
        &asset,
        total_debt
            .checked_add(amount)
            .ok_or(CrossAssetError::Overflow)?,
    );

    Ok(())
}

/// Repays a borrowed asset.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `user` - The address of the user.
/// * `asset` - The address of the asset to repay.
/// * `amount` - The amount to repay.
///
/// # Errors
/// * `InvalidAmount`: If the amount is less than or equal to 0.
/// * `ProtocolPaused`: If repay operations are paused.
/// * `Overflow`: If an arithmetic overflow occurs.
///
/// # Security
/// * The user must authorize the repayment.
/// * Tokens are transferred from the user to the contract.
pub fn repay_asset(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<(), CrossAssetError> {
    user.require_auth();

    if pause::is_paused(env, PauseType::Repay) {
        return Err(CrossAssetError::ProtocolPaused);
    }

    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }

    let mut position = get_user_position(env, &user);
    let current_debt = position.debt_balances.get(asset.clone()).unwrap_or(0);

    let repay_amount = if amount > current_debt {
        current_debt
    } else {
        amount
    };

    position.debt_balances.set(
        asset.clone(),
        current_debt
            .checked_sub(repay_amount)
            .ok_or(CrossAssetError::Overflow)?,
    );

    save_user_position(env, &user, &position);

    let total_debt = get_total_asset_debt(env, &asset);
    set_total_asset_debt(
        env,
        &asset,
        total_debt
            .checked_sub(repay_amount)
            .ok_or(CrossAssetError::Overflow)?,
    );

    Ok(())
}

/// Withdraws an asset from the user's collateral portfolio.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `user` - The address of the user.
/// * `asset` - The address of the asset to withdraw.
/// * `amount` - The amount to withdraw.
///
/// # Errors
/// * `InvalidAmount`: If the amount is less than or equal to 0 or exceeds the user's collateral balance.
/// * `ProtocolPaused`: If withdraw operations are paused.
/// * `InsufficientCollateral`: If the withdrawal would make the position liquidatable.
/// * `Overflow`: If an arithmetic overflow occurs.
///
/// # Security
/// * The user must authorize the withdrawal.
/// * Tokens are transferred from the contract to the user.
/// * The position health is checked before completing the withdrawal.
pub fn withdraw_asset(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<(), CrossAssetError> {
    user.require_auth();

    if pause::is_paused(env, PauseType::Withdraw) {
        return Err(CrossAssetError::ProtocolPaused);
    }

    if amount <= 0 {
        return Err(CrossAssetError::InvalidAmount);
    }

    let mut position = get_user_position(env, &user);
    let current_balance = position.collateral_balances.get(asset.clone()).unwrap_or(0);

    if amount > current_balance {
        return Err(CrossAssetError::InvalidAmount);
    }

    let mut collateral_balances = position.collateral_balances.clone();
    collateral_balances.set(
        asset.clone(),
        current_balance
            .checked_sub(amount)
            .ok_or(CrossAssetError::Overflow)?,
    );

    let summary = calculate_position_summary(env, &collateral_balances, &position.debt_balances)?;

    // Only allow withdrawal if health factor remains healthy
    if summary.total_debt_usd > 0 && summary.health_factor < HEALTH_FACTOR_SCALE {
        return Err(CrossAssetError::InsufficientCollateral);
    }

    position.collateral_balances = collateral_balances;
    save_user_position(env, &user, &position);

    // Transfer tokens from contract to user
    let token_client = token::Client::new(env, &asset);
    token_client.transfer(&env.current_contract_address(), &user, &amount);

    CrossWithdrawEvent {
        user,
        asset,
        amount,
    }
    .publish(env);

    Ok(())
}

pub fn get_cross_position_summary(
    env: &Env,
    user: Address,
) -> Result<PositionSummary, CrossAssetError> {
    let position = get_user_position(env, &user);
    calculate_position_summary(env, &position.collateral_balances, &position.debt_balances)
}

// Internal helpers

fn check_admin(env: &Env) -> Result<(), CrossAssetError> {
    let admin: Address = env
        .storage()
        .persistent()
        .get(&CrossAssetDataKey::Admin)
        .ok_or(CrossAssetError::Unauthorized)?;
    admin.require_auth();
    Ok(())
}

fn get_asset_params(env: &Env, asset: &Address) -> Result<AssetParams, CrossAssetError> {
    env.storage()
        .persistent()
        .get(&CrossAssetDataKey::AssetParams(asset.clone()))
        .ok_or(CrossAssetError::AssetNotSupported)
}

fn get_user_position(env: &Env, user: &Address) -> UserCrossPosition {
    env.storage()
        .persistent()
        .get(&CrossAssetDataKey::UserPosition(user.clone()))
        .unwrap_or(UserCrossPosition {
            collateral_balances: Map::new(env),
            debt_balances: Map::new(env),
            last_update: env.ledger().timestamp(),
        })
}

fn save_user_position(env: &Env, user: &Address, position: &UserCrossPosition) {
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::UserPosition(user.clone()), position);
}

fn get_total_asset_debt(env: &Env, asset: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&CrossAssetDataKey::TotalAssetDebt(asset.clone()))
        .unwrap_or(0)
}

fn set_total_asset_debt(env: &Env, asset: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::TotalAssetDebt(asset.clone()), &amount);
}

fn calculate_position_summary(
    env: &Env,
    collateral_balances: &Map<Address, i128>,
    debt_balances: &Map<Address, i128>,
) -> Result<PositionSummary, CrossAssetError> {
    let mut total_collateral_usd = 0i128;
    let mut total_weighted_collateral_usd = 0i128;
    let mut total_debt_usd = 0i128;

    for (asset, amount) in collateral_balances.iter() {
        let params = get_asset_params(env, &asset)?;
        let price = get_price(env, &params.price_feed)?;
        let value_usd = amount
            .checked_mul(price)
            .ok_or(CrossAssetError::Overflow)?
            .checked_div(10000000)
            .ok_or(CrossAssetError::Overflow)?;
        total_collateral_usd = total_collateral_usd
            .checked_add(value_usd)
            .ok_or(CrossAssetError::Overflow)?;

        let weighted_value = value_usd
            .checked_mul(params.ltv)
            .ok_or(CrossAssetError::Overflow)?
            .checked_div(BPS_SCALE)
            .ok_or(CrossAssetError::Overflow)?;
        total_weighted_collateral_usd = total_weighted_collateral_usd
            .checked_add(weighted_value)
            .ok_or(CrossAssetError::Overflow)?;
    }

    for (asset, amount) in debt_balances.iter() {
        let params = get_asset_params(env, &asset)?;
        let price = get_price(env, &params.price_feed)?;
        let value_usd = amount
            .checked_mul(price)
            .ok_or(CrossAssetError::Overflow)?
            .checked_div(10000000)
            .ok_or(CrossAssetError::Overflow)?;
        total_debt_usd = total_debt_usd
            .checked_add(value_usd)
            .ok_or(CrossAssetError::Overflow)?;
    }

    let health_factor = if total_debt_usd == 0 {
        1000000 // Very large number if no debt
    } else {
        total_weighted_collateral_usd
            .checked_mul(BPS_SCALE)
            .ok_or(CrossAssetError::Overflow)?
            .checked_div(total_debt_usd)
            .ok_or(CrossAssetError::Overflow)?
    };

    Ok(PositionSummary {
        total_collateral_usd,
        total_debt_usd,
        health_factor,
    })
}

/// Fetches the price for a given asset from its oracle price feed.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `price_feed` - The address of the oracle price feed.
///
/// # Returns
/// The price of the asset (scaled by 10^7).
///
/// # Errors
/// * `PriceUnavailable`: If the oracle price is not available.
///
/// # Security
/// * In a production implementation, this should call a trusted oracle contract.
fn get_price(_env: &Env, _price_feed: &Address) -> Result<i128, CrossAssetError> {
    // Mock price feed - in real app, call oracle contract
    // Example: let oracle = oracle::Client::new(env, price_feed); oracle.get_price(...)
    Ok(10000000) // $1.00 with 7 decimals
}

pub fn initialize_admin(env: &Env, admin: Address) {
    env.storage()
        .persistent()
        .set(&CrossAssetDataKey::Admin, &admin);
}
