use soroban_sdk::{contractevent, contracttype, Address, Env};

/// Types of operations that can be paused.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PauseType {
    /// Pause all protocol operations
    All = 0,
    /// Pause deposit operations
    Deposit = 1,
    /// Pause borrow operations
    Borrow = 2,
    /// Pause repay operations
    Repay = 3,
    /// Pause withdraw operations
    Withdraw = 4,
    /// Pause liquidation operations
    Liquidation = 5,
}

/// Emergency lifecycle states for protocol-wide incident handling.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EmergencyState {
    /// Protocol operates normally.
    Normal = 0,
    /// High-risk operations are hard-stopped.
    Shutdown = 1,
    /// Controlled unwind mode for user recovery.
    Recovery = 2,
}

/// Storage keys for pause states.
#[contracttype]
#[derive(Clone)]
pub enum PauseDataKey {
    /// Pause state for a specific operation type
    State(PauseType),
    /// Optional guardian address authorized to trigger emergency shutdown.
    Guardian,
    /// Current emergency lifecycle state.
    EmergencyState,
}

/// Event data emitted on pause state change.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PauseEvent {
    /// Operation type affected
    pub pause_type: PauseType,
    /// New pause state
    pub paused: bool,
    /// Admin who performed the action
    pub admin: Address,
}

/// Event emitted whenever guardian configuration changes.
#[contractevent]
#[derive(Clone, Debug)]
pub struct GuardianSetEvent {
    /// Guardian address newly configured by admin.
    pub guardian: Address,
    /// Admin who set the guardian.
    pub admin: Address,
}

/// Event emitted on emergency state transitions.
#[contractevent]
#[derive(Clone, Debug)]
pub struct EmergencyStateEvent {
    /// Previous emergency state.
    pub from: EmergencyState,
    /// New emergency state.
    pub to: EmergencyState,
    /// Caller that triggered transition.
    pub caller: Address,
}

/// Set the pause state for a specific operation type.
///
/// Writes the new state to persistent storage and emits a [`PauseEvent`] so
/// off-chain monitors can react immediately. Authorization is enforced by the
/// contract entry point before this function is called.
///
/// # Arguments
/// * `env`        - The contract environment
/// * `admin`      - The admin address included in the emitted event
/// * `pause_type` - The operation type to pause or unpause
/// * `paused`     - `true` to pause, `false` to unpause
///
/// # Security
/// Caller must be the protocol admin. The entry point (`LendingContract::set_pause`)
/// enforces `ensure_admin` before delegating here.
pub fn set_pause(env: &Env, admin: Address, pause_type: PauseType, paused: bool) {
    // Store the pause state
    env.storage()
        .persistent()
        .set(&PauseDataKey::State(pause_type), &paused);

    // Emit event
    PauseEvent {
        pause_type,
        paused,
        admin,
    }
    .publish(env);
}

/// Query whether a specific operation type is currently paused.
///
/// Read-only wrapper around [`is_paused`] intended for contract entry-point
/// exposure. Frontends and off-chain tooling can call this to surface live
/// pause state without issuing a state-modifying transaction.
///
/// # Arguments
/// * `env` - The contract environment
/// * `pause_type` - The operation type to query
///
/// # Returns
/// `true` if the operation is paused (either by its own flag or the global
/// `All` flag); `false` otherwise.
///
/// # Security
/// This function is read-only and requires no authorization. It reflects the
/// exact same state enforced at every operation entry point.
pub fn get_pause_state(env: &Env, pause_type: PauseType) -> bool {
    is_paused(env, pause_type)
}

/// Check if a specific operation is paused.
///
/// An operation is considered paused if either its specific pause flag is set
/// **or** the global [`PauseType::All`] flag is set. The global flag acts as a
/// master kill-switch that overrides individual unpause states.
///
/// # Arguments
/// * `env` - The contract environment
/// * `pause_type` - The operation type to check
///
/// # Returns
/// `true` if the operation is paused, `false` otherwise.
///
/// # Security
/// The global `All` pause is checked first so that a partial unpause of one
/// operation cannot bypass a protocol-wide halt.
pub fn is_paused(env: &Env, pause_type: PauseType) -> bool {
    // Check global pause first
    if env
        .storage()
        .persistent()
        .get(&PauseDataKey::State(PauseType::All))
        .unwrap_or(false)
    {
        return true;
    }

    // Check specific operation pause
    if pause_type != PauseType::All {
        return env
            .storage()
            .persistent()
            .get(&PauseDataKey::State(pause_type))
            .unwrap_or(false);
    }

    false
}

