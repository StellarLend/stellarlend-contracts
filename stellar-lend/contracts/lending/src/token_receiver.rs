//! # Token Receiver Hook Implementation
//!
//! Handles incoming token transfers to the contract, enabling automatic
//! collateral deposits and repayments.

use crate::borrow::{deposit, repay, BorrowError};
use soroban_sdk::{Address, Env, FromVal, Symbol, Val, Vec};

/// Token receiver hook for Soroban tokens
///
/// This function is called by token contracts. It delegates to deposit or repay
/// logic based on the payload.
///
/// # Arguments
/// * `env` - The contract environment
/// * `token_asset` - The address of the token contract (should be the caller)
/// * `from` - The address that sent the tokens
/// * `amount` - The amount of tokens sent
/// * `payload` - A vector containing custom data (expected: [Symbol])
pub fn receive(
    env: Env,
    token_asset: Address,
    from: Address,
    amount: i128,
    payload: Vec<Val>,
) -> Result<(), BorrowError> {
    if payload.is_empty() {
        return Err(BorrowError::InvalidAmount);
    }

    let action = Symbol::from_val(&env, &payload.get(0).ok_or(BorrowError::InvalidAmount)?);

    if action == Symbol::new(&env, "deposit") {
        deposit(&env, from, token_asset, amount).map(|_| ())
    } else if action == Symbol::new(&env, "repay") {
        repay(&env, from, token_asset, amount)
    } else {
        Err(BorrowError::AssetNotSupported)
    }
}
