use crate::pause::{self, PauseType};
use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env};

/// Errors that can occur during deposit operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DepositError {
    InvalidAmount = 1,
    DepositPaused = 2,
    Overflow = 3,
    AssetNotSupported = 4,
    ExceedsDepositCap = 5,
    Unauthorized = 6,
    Reentrancy = 7,
}

/// Storage keys for deposit-related data
#[contracttype]
#[derive(Clone)]
#[allow(clippy::enum_variant_names)]
pub enum DepositDataKey {
    UserCollateral(Address),
    TotalAmount,
    CapAmount,
    MinAmount,
}

/// User deposit position
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct DepositCollateral {
    pub amount: i128,
    pub asset: Address,
    pub last_deposit_time: u64,
}

/// Deposit event data
#[contractevent]
#[derive(Clone, Debug)]
pub struct DepositEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub new_balance: i128,
    pub timestamp: u64,
}

use crate::reentrancy::ReentrancyGuard;
use soroban_sdk::token;

/// Deposit collateral into the protocol
///
/// # Arguments
/// * `env` - The contract environment
/// * `user` - The depositor's address
/// * `asset` - The collateral asset address
/// * `amount` - The amount to deposit
///
/// # Returns
/// Returns the updated collateral balance on success
///
/// # Security
/// - Requires user authorization to ensure intent and allow token transfer.
/// - Implements reentrancy protection against synchronous hook callbacks.
/// - Enforces asset consistency for the user's single-asset position in this module.
pub fn deposit(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<i128, DepositError> {
    // 1. Authorization: Verify user intent. User must sign to allow transfer_from.
    user.require_auth();

    // 2. State: Check pause status.
    if pause::is_paused(env, PauseType::Deposit) {
        return Err(DepositError::DepositPaused);
    }

    // 3. Security: RAII Reentrancy Guard.
    let _guard = ReentrancyGuard::new(env).map_err(|_| DepositError::Reentrancy)?;

    // 4. Validation: amount must be positive and meet minimum.
    if amount <= 0 {
        return Err(DepositError::InvalidAmount);
    }

    let min_deposit = get_min_deposit_amount(env);
    if amount < min_deposit {
        return Err(DepositError::InvalidAmount);
    }

    // 5. Validation: Protocol caps.
    let total_deposits = get_total_deposits(env);
    let deposit_cap = get_deposit_cap(env);
    let new_total = total_deposits
        .checked_add(amount)
        .ok_or(DepositError::Overflow)?;

    if new_total > deposit_cap {
        return Err(DepositError::ExceedsDepositCap);
    }

    // 6. Validating/Updating User Position.
    let mut position = get_deposit_position(env, &user, &asset);
    
    // Enforcement: This module's simple position model only supports one primary asset.
    if position.amount > 0 && position.asset != asset {
        return Err(DepositError::AssetNotSupported);
    }

    position.amount = position
        .amount
        .checked_add(amount)
        .ok_or(DepositError::Overflow)?;
    position.last_deposit_time = env.ledger().timestamp();
    position.asset = asset.clone();

    // 7. Execution: Move the physical tokens (SAC Transfer).
    // The user has authorized this contract to call transfer on their behalf in the token contract.
    let token_client = token::Client::new(env, &asset);
    token_client.transfer(&user, env.current_contract_address(), &amount);

    // 8. Storage: Update accounting.
    save_deposit_position(env, &user, &position);
    set_total_deposits(env, new_total);
    emit_deposit_event(env, user, asset, amount, position.amount);

    Ok(position.amount)
}

/// Initialize deposit settings
pub fn initialize_deposit_settings(
    env: &Env,
    deposit_cap: i128,
    min_deposit_amount: i128,
) -> Result<(), DepositError> {
    env.storage()
        .persistent()
        .set(&DepositDataKey::CapAmount, &deposit_cap);
    env.storage()
        .persistent()
        .set(&DepositDataKey::MinAmount, &min_deposit_amount);
    Ok(())
}

pub fn get_user_collateral(env: &Env, user: &Address, asset: &Address) -> DepositCollateral {
    get_deposit_position(env, user, asset)
}

fn get_deposit_position(env: &Env, user: &Address, asset: &Address) -> DepositCollateral {
    env.storage()
        .persistent()
        .get(&DepositDataKey::UserCollateral(user.clone()))
        .unwrap_or(DepositCollateral {
            amount: 0,
            asset: asset.clone(),
            last_deposit_time: env.ledger().timestamp(),
        })
}

fn save_deposit_position(env: &Env, user: &Address, position: &DepositCollateral) {
    env.storage()
        .persistent()
        .set(&DepositDataKey::UserCollateral(user.clone()), position);
}

fn get_total_deposits(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DepositDataKey::TotalAmount)
        .unwrap_or(0)
}

fn set_total_deposits(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&DepositDataKey::TotalAmount, &amount);
}

fn get_deposit_cap(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DepositDataKey::CapAmount)
        .unwrap_or(i128::MAX)
}

fn get_min_deposit_amount(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DepositDataKey::MinAmount)
        .unwrap_or(0)
}

fn emit_deposit_event(env: &Env, user: Address, asset: Address, amount: i128, new_balance: i128) {
    DepositEvent {
        user,
        asset,
        amount,
        new_balance,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);
}