/// Set or rotate the guardian authorized to trigger emergency shutdown.
///
/// The guardian is a trusted address (e.g., a security multisig) that can call
/// [`trigger_shutdown`] without waiting for full governance latency. Only one
/// guardian is active at a time; calling this again replaces the previous one.
///
/// # Arguments
/// * `env`      - The contract environment
/// * `admin`    - Admin address included in the emitted event
/// * `guardian` - New guardian address
///
/// # Security
/// Only the protocol admin may configure the guardian. Setting this to a
/// compromised address grants the power to halt the protocol, so it must be
/// a high-trust multisig.
pub fn set_guardian(env: &Env, admin: Address, guardian: Address) {
    env.storage()
        .persistent()
        .set(&PauseDataKey::Guardian, &guardian);
    GuardianSetEvent { guardian, admin }.publish(env);
}

/// Return the currently configured guardian address, if any.
///
/// Returns `None` when no guardian has been set, in which case only the admin
/// can trigger emergency shutdown.
pub fn get_guardian(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&PauseDataKey::Guardian)
}

/// Return the current emergency lifecycle state.
///
/// Defaults to [`EmergencyState::Normal`] when no state has been written
/// (i.e., before any emergency event has occurred).
pub fn get_emergency_state(env: &Env) -> EmergencyState {
    env.storage()
        .persistent()
        .get(&PauseDataKey::EmergencyState)
        .unwrap_or(EmergencyState::Normal)
}

/// Return `true` if the protocol is in controlled recovery mode.
///
/// During recovery, unwind operations (`repay`, `withdraw`) are permitted so
/// users can exit positions, while new-risk operations (`borrow`, `deposit`,
/// `flash_loan`) remain blocked.
pub fn is_recovery(env: &Env) -> bool {
    get_emergency_state(env) == EmergencyState::Recovery
}

/// Return `true` if the current emergency state should block high-risk operations.
///
/// Both [`EmergencyState::Shutdown`] and [`EmergencyState::Recovery`] block
/// new-risk operations. Entry points for `borrow`, `deposit`, `liquidate`, and
/// `flash_loan` call this helper to enforce the emergency boundary.
pub fn blocks_high_risk_ops(env: &Env) -> bool {
    matches!(
        get_emergency_state(env),
        EmergencyState::Shutdown | EmergencyState::Recovery
    )
}

/// Transition the protocol into emergency shutdown.
///
/// Sets [`EmergencyState::Shutdown`] and emits an [`EmergencyStateEvent`].
/// May only be called via the contract entry point after `ensure_shutdown_authorized`
/// has verified the caller is the admin or the configured guardian.
///
/// # Security
/// This is the lowest-latency halt path. The guardian can invoke it without
/// waiting for governance. Shutdown blocks all high-risk operations immediately.
pub fn trigger_shutdown(env: &Env, caller: Address) {
    set_emergency_state(env, caller, EmergencyState::Shutdown);
}

/// Transition the protocol from shutdown to controlled recovery.
///
/// Sets [`EmergencyState::Recovery`] and emits an [`EmergencyStateEvent`].
/// The entry point enforces that the current state is [`EmergencyState::Shutdown`]
/// before delegating here.
///
/// # Security
/// Only the admin may initiate recovery. Recovery intentionally keeps
/// high-risk operations blocked while allowing users to unwind positions.
pub fn start_recovery(env: &Env, caller: Address) {
    set_emergency_state(env, caller, EmergencyState::Recovery);
}

/// Complete the emergency lifecycle and return the protocol to normal operation.
///
/// Sets [`EmergencyState::Normal`] and emits an [`EmergencyStateEvent`].
/// Admin-only; should be called only after all user positions have been
/// safely resolved during recovery.
///
/// # Security
/// Returning to `Normal` re-enables all high-risk operations. Ensure that the
/// root cause of the emergency has been fully addressed before calling this.
pub fn complete_recovery(env: &Env, caller: Address) {
    set_emergency_state(env, caller, EmergencyState::Normal);
}

fn set_emergency_state(env: &Env, caller: Address, to: EmergencyState) {
    let from = get_emergency_state(env);
    env.storage()
        .persistent()
        .set(&PauseDataKey::EmergencyState, &to);
    EmergencyStateEvent { from, to, caller }.publish(env);
}
