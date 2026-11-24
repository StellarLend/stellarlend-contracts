//! Insurance fund, protocol reserves, bad debt and liquidation grace entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // -----------------------------------------------------------------------
    // Bad-debt accounting views
    // -----------------------------------------------------------------------

    // Return the current insurance fund balance.
    // 
    // The insurance fund is credited by [`credit_insurance_fund`] and consumed
    // by [`write_off_bad_debt`] before the reserve or depositor pool is touched.
    pub(crate) fn get_insurance_fund(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0)
    }

    // Return the configured insurance fund interest share in basis points.
    pub(crate) fn get_insurance_share(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::InsuranceShareBps)
            .unwrap_or(0)
    }

    // Return the current protocol reserve balance.
    pub(crate) fn get_protocol_reserves(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::ProtocolReserves)
            .unwrap_or(0)
    }

    // Return the configured reserve factor in basis points.
    pub(crate) fn get_reserve_factor_bps(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::ReserveFactorBps)
            .unwrap_or(0)
    }

    // Set the share of gross accrued interest retained as protocol reserves.
    //
    // Existing positions are settled with the previous factor before this
    // setting changes, preserving the accrual period's configuration.
    pub(crate) fn set_reserve_factor_bps(
        env: Env,
        reserve_factor_bps: i128,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();
        if !(0..=MAX_RESERVE_FACTOR_BPS).contains(&reserve_factor_bps) {
            return Err(LendingError::InvalidAmount);
        }

        checkpoint_reserve_factor(&env)?;
        env.storage()
            .instance()
            .set(&DataKey::ReserveFactorBps, &reserve_factor_bps);
        ReserveFactorSetEvent { reserve_factor_bps }.publish(&env);
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_reserve_factor_bps"),
            admin,
            None,
        );
        Ok(())
    }

    // Fund the insurance fund by an explicit amount.
    //
    // Only the protocol admin can call this.
    //
    // # Errors
    // - [`LendingError::InvalidAmount`] if `amount <= 0`.
    // - [`LendingError::Overflow`] if the resulting balance would overflow i128.
    pub(crate) fn fund_insurance(env: Env, amount: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        let current: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
        env.storage()
            .instance()
            .set(&DataKey::InsuranceFund, &new_balance);
        emit_insurance_funded(&env, amount, new_balance);
        Ok(())
    }

    // Set the insurance fund interest share in basis points (admin-only).
    // 
    // The share must be between 0 and 10000 bps (0% to 100%).
    // 
    // # Errors
    // - [`LendingError::InvalidAmount`] if `share_bps < 0 || share_bps > 10000`.
    pub(crate) fn set_insurance_share(env: Env, share_bps: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if share_bps < 0 || share_bps > 10000 {
            return Err(LendingError::InvalidAmount);
        }
        let old_share_bps = Self::get_insurance_share(env.clone());
        env.storage()
            .instance()
            .set(&DataKey::InsuranceShareBps, &share_bps);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_insurance_share"),
            admin,
            None,
        );

        emit_insurance_share_set(&env, old_share_bps, share_bps);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Bad-debt accounting mutators (admin-only)
    // -----------------------------------------------------------------------

    // Credit the insurance fund by `amount`.
    // 
    // This is a **pure accounting** operation — no token transfer is performed.
    // The caller (governance or an authorised fee-routing contract) is
    // responsible for ensuring the corresponding tokens are available.
    // 
    // # Errors
    // - [`LendingError::InvalidAmount`] if `amount <= 0`.
    // - [`LendingError::Overflow`] if the resulting balance would overflow i128.
    pub(crate) fn credit_insurance_fund(env: Env, amount: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        let current: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
        env.storage()
            .instance()
            .set(&DataKey::InsuranceFund, &new_balance);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "credit_insurance_fund"),
            admin,
            None,
        );

        Ok(())
    }

    // Withdraw protocol reserves from accounting to `to`.
    //
    // This changes accounting only. A real token transfer is intentionally
    // left to the token-transfer integration.
    pub(crate) fn withdraw_reserves(
        env: Env,
        amount: i128,
        to: Address,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        let reserves: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::ProtocolReserves)
            .unwrap_or(0);
        if amount > reserves {
            return Err(LendingError::InvalidAmount);
        }
        let remaining = reserves
            .checked_sub(amount)
            .ok_or(LendingError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::ProtocolReserves, &remaining);
        ProtocolReservesWithdrawnEvent {
            amount,
            recipient: to.clone(),
        }
        .publish(&env);
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "withdraw_reserves"),
            admin,
            None,
        );
        Ok(())
    }

    // Govern-gated entrypoint to write off recorded bad debt.
    //
    // Clears `amount` of bad debt against available backstops in strict
    // precedence order:
    //
    // 1. **Insurance fund** — consumed first (lowest socialisation impact).
    // 2. **Protocol reserves** — consumed next.
    // 3. **Depositor socialisation** — residual applied as an index haircut
    //    to `TotalDeposits` only when both backstops are exhausted.
    //
    // Emits [`BadDebtWrittenOffEvent`] on success, recording the exact amount
    // drawn from each source for auditability.
    //
    // # Checks-Effects-Interactions
    // All state mutations occur atomically before the event is published.
    // No external call is made; the function is re-entrancy-safe by design.
    //
    // # Errors
    // - [`LendingError::InvalidAmount`] if `amount <= 0`.
    // - [`LendingError::NoBadDebt`] if there is no recorded bad debt.
    // - [`LendingError::WriteOffExceedsBadDebt`] if `amount > bad_debt`.
    // - [`LendingError::Overflow`] if any intermediate subtraction would
    //   underflow (should never occur given correct guards, but kept for safety).
    pub(crate) fn write_off_bad_debt(env: Env, amount: i128) -> Result<(), LendingError> {
        // --- Auth ---
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        // --- Input guards ---
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        let bad_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::BadDebt)
            .unwrap_or(0);
        if bad_debt == 0 {
            return Err(LendingError::NoBadDebt);
        }
        if amount > bad_debt {
            return Err(LendingError::WriteOffExceedsBadDebt);
        }

        // --- Load mutable state ---
        let insurance: i128 = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(0);
        let protocol_reserves: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::ProtocolReserves)
            .unwrap_or(0);
        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);

        // --- Precedence 1: insurance fund ---
        let insurance_used = amount.min(insurance);
        let new_insurance = insurance
            .checked_sub(insurance_used)
            .ok_or(LendingError::Overflow)?;
        let mut remaining = amount
            .checked_sub(insurance_used)
            .ok_or(LendingError::Overflow)?;

        // --- Precedence 2: protocol reserves ---
        let reserve_used = remaining.min(protocol_reserves);
        let new_protocol_reserves = protocol_reserves
            .checked_sub(reserve_used)
            .ok_or(LendingError::Overflow)?;
        remaining = remaining
            .checked_sub(reserve_used)
            .ok_or(LendingError::Overflow)?;

        // --- Precedence 3: depositor socialisation (index haircut) ---
        // `remaining` is the unabsorbed residual after both backstops.
        let socialized = remaining;
        if socialized > total_deposits {
            return Err(LendingError::Overflow);
        }
        let final_total_deposits = total_deposits
            .checked_sub(socialized)
            .ok_or(LendingError::Overflow)?;

        // --- Update bad debt ---
        let new_bad_debt = bad_debt.checked_sub(amount).ok_or(LendingError::Overflow)?;

        // --- Effects: persist all state changes atomically ---
        env.storage()
            .instance()
            .set(&DataKey::InsuranceFund, &new_insurance);
        env.storage()
            .persistent()
            .set(&DataKey::ProtocolReserves, &new_protocol_reserves);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &final_total_deposits);
        crate::ttl::bump_global(&env, &DataKey::TotalDeposits);
        env.storage()
            .persistent()
            .set(&DataKey::BadDebt, &new_bad_debt);
        crate::ttl::bump_global(&env, &DataKey::BadDebt);

        // --- Emit auditable event ---
        BadDebtWrittenOffEvent {
            amount,
            insurance_used,
            reserve_used,
            socialized,
        }
        .publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "write_off_bad_debt"),
            admin,
            None,
        );

        Ok(())
    }

    // Set the liquidation grace period in seconds (admin-only).
    // 
    // The grace period enforces a minimum time that must elapse after a position
    // becomes unhealthy (health factor < 1.0) before it can be liquidated.
    // Caps the grace period at 30 days (`MAX_LIQUIDATION_GRACE_PERIOD_SECS`).
    pub(crate) fn set_liquidation_grace_period(env: Env, grace_secs: u64) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if grace_secs > MAX_LIQUIDATION_GRACE_PERIOD_SECS {
            return Err(LendingError::InvalidLiquidationGracePeriod);
        }
        let old_grace_secs = Self::get_liquidation_grace_period(env.clone());
        env.storage()
            .persistent()
            .set(&DataKey::LiquidationGracePeriodSecs, &grace_secs);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_liquidation_grace_period"),
            admin,
            None,
        );

        emit_liquidation_grace_period_set(&env, old_grace_secs, grace_secs);
        Ok(())
    }

    // Return the configured liquidation grace period in seconds.
    pub(crate) fn get_liquidation_grace_period(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::LiquidationGracePeriodSecs)
            .unwrap_or(0)
    }

    // Record the timestamp when a borrower's position first became unhealthy.
    //
    // Permissionless entrypoint. If the position is unhealthy (`HF < 1.0`) and no
    // `FirstUnhealthyTimestamp` is recorded, stores the current ledger timestamp and
    // extends its TTL. If the position is healthy (`HF >= 1.0`), any stale marker is
    // cleared.
    //
    // Rollback note: `liquidate` must fail with `LiquidationGracePeriodActive` while
    // a grace period is active, and Soroban rolls back writes made by a failing call,
    // so keepers call this entrypoint to start the grace-period countdown on-chain.
    pub(crate) fn mark_unhealthy(env: Env, borrower: Address) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let first_unhealthy_key = DataKey::FirstUnhealthyTimestamp(borrower.clone());

        let position = load_debt(&env, &borrower);
        let debt = position.principal;
        if debt <= 0 {
            if env.storage().persistent().has(&first_unhealthy_key) {
                env.storage().persistent().remove(&first_unhealthy_key);
            }
            return Ok(());
        }

        let col_key = DataKey::Collateral(borrower.clone());
        let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
        let threshold_bps = Self::get_liquidation_threshold_bps(env.clone());
        let collateral_value = value_collateral(&env, collateral)?;
        let debt_value = value_debt(&env, debt)?;
        let hf = math::checked_mul_div_floor(collateral_value, threshold_bps, debt_value)
            .map_err(|_| LendingError::Overflow)?;

        if hf >= HEALTH_FACTOR_SCALE {
            if env.storage().persistent().has(&first_unhealthy_key) {
                env.storage().persistent().remove(&first_unhealthy_key);
            }
            return Ok(());
        }

        if !env.storage().persistent().has(&first_unhealthy_key) {
            let now = env.ledger().timestamp();
            env.storage().persistent().set(&first_unhealthy_key, &now);
        }
        extend_first_unhealthy_ttl(&env, &borrower);
        Ok(())
    }

    // Return the timestamp when `borrower`'s position first became unhealthy, if recorded.
    pub(crate) fn get_first_unhealthy_timestamp(env: Env, borrower: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::FirstUnhealthyTimestamp(borrower))
    }
}
