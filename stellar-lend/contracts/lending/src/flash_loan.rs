use soroban_sdk::{contracterror, contracttype, token, Address, Bytes, Env, IntoVal, Symbol};

use crate::constants::{BPS_SCALE, MAX_FLASH_LOAN_FEE_BPS};

/// Errors that can occur during flash loan operations
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FlashLoanError {
    InvalidAmount = 1,
    InsufficientRepayment = 2,
    Unauthorized = 3,
    InvalidFee = 4,
    CallbackFailed = 5,
    Reentrancy = 6,
    ProtocolPaused = 7,
}

/// Storage keys for flash loan data
#[contracttype]
#[derive(Clone)]
pub enum FlashLoanDataKey {
    FlashLoanFeeBps,
    ReentrancyGuard,
}

const MAX_FEE_BPS: i128 = MAX_FLASH_LOAN_FEE_BPS; // 10% maximum fee

/// Initiate a flash loan
///
/// # Arguments
/// * `env` - The contract environment
/// * `receiver` - The address of the contract receiving the funds and implementing the callback
/// * `asset` - The address of the token to borrow
/// * `amount` - The amount to borrow
/// * `params` - Arbitrary data to pass to the receiver's callback
pub fn flash_loan(
    env: &Env,
    receiver: Address,
    asset: Address,
    amount: i128,
    params: Bytes,
) -> Result<(), FlashLoanError> {
    if amount <= 0 {
        return Err(FlashLoanError::InvalidAmount);
    }

    let guard_key = FlashLoanDataKey::ReentrancyGuard;
    if env.storage().instance().get(&guard_key).unwrap_or(false) {
        return Err(FlashLoanError::Reentrancy);
    }
    env.storage().instance().set(&guard_key, &true);

    let fee = calculate_fee(env, amount);

    // 0. Record initial balance
    let token_client = token::Client::new(env, &asset);
    let initial_balance = token_client.balance(&env.current_contract_address());

    // 1. Transfer funds to the receiver
    token_client.transfer(&env.current_contract_address(), &receiver, &amount);

    // 2. Execute callback on receiver
    let callback_result: bool = env.invoke_contract(
        &receiver,
        &Symbol::new(env, "on_flash_loan"),
        (
            env.current_contract_address(),
            asset.clone(),
            amount,
            fee,
            params,
        )
            .into_val(env),
    );

    if !callback_result {
        env.storage().instance().set(&guard_key, &false);
        return Err(FlashLoanError::CallbackFailed);
    }

    // 3. Verify repayment
    let final_balance = token_client.balance(&env.current_contract_address());

    // Clear the reentrancy guard
    env.storage().instance().set(&guard_key, &false);

    if final_balance < initial_balance + fee {
        return Err(FlashLoanError::InsufficientRepayment);
    }

    Ok(())
}

/// Calculate the fee for a flash loan
fn calculate_fee(env: &Env, amount: i128) -> i128 {
    let fee_bps = get_flash_loan_fee_bps(env);
    amount.saturating_mul(fee_bps).saturating_div(BPS_SCALE)
}

/// Set the flash loan fee in basis points
pub fn set_flash_loan_fee_bps(env: &Env, fee_bps: i128) -> Result<(), FlashLoanError> {
    if !(0..=MAX_FEE_BPS).contains(&fee_bps) {
        return Err(FlashLoanError::InvalidFee);
    }
    env.storage()
        .persistent()
        .set(&FlashLoanDataKey::FlashLoanFeeBps, &fee_bps);
    Ok(())
}

/// Get the current flash loan fee in basis points
pub fn get_flash_loan_fee_bps(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&FlashLoanDataKey::FlashLoanFeeBps)
        .unwrap_or(5) // Default 5 bps (0.05%)
}
