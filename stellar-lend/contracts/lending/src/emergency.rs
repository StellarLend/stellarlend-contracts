//! Emergency state and pause helpers.

use super::*;

pub(crate) fn pause_is_active(env: &Env, operation: PauseType) -> bool {
    let key = DataKey::PauseState(operation);
    env.storage()
        .instance()
        .get(&key)
        .map(|state: PauseState| state.paused && env.ledger().sequence() < state.expires_at_ledger)
        .unwrap_or(false)
}

pub(crate) fn check_pause_status(env: &Env, action: ProtocolAction) {
    if pause_is_active(env, PauseType::All) {
        panic_with_error!(&env, LendingError::Unauthorized);
    }
    let operation = match action {
        ProtocolAction::Deposit => PauseType::Deposit,
        ProtocolAction::Withdraw => PauseType::Withdraw,
        ProtocolAction::Borrow => PauseType::Borrow,
        ProtocolAction::Repay => PauseType::Repay,
        ProtocolAction::Liquidate => PauseType::Liquidation,
        ProtocolAction::FlashLoan => PauseType::FlashLoan,
    };
    if pause_is_active(env, operation) {
        panic_with_error!(&env, LendingError::Unauthorized);
    }
}

pub(crate) fn read_emergency_state(env: &Env) -> EmergencyState {
    env.storage()
        .instance()
        .get(&DataKey::EmergencyState)
        .unwrap_or(EmergencyState::Normal)
}

pub(crate) fn set_emergency_state_internal(env: &Env, state: EmergencyState) {
    env.storage()
        .instance()
        .set(&DataKey::EmergencyState, &state);
}

pub(crate) fn check_emergency_status(env: &Env, action: ProtocolAction) {
    match read_emergency_state(env) {
        EmergencyState::Normal => {}
        EmergencyState::Shutdown => panic!("OperationDisabledDuringShutdown"),
        EmergencyState::Recovery => match action {
            ProtocolAction::Repay | ProtocolAction::Withdraw | ProtocolAction::Liquidate => {}
            _ => panic!("ActionBlockedInRecovery"),
        },
    }
}

// Guard that every state-mutating entry point calls first.
// 
// Checks that [`DataKey::Admin`] is present in instance storage, which is
// only true after a successful [`LendingContract::initialize`] call.
// 
// # Errors
// Returns [`LendingError::NotInitialized`] when the contract has not yet
// been initialized, allowing callers to surface a clean, typed error
// instead of panicking with an `unwrap` failure.
// 
// # Example
// ```rust,ignore
// pub fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
//     require_initialized(&env)?;
//     // … rest of deposit logic
// }
// ```
pub(crate) fn require_initialized(env: &Env) -> Result<(), LendingError> {
    if env.storage().instance().has(&DataKey::Admin) {
        ttl::extend_instance_ttl(env);
        Ok(())
    } else {
        Err(LendingError::NotInitialized)
    }
}
