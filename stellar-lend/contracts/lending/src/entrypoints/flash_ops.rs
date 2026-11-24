//! Flash-loan entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // Set the flash loan fee in basis points (admin-only). Must be in [0, 1000].
    pub(crate) fn set_flash_fee(env: Env, fee_bps: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if !(0..=1000).contains(&fee_bps) {
            return Err(LendingError::InvalidFeeBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::FlashFeeBps, &fee_bps);
        crate::events::emit_flash_fee_updated(&env, fee_bps);

        // Record audit entry
        audit_log::record_audit_entry(&env, String::from_str(&env, "set_flash_fee"), admin, None);

        Ok(())
    }

    // **DEPRECATED**: Repay function used by receiver during callback to return funds to the contract.
    //
    // **This function is deprecated.** New flash loan integrations should repay by
    // calling `token.transfer()` directly to send `amount + fee` to the contract.
    // The updated `flash_loan` verifies repayment via the contract's actual token balance.
    //
    // Uses checked arithmetic to prevent overflow/underflow.
    //
    // Gated behind pause and emergency checks to prevent any flash-loan
    // interaction during a protocol pause or emergency shutdown.
    //
    // # Errors
    // - [`LendingError::NotInitialized`] if the contract has not been
    //   initialized yet.
    // - [`LendingError::InvalidAmount`] if `amount` is zero or negative.
    // - [`LendingError::InsufficientCollateral`] if the payer's recorded
    //   balance is less than `amount`.
    // - [`LendingError::Overflow`] if the checked arithmetic on the payer
    //   or treasury balance would overflow / underflow.
    pub(crate) fn repay_flash_loan(
        env: Env,
        payer: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::FlashLoan);
        check_emergency_status(&env, ProtocolAction::FlashLoan);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        payer.require_auth();
        let payer_key = DataKey::Balance(asset.clone(), payer.clone());
        let payer_bal: i128 = env.storage().persistent().get(&payer_key).unwrap_or(0);
        if payer_bal < amount {
            return Err(LendingError::InsufficientCollateral);
        }
        let new_payer_bal = payer_bal
            .checked_sub(amount)
            .ok_or(LendingError::Overflow)?;
        env.storage().persistent().set(&payer_key, &new_payer_bal);

        let tre_key = DataKey::Treasury(asset.clone());
        let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
        let new_tre_bal = tre_bal
            .checked_add(amount)
            .ok_or(LendingError::Overflow)?;
        env.storage().persistent().set(&tre_key, &new_tre_bal);

        // Emit flash loan repaid event
        emit_flash_loan_repaid(&env, &payer, &asset, amount);
        Ok(())
    }

    // Issue a callback-based flash loan with real token transfers.
    //
    // The receiver gets `amount` tokens during the callback and must repay
    // `amount + fee` before this function returns. The fee is computed with
    // ceiling rounding so small loans always pay at least the minimum fee.
    //
    // Gated behind pause and emergency checks so that flash loans
    // are blocked during granular pause, global pause, and emergency
    // shutdown/recovery — just like deposit/borrow/repay/withdraw.
    //
    // # Errors
    // - [`LendingError::InsufficientLiquidity`] if the contract's token balance
    //   is less than `amount`.
    // - [`LendingError::FlashLoanUtilizationExceeded`] if `amount` exceeds
    //   the maximum flash-loan cap (`max_flash_bps` of available liquidity).
    // - [`LendingError::InsufficientRepayment`] if the contract's token balance
    //   after the callback is less than `balance_before + fee`.
    // - [`LendingError::FlashLoanReentrancy`] if a flash loan is already active.
    // - [`LendingError::InvalidAmount`] if `amount <= 0`.
    // - [`LendingError::Unauthorized`] if `receiver` equals the contract itself.
    pub(crate) fn flash_loan(
        env: Env,
        initiator: Address,
        receiver: Address,
        asset: Address,
        amount: i128,
        params: Bytes,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::FlashLoan);
        check_emergency_status(&env, ProtocolAction::FlashLoan);
        
        // Check reentrancy guard
        let active: bool = env
            .storage()
            .instance()
            .get(&DataKey::FlashActive)
            .unwrap_or(false);
        if active {
            return Err(LendingError::FlashLoanReentrancy);
        }

        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        // Prevent receiver from being the contract itself
        if receiver == env.current_contract_address() {
            return Err(LendingError::Unauthorized);
        }

        initiator.require_auth();

        let token = TokenClient::new(&env, &asset);
        let contract_address = env.current_contract_address();
        
        // Get current balance and subtract any liquidity already reserved for flash loans
        let balance_before = token.balance(&contract_address);
        let reserved = get_flash_loan_reserved(&env, &asset);
        let available_liquidity = balance_before.saturating_sub(reserved);

        // Check liquidity against available (unreserved) balance
        if amount > available_liquidity {
            return Err(LendingError::InsufficientLiquidity);
        }

        // Check utilization cap against available (unreserved) liquidity
        let max_flash_bps = Self::max_flash_bps_config(&env);
        let max_flash_amount = available_liquidity
            .checked_mul(max_flash_bps)
            .and_then(|value| value.checked_div(BPS_DENOM))
            .ok_or(LendingError::Overflow)?;
        if amount > max_flash_amount {
            return Err(LendingError::FlashLoanUtilizationExceeded);
        }

        // Compute fee with ceiling rounding (so small loans pay at least 1)
        let fee_bps = Self::get_flash_fee_bps(&env);
        let fee = amount
            .checked_mul(fee_bps)
            .ok_or(LendingError::Overflow)?;
        // Ceiling division: (amount * fee_bps + BPS_DENOM - 1) / BPS_DENOM
        let fee = fee
            .checked_add(BPS_DENOM - 1)
            .ok_or(LendingError::Overflow)?
            / BPS_DENOM;

        // Reserve liquidity in Temporary storage before external callback
        reserve_flash_loan(&env, &asset, amount)?;

        // Transfer tokens to receiver
        token.transfer(&contract_address, &receiver, &amount);

        // Emit flash loan event before callback (rolled back atomically on error).
        emit_flash_loan(&env, &initiator, &receiver, &asset, amount, fee);

        // Set reentrancy guard
        env.storage().instance().set(&DataKey::FlashActive, &true);

        // Invoke callback
        let method = Symbol::new(&env, "on_flash_loan");
        let _result: Val = env.invoke_contract(
            &receiver,
            &method,
            soroban_sdk::vec![
                &env,
                initiator.into_val(&env),
                asset.into_val(&env),
                amount.into_val(&env),
                fee.into_val(&env),
                params.into_val(&env)
            ],
        );

        // Clear reentrancy guard and release flash-loan reservation
        env.storage().instance().set(&DataKey::FlashActive, &false);
        release_flash_loan_reservation(&env, &asset, amount)?;

        // Verify repayment: balance must be >= balance_before + fee
        let balance_after = token.balance(&contract_address);
        let required_balance = balance_before
            .checked_add(fee)
            .ok_or(LendingError::Overflow)?;
        
        if balance_after < required_balance {
            return Err(LendingError::InsufficientRepayment);
        }

        // Route the fee to the insurance fund or reserves based on InsuranceShareBps
        let insurance_share_bps: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceShareBps)
            .unwrap_or(0);
        
        if insurance_share_bps > 0 && insurance_share_bps <= BPS_DENOM {
            let insurance_portion = fee
                .checked_mul(insurance_share_bps)
                .and_then(|v| v.checked_div(BPS_DENOM))
                .unwrap_or(0);
            
            if insurance_portion > 0 {
                let current_insurance: i128 = env
                    .storage()
                    .instance()
                    .get(&DataKey::InsuranceFund)
                    .unwrap_or(0);
                let new_insurance = current_insurance
                    .checked_add(insurance_portion)
                    .ok_or(LendingError::Overflow)?;
                env.storage()
                    .instance()
                    .set(&DataKey::InsuranceFund, &new_insurance);
            }
        }
        // The remainder stays in the contract balance as protocol reserves

        Ok(())
    }
}
