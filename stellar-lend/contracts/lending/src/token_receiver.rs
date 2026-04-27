//! # Token Receiver Implementation
//!
//! Provides a token-aware entrypoint for collateral deposits and debt
//! repayments.
//!
//! ## Security model
//! - The caller must be the user whose balance is being debited.
//! - The user must have approved the lending contract as a token spender.
//! - The contract validates pause state *before* pulling funds.
//! - Funds are transferred with `transfer_from`, then the internal lending
//!   state is updated.
//!
//! This matches the Soroban token interface exposed in this repository, which
//! supports `approve` and `transfer_from` but not a standard authenticated
//! `transfer_call`/receiver-hook flow.

use crate::{
    borrow::{deposit, repay, BorrowError},
    pause::{self, blocks_high_risk_ops, PauseType},
};
use soroban_sdk::{token, Address, Env, FromVal, Symbol, Val, Vec};

/// Token-aware receive entrypoint for Soroban tokens.
///
/// The entrypoint expects the caller to be the token owner (`from`). The owner
/// must authorize the call and pre-approve the lending contract to spend at
/// least `amount` of `token_asset`. The contract then pulls the tokens via the
/// Soroban token `transfer_from` interface and routes the amount to either the
/// collateral deposit path or the debt repayment path.
///
/// # Arguments
/// * `env` - The contract environment
/// * `token_asset` - The token contract to pull funds from
/// * `from` - The owner whose balance will be debited
/// * `amount` - The amount of tokens to pull
/// * `payload` - A vector containing custom data (expected: [Symbol])
pub fn receive(
    env: Env,
    token_asset: Address,
    from: Address,
    amount: i128,
    payload: Vec<Val>,
) -> Result<(), BorrowError> {
    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    if payload.is_empty() {
        return Err(BorrowError::InvalidAmount);
    }

    let action = Symbol::from_val(&env, &payload.get(0).ok_or(BorrowError::InvalidAmount)?);

    if action == Symbol::new(&env, "deposit") {
        if pause::is_paused(&env, PauseType::Deposit) || blocks_high_risk_ops(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
    } else if action == Symbol::new(&env, "repay") {
        if pause::is_paused(&env, PauseType::Repay)
            || (!pause::is_recovery(&env) && blocks_high_risk_ops(&env))
        {
            return Err(BorrowError::ProtocolPaused);
        }
    } else {
        return Err(BorrowError::AssetNotSupported);
    }

    from.require_auth();
    pull_tokens(&env, &token_asset, &from, amount)?;

    if action == Symbol::new(&env, "deposit") {
        deposit(&env, from, token_asset, amount)
    } else {
        repay(&env, from, token_asset, amount)
    }
}

fn pull_tokens(
    env: &Env,
    token_asset: &Address,
    from: &Address,
    amount: i128,
) -> Result<(), BorrowError> {
    let spender = env.current_contract_address();
    let token_client = token::Client::new(env, token_asset);

    if token_client.allowance(from, &spender) < amount {
        return Err(BorrowError::Unauthorized);
    }

    if token_client.balance(from) < amount {
        return Err(BorrowError::InvalidAmount);
    }

    token_client.transfer_from(&spender, from, &spender, &amount);
    Ok(())
}
