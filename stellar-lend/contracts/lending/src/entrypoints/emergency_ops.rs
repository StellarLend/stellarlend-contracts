//! Admin handover, guardian, emergency state, pause and min-borrow entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // Propose a new admin (current admin only).
    // 
    // Replaces any existing pending admin proposal.
    pub(crate) fn propose_admin(env: Env, new_admin: Address) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let current_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        current_admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        emit_admin_proposed(&env, &current_admin, &new_admin);
        Ok(())
    }

    // Accept the currently pending admin handover.
    // 
    // Returns `PendingAdminNotSet` if no admin has been proposed yet. On
    // success, the pending admin must sign the call, the admin address is
    // updated, and `PendingAdmin` is cleared.
    pub(crate) fn accept_admin(env: Env) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let old_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(LendingError::PendingAdminNotSet)?;
        pending_admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::Admin, &pending_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "accept_admin"),
            pending_admin.clone(),
            None,
        );

        emit_admin_accepted(&env, &old_admin, &pending_admin);
        Ok(())
    }

    pub(crate) fn set_guardian(env: Env, guardian: Address) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        let old_guardian: Option<Address> = env.storage().instance().get(&DataKey::Guardian);
        env.storage().instance().set(&DataKey::Guardian, &guardian);
        GuardianSetEvent {
            caller: admin.clone(),
            old_guardian,
            new_guardian: Some(guardian),
        }
        .publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_guardian"),
            admin.clone(),
            None,
        );

        Ok(())
    }

    pub(crate) fn clear_guardian(env: Env) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        let old_guardian: Option<Address> = env.storage().instance().get(&DataKey::Guardian);
        env.storage().instance().remove(&DataKey::Guardian);
        GuardianSetEvent {
            caller: admin.clone(),
            old_guardian,
            new_guardian: None,
        }
        .publish(&env);
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "clear_guardian"),
            admin,
            None,
        );
        Ok(())
    }

    pub(crate) fn get_guardian(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Guardian)
    }

    // Return the current emergency state, or `None` before initialization.
    pub(crate) fn get_emergency_state(env: Env) -> Option<EmergencyState> {
        env.storage().instance().get(&DataKey::EmergencyState)
    }

    pub(crate) fn set_emergency_state(
        env: Env,
        caller: Address,
        new_state: EmergencyState,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        caller.require_auth();
        let is_admin = is_admin(&env, &caller);
        let is_guardian = is_guardian(&env, &caller);
        let authorized = match new_state {
            EmergencyState::Shutdown => is_admin || is_guardian,
            EmergencyState::Recovery | EmergencyState::Normal => is_admin,
        };
        if !authorized {
            return Err(LendingError::Unauthorized);
        }

        let old_state = read_emergency_state(&env);
        set_emergency_state_internal(&env, new_state);
        EmergencyStateChangedEvent {
            caller: caller.clone(),
            old_state,
            new_state,
        }
        .publish(&env);

        // Record audit entry
        let state_name = match new_state {
            EmergencyState::Normal => "set_state_normal",
            EmergencyState::Shutdown => "set_state_shutdown",
            EmergencyState::Recovery => "set_state_recovery",
        };
        audit_log::record_audit_entry(&env, String::from_str(&env, state_name), caller, None);

        Ok(())
    }

    // Set or clear a granular pause flag with a TTL.
    // 
    // `ttl_ledgers` is added to the current ledger sequence to compute
    // `expires_at_ledger`. A `ttl_ledgers` of 0 means the pause expires
    // immediately (at the current sequence), so `pause_is_active` returns
    // false right away. Setting `paused = false` is a valid unpause call
    // that clears the active pause regardless of TTL.
    // 
    // Core user operations check granular/global pause state before emergency
    // state so an active pause can also stop recovery-allowed unwind paths.
    pub(crate) fn set_pause(
        env: Env,
        caller: Address,
        pause_type: PauseType,
        paused: bool,
        ttl_ledgers: u32,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        caller.require_auth();
        let is_admin = is_admin(&env, &caller);
        let is_guardian = is_guardian(&env, &caller);
        if !(is_admin || (paused && is_guardian)) {
            return Err(LendingError::Unauthorized);
        }

        let expires_at_ledger = env.ledger().sequence().saturating_add(ttl_ledgers);

        let key = DataKey::PauseState(pause_type);
        let old_state = env.storage().instance().get(&key).unwrap_or(PauseState {
            paused: false,
            expires_at_ledger: 0,
        });
        if !paused && (!old_state.paused || env.ledger().sequence() >= old_state.expires_at_ledger) {
            return Ok(());
        }
        let new_state = PauseState {
            paused,
            expires_at_ledger,
        };
        env.storage().instance().set(&key, &new_state);
        PauseStateChangedEvent {
            caller: caller.clone(),
            operation: pause_type,
            old_state,
            new_state,
        }
        .publish(&env);

        // Record audit entry
        let action_name = if paused { "set_pause" } else { "unset_pause" };
        audit_log::record_audit_entry(&env, String::from_str(&env, action_name), caller, None);

        Ok(())
    }

    // Return true if a specific operation is currently paused.
    // 
    // `PauseType::All` acts as a global override for every operation-specific
    // query. Expired pauses are treated as inactive.
    pub(crate) fn get_pause_state(env: Env, pause_type: PauseType) -> bool {
        if pause_is_active(&env, PauseType::All) {
            return true;
        }
        pause_is_active(&env, pause_type)
    }

    pub(crate) fn set_min_borrow(env: Env, min_borrow: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if min_borrow < 0 {
            return Err(LendingError::InvalidAmount);
        }

        env.storage()
            .instance()
            .set(&DataKey::BorrowMinAmount, &min_borrow);
        MinBorrowSetEvent { min_borrow }.publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(&env, String::from_str(&env, "set_min_borrow"), admin, None);

        Ok(())
    }

    pub(crate) fn get_min_borrow(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::BorrowMinAmount)
            .unwrap_or(0)
    }
}
