//! Social-recovery module — guardian-based admin rotation.
//!
//! This module owns the **recovery-specific** entrypoints that `lib.rs`
//! exposes as top-level contract functions:
//!
//! | Entrypoint          | Description                                              |
//! |---------------------|----------------------------------------------------------|
//! | [`set_guardians`]   | Admin-only: replace the full guardian set + threshold.   |
//! | [`start_recovery`]  | Guardian: open a new recovery request.                   |
//! | [`approve_recovery`]| Guardian: add an approval to the open request.           |
//! | [`execute_recovery`]| Any caller: execute once the threshold is reached.       |
//!
//! ## Storage
//!
//! Recovery state is stored under the *same* `GovernanceDataKey` variants
//! that `governance.rs` uses (`GuardianConfig`, `RecoveryRequest`,
//! `RecoveryApprovals`).  This means the `gov_*` entrypoints and the direct
//! `recovery::*` entrypoints share one consistent view of guardian / recovery
//! state.
//!
//! ## Admin rotation
//!
//! On successful execution the protocol admin stored by `admin.rs`
//! (`AdminDataKey::Admin`) is updated directly.  The governance config admin
//! is updated in parallel when governance has been initialised, so both
//! access paths stay in sync.
//!
//! ## Security notes
//!
//! * `set_guardians` requires the current protocol admin.
//! * `start_recovery` and `approve_recovery` require the caller to be a
//!   configured guardian (checked against storage, not via a passed-in flag).
//! * Double-approval by the same guardian is silently ignored (idempotent).
//! * `execute_recovery` requires no special role — anyone may submit the
//!   transaction once the threshold is met; the threshold check itself is the
//!   security gate.
//! * Opening a new recovery request while one is already pending overwrites
//!   the previous request.  Guardians should coordinate off-chain.

use soroban_sdk::{Address, Env, Vec};

use crate::governance::{GovernanceDataKey, GovernanceError};
use crate::storage::GuardianConfig;
use crate::types::{GovernanceConfig, RecoveryRequest};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Return the stored [`GuardianConfig`], or `None` if none has been set.
fn load_guardian_config(env: &Env) -> Option<GuardianConfig> {
    env.storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
}

