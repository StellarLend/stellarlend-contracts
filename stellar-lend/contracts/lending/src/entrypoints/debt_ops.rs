//! Repay, debt views, migration, caps and rate-parameter entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    pub(crate) fn repay(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::Repay);
        check_emergency_status(&env, ProtocolAction::Repay);

        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env)?;
        user.require_auth();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let now = env.ledger().timestamp();
        let rate = current_borrow_rate(&env);
        let settled_position = settle_and_accrue_insurance(&env, &position, now, rate)?;
        let updated = repay_amount(settled_position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::RepayAmountTooHigh => LendingError::RepayAmountTooHigh,
            debt::DebtError::Overflow => LendingError::Overflow,
            debt::DebtError::IndexInvariantViolated => LendingError::Overflow,
        })?;
        save_debt(&env, &user, &updated);

        // Track protocol-level total debt.
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let repaid = prev_principal.checked_sub(updated.principal).unwrap_or(0);
        let new_total_debt = total_debt.saturating_sub(repaid);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        crate::ttl::bump_global(&env, &DataKey::TotalDebt);
        extend_debt_ttl(&env, &user);

        // Emit repay event
        emit_repay(&env, &user, amount, updated.principal);

        Ok(updated.principal)
    }

    pub(crate) fn get_debt_position(env: Env, user: Address) -> DebtPosition {
        let position = load_debt(&env, &user);
        if position.principal != 0 {
            extend_debt_ttl(&env, &user);
        }
        position
    }

    // Read-only view of current global borrow index.
    pub(crate) fn get_borrow_index(env: Env) -> i128 {
        load_borrow_index(&env)
    }

    // Read-only view of a user's accrued debt.
    pub(crate) fn compute_debt_view(env: Env, user: Address) -> i128 {
        let position = load_debt(&env, &user);
        let current_index = load_borrow_index(&env);
        debt::compute_debt(&position, current_index)
    }

    // One-time migration: initialise `borrow_index_snapshot` on all
    // pre-existing [`DebtPosition`] records that pre-date the global
    // borrow-index feature.
    // 
    // # Behaviour
    // 1. Requires admin authorisation.
    // 2. Advances the global [`DataKey::BorrowIndex`] to the current
    //    ledger time **before** writing any snapshots, so all migrated
    //    positions share the same post-upgrade index baseline.
    // 3. Iterates every address in [`DataKey::BorrowerList`] and writes
    //    `current_index` into each [`DebtPosition`] whose
    //    `borrow_index_snapshot` is `0` (the sentinel for pre-migration
    //    records; see [`debt::DebtPosition`]).
    // 4. Emits a [`events::MigrationEvent`] recording the index value
    //    used and the number of positions migrated.
    // 5. When called a second time after all positions are already
    //    up-to-date, performs no writes and returns `0`.
    // 
    // # Returns
    // The number of positions whose snapshot was updated.
    // 
    // # Errors
    // - [`LendingError::NotInitialized`] – contract has not been
    //   initialised yet.
    // - [`LendingError::Unauthorized`] – caller is not the admin.
    pub(crate) fn migrate_positions(env: Env) -> Result<u32, LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        // Step 1: Advance the global borrow index to the current time so all
        //         migrated positions share the same post-upgrade baseline.
        let now = env.ledger().timestamp();
        let rate = cached_borrow_rate(&env);
        let current_index = touch_borrow_index(&env, now, rate);

        // Step 2: Load the borrower list (empty if no borrows have ever occurred).
        let borrowers: soroban_sdk::Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::BorrowerList)
            .unwrap_or_else(|| soroban_sdk::vec![&env]);

        // Step 3: Migrate every position whose snapshot is still 0.
        let mut migrated: u32 = 0;
        for i in 0..borrowers.len() {
            let user = borrowers.get(i).unwrap();
            let key = DataKey::Debt(user.clone());
            if let Some(mut position) = env
                .storage()
                .persistent()
                .get::<DataKey, debt::DebtPosition>(&key)
            {
                if position.borrow_index_snapshot == 0 {
                    position.borrow_index_snapshot = current_index;
                    env.storage().persistent().set(&key, &position);
                    migrated += 1;
                }
            }
        }

        // Step 4: Emit migration event.
        events::emit_migration(
            &env,
            1, // old schema version (pre-index)
            2, // new schema version (post-index)
            soroban_sdk::String::from_str(&env, "global-borrow-index migration"),
        );

        Ok(migrated)
    }

    // Set the protocol-level debt ceiling (admin-only).
    pub(crate) fn set_debt_ceiling(env: Env, ceiling: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if ceiling <= 0 {
            return Err(LendingError::Overflow);
        }
        env.storage()
            .instance()
            .set(&DataKey::DebtCeiling, &ceiling);
        crate::events::emit_debt_ceiling_updated(&env, ceiling);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_debt_ceiling"),
            admin,
            None,
        );

        Ok(())
    }

    // Set the maximum total deposits allowed across the protocol (admin-only).
    pub(crate) fn set_deposit_cap(env: Env, cap: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if cap <= 0 {
            return Err(LendingError::InvalidDepositCap);
        }
        let old_cap = Self::get_deposit_cap(env.clone());
        env.storage().persistent().set(&DataKey::DepositCap, &cap);
        crate::ttl::bump_global(&env, &DataKey::DepositCap);

        // Record audit entry
        audit_log::record_audit_entry(&env, String::from_str(&env, "set_deposit_cap"), admin, None);

        emit_deposit_cap_set(&env, old_cap, cap);
        Ok(())
    }

    // Get the current deposit cap, falling back to the protocol default.
    pub(crate) fn get_deposit_cap(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::DepositCap)
            .unwrap_or(DEFAULT_DEPOSIT_CAP)
    }

    // Set the interest-rate curve parameters (admin-only).
    pub(crate) fn set_rate_params(env: Env, params: rate_model::RateParams) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        params.validate()?;

        let old = Self::get_rate_params(env.clone());
        let now = env.ledger().timestamp();
        let old_rate = current_borrow_rate(&env);
        let checkpoint_index = touch_borrow_index(&env, now, old_rate);

        env.storage().instance().set(&DataKey::RateParams, &params);
        RateParamsUpdated {
            old,
            new: params.clone(),
            checkpoint_index,
        }
        .publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(&env, String::from_str(&env, "set_rate_params"), admin, None);

        emit_rate_params_set(
            &env,
            params.base_rate_bps,
            params.multiplier_bps,
            params.jump_multiplier_bps,
            params.kink_utilization_bps,
        );
        Ok(())
    }

    // Get the current rate model parameters, falling back to the default curve.
    pub(crate) fn get_rate_params(env: Env) -> rate_model::RateParams {
        env.storage()
            .instance()
            .get(&DataKey::RateParams)
            .unwrap_or_default()
    }
}
