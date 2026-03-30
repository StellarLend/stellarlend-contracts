//! # Token Receiver Hook Implementation
//!
//! Handles incoming token transfers to the contract, enabling automatic
//! collateral deposits and repayments.

use crate::borrow::{deposit, repay, BorrowError};
use crate::pause::{self, PauseType};
use crate::reentrancy::ReentrancyGuard;
use soroban_sdk::{Address, Env, FromVal, Symbol, Val, Vec};

/// Token receiver hook for Soroban tokens (SAC compatible via manual dispatch).
///
/// This function is called by token contracts when funds are transferred.
/// It dispatches to deposit or repay logic based on the payload provided.
///
/// # Errors
/// - `BorrowError::Reentrancy`: If a nested call is detected.
/// - `BorrowError::Unauthorized`: If the token_asset does not match the caller.
/// - `BorrowError::ProtocolPaused`: If the requested operation is paused.
/// - `BorrowError::InvalidAmount`: If amount is <= 0 or payload is empty.
pub fn receive(
    env: Env,
    token_asset: Address,
    from: Address,
    amount: i128,
    payload: Vec<Val>,
) -> Result<(), BorrowError> {
    // 1. Authorization: Ensure the call is authorized by the token contract.
    // This confirms that token_asset is the actual caller in the invocation tree.
    token_asset.require_auth();

    // 2. Safety: Reject invalid amounts.
    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    // 3. Security: RAII Reentrancy Guard.
    let _guard = ReentrancyGuard::new(&env).map_err(|_| BorrowError::Reentrancy)?;

    // 4. Dispatch based on payload action.
    if payload.is_empty() {
        return Err(BorrowError::InvalidAmount);
    }

    let action = Symbol::from_val(&env, &payload.get(0).ok_or(BorrowError::InvalidAmount)?);

    if action == Symbol::new(&env, "deposit") {
        if pause::is_paused(&env, PauseType::Deposit) {
            return Err(BorrowError::ProtocolPaused);
        }
        deposit(&env, from, token_asset, amount)
    } else if action == Symbol::new(&env, "repay") {
        if pause::is_paused(&env, PauseType::Repay) {
            return Err(BorrowError::ProtocolPaused);
        }
        repay(&env, from, token_asset, amount)
    } else {
        Err(BorrowError::AssetNotSupported)
    }
}