/// Return `true` when `address` is a configured guardian.
fn is_guardian(env: &Env, address: &Address) -> bool {
    match load_guardian_config(env) {
        Some(gc) => gc.guardians.contains(address),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Replace the full guardian set and threshold (admin-only).
///
/// This is a **bulk replace** operation: the new `guardians` list completely
/// supersedes the previous one.  The admin must call this at least once before
/// any recovery can be initiated.
///
/// # Arguments
///
/// * `caller`    – Must be the stored protocol admin.
/// * `guardians` – New list of guardian addresses.  May be empty, but an
///                 empty list means recovery can never be started.
/// * `threshold` – How many guardian approvals are required to execute a
///                 recovery.  Must satisfy `1 ≤ threshold ≤ guardians.len()`.
///
/// # Errors
///
/// * [`GovernanceError::Unauthorized`]  – `caller` is not the protocol admin.
/// * [`GovernanceError::InvalidConfig`] – `threshold` is 0, or exceeds the
///                                        length of `guardians`.
pub fn set_guardians(
    env: &Env,
    caller: Address,
    guardians: Vec<Address>,
    threshold: u32,
) -> Result<(), GovernanceError> {
    caller.require_auth();

    // Require the caller to be the stored protocol admin.
    crate::admin::require_admin(env, &caller)
        .map_err(|_| GovernanceError::Unauthorized)?;

    // Validate threshold.
    if threshold == 0 || threshold as usize > guardians.len() as usize {
        return Err(GovernanceError::InvalidConfig);
    }

    env.storage().instance().set(
        &GovernanceDataKey::GuardianConfig,
        &GuardianConfig { guardians, threshold },
    );

    Ok(())
}

/// Open a new recovery request (guardian-only).
///
/// The initiating guardian is automatically recorded as the first approval, so
/// a single-guardian configuration can proceed immediately to
/// [`execute_recovery`] (if the threshold is 1).
///
/// Opening a new request while a prior one is still pending overwrites it and
/// resets approvals.  Guardians should coordinate off-chain to agree on
/// `new_admin` before one of them calls this function.
///
/// # Arguments
///
/// * `initiator`  – Must be a configured guardian.
/// * `old_admin`  – The current admin address (informational / integrity check
///                  — the on-chain admin is not validated here; execution
///                  reads the stored admin directly).
/// * `new_admin`  – The address to install as admin on execution.
///
/// # Errors
///
/// * [`GovernanceError::Unauthorized`] – `initiator` is not a guardian.
pub fn start_recovery(
    env: &Env,
    initiator: Address,
    old_admin: Address,
    new_admin: Address,
) -> Result<(), GovernanceError> {
    initiator.require_auth();

    if !is_guardian(env, &initiator) {
        return Err(GovernanceError::Unauthorized);
    }

    let request = RecoveryRequest {
        old_admin,
        new_admin,
        initiated_at: env.ledger().timestamp(),
        // approval_count mirrors the approvals Vec length; the Vec is the
        // canonical source of truth, but we keep this field for cheap
        // threshold comparisons.
        approval_count: 1,
    };

    // Initiator counts as the first approval.
    let mut approvals: Vec<Address> = Vec::new(env);
    approvals.push_back(initiator);

    env.storage()
        .instance()
        .set(&GovernanceDataKey::RecoveryRequest, &request);
    env.storage()
        .instance()
        .set(&GovernanceDataKey::RecoveryApprovals, &approvals);

    Ok(())
}

/// Add a guardian approval to the open recovery request (guardian-only).
///
/// Calling this more than once with the same `approver` is a no-op
/// (idempotent): the approver is only added once.
///
/// # Errors
///
/// * [`GovernanceError::Unauthorized`]    – `approver` is not a guardian.
/// * [`GovernanceError::NotInitialized`]  – No recovery request is open.
pub fn approve_recovery(env: &Env, approver: Address) -> Result<(), GovernanceError> {
    approver.require_auth();

    if !is_guardian(env, &approver) {
        return Err(GovernanceError::Unauthorized);
    }

    // There must be an open request to approve.
    if !env
        .storage()
        .instance()
        .has(&GovernanceDataKey::RecoveryRequest)
    {
        return Err(GovernanceError::NotInitialized);
    }

    let mut approvals: Vec<Address> = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryApprovals)
        .unwrap_or_else(|| Vec::new(env));

    // Idempotent: only add if not already present.
    if !approvals.contains(&approver) {
        approvals.push_back(approver);
    }

    env.storage()
        .instance()
        .set(&GovernanceDataKey::RecoveryApprovals, &approvals);

    Ok(())
}

/// Execute the recovery once the guardian threshold has been reached.
///
/// On success:
/// 1. The protocol admin stored by `admin.rs` is replaced with
///    `request.new_admin`.
/// 2. If governance has been initialised, the governance config admin is
///    updated to the same address.
/// 3. The `RecoveryRequest` and `RecoveryApprovals` storage entries are
///    removed (one-shot execution).
///
/// The executor does not need to be a guardian; anyone may submit the
/// transaction once the threshold is met.
///
/// # Errors
///
/// * [`GovernanceError::NotInitialized`] – No open recovery request exists.
/// * [`GovernanceError::Unauthorized`]   – Approval count is below the
///                                         guardian threshold.
pub fn execute_recovery(env: &Env, executor: Address) -> Result<(), GovernanceError> {
    executor.require_auth();

    let request: RecoveryRequest = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryRequest)
        .ok_or(GovernanceError::NotInitialized)?;

    let gc: GuardianConfig = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::GuardianConfig)
        .ok_or(GovernanceError::Unauthorized)?;

    let approvals: Vec<Address> = env
        .storage()
        .instance()
        .get(&GovernanceDataKey::RecoveryApprovals)
        .unwrap_or_else(|| Vec::new(env));

    if (approvals.len() as u32) < gc.threshold {
        return Err(GovernanceError::Unauthorized);
    }

    // ── 1. Rotate the low-level admin stored by admin.rs ──────────────────
    //
    // We bypass `admin::set_admin`'s two-step auth check here because the
    // social-recovery mechanism is itself the authentication: threshold
    // guardian signatures already authorise the rotation.
    env.storage()
        .instance()
        .set(&crate::admin::AdminDataKey::Admin, &request.new_admin);

    // ── 2. Keep governance config admin in sync (if initialised) ──────────
    if let Some(mut config) = env
        .storage()
        .instance()
        .get::<GovernanceDataKey, GovernanceConfig>(&GovernanceDataKey::Config)
    {
        config.admin = request.new_admin.clone();
        env.storage()
            .instance()
            .set(&GovernanceDataKey::Config, &config);
    }

    // ── 3. Clean up recovery state (one-shot) ─────────────────────────────
    env.storage()
        .instance()
        .remove(&GovernanceDataKey::RecoveryRequest);
    env.storage()
        .instance()
        .remove(&GovernanceDataKey::RecoveryApprovals);

    Ok(())
}
