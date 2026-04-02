//! Social recovery helpers for guardian-managed admin rotation.
//!
//! This module owns the legacy contract surface exported from [`crate::lib`]
//! for bulk guardian configuration and recovery approval/execution.
//!
//! # Trust Boundaries
//!
//! - Multisig admins may configure guardians while no recovery is active.
//! - Guardians may initiate and approve admin rotation, but cannot transfer
//!   protocol funds through this module.
//! - Recovery only mutates the multisig admin set after quorum is met.
//!
//! # Security
//!
//! - Guardian configuration changes are blocked while an unexpired recovery is
//!   active so the approval threshold cannot be changed mid-flight.
//! - Recovery targets are validated against the current multisig admin set on
//!   start, approval, and execution.
//! - This module performs no external contract calls and therefore exposes no
//!   reentrancy surface of its own.
#![allow(unused)]

use soroban_sdk::{Address, Env, Vec};
use crate::prelude::*;

use crate::errors::GovernanceError;
use crate::governance::{
    emit_guardian_added_event, emit_guardian_removed_event, emit_recovery_approved_event,
    emit_recovery_executed_event, emit_recovery_started_event,
};
use crate::storage::GovernanceDataKey;
use crate::types::RecoveryRequest;

const DEFAULT_RECOVERY_PERIOD: u64 = 3 * 24 * 60 * 60;

fn require_multisig_admin(env: &Env, caller: &Address) -> Result<(), GovernanceError> {
    let config =
        crate::governance::get_multisig_config(env).ok_or(GovernanceError::NotInitialized)?;
    if !config.admins.contains(caller.clone()) {
        return Err(GovernanceError::Unauthorized);
    }
    Ok(())
}

fn clear_recovery_state(env: &Env) {
    env.storage()
        .persistent()
        .remove(&GovernanceDataKey::RecoveryRequest);
    env.storage()
        .persistent()
        .remove(&GovernanceDataKey::RecoveryApprovals);
}

fn clear_expired_recovery(env: &Env) -> Result<(), GovernanceError> {
    if let Some(recovery) = env
        .storage()
        .persistent()
        .get::<GovernanceDataKey, RecoveryRequest>(&GovernanceDataKey::RecoveryRequest)
    {
        if env.ledger().timestamp() > recovery.expires_at {
            clear_recovery_state(env);
            return Err(GovernanceError::ProposalExpired);
        }
    }

    Ok(())
}

fn ensure_no_active_recovery(env: &Env) -> Result<(), GovernanceError> {
    match clear_expired_recovery(env) {
        Ok(()) => {}
        Err(GovernanceError::ProposalExpired) => {}
        Err(err) => return Err(err),
    }

    if env
        .storage()
        .persistent()
        .has(&GovernanceDataKey::RecoveryRequest)
    {
        return Err(GovernanceError::RecoveryInProgress);
    }

    Ok(())
}

fn get_multisig_admins(env: &Env) -> Result<Vec<Address>, GovernanceError> {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::MultisigAdmins)
        .ok_or(GovernanceError::InvalidMultisigConfig)
}

fn validate_recovery_targets(
    admins: &Vec<Address>,
    old_admin: &Address,
    new_admin: &Address,
) -> Result<(), GovernanceError> {
    if old_admin == new_admin {
        return Err(GovernanceError::InvalidProposal);
    }

    if !admins.contains(old_admin.clone()) || admins.contains(new_admin.clone()) {
        return Err(GovernanceError::InvalidProposal);
    }

    Ok(())
}

fn count_valid_unique_approvals(guardians: &Vec<Address>, approvals: &Vec<Address>) -> u32 {
    let mut valid = 0u32;
    for guardian in guardians.iter() {
        if approvals.contains(guardian) {
            valid += 1;
        }
    }
    valid
}

/// Replace the guardian set and threshold in one atomic update.
///
/// # Errors
///
/// - `Unauthorized` if `caller` is not a multisig admin.
/// - `RecoveryInProgress` if a non-expired recovery is active.
/// - `InvalidGuardianConfig` if the guardian set is empty, contains duplicates,
///   or `threshold` is zero / greater than the guardian count.
///
/// # Security
///
/// Guardian changes are blocked while recovery is active so approval quorum
/// cannot be manipulated during admin rotation.
pub fn set_guardians(
    env: &Env,
    caller: Address,
    guardians: Vec<Address>,
    threshold: u32,
) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;
    ensure_no_active_recovery(env)?;

    if guardians.is_empty() {
        return Err(GovernanceError::InvalidGuardianConfig);
    }
    if threshold == 0 || threshold > guardians.len() {
        return Err(GovernanceError::InvalidGuardianConfig);
    }

    for i in 0..guardians.len() {
        for j in (i + 1)..guardians.len() {
            if guardians.get(i).unwrap() == guardians.get(j).unwrap() {
                return Err(GovernanceError::InvalidGuardianConfig);
            }
        }
    }

    env.storage()
        .persistent()
        .set(&GovernanceDataKey::Guardians, &guardians);
    env.storage()
        .persistent()
        .set(&GovernanceDataKey::GuardianThreshold, &threshold);

    for g in guardians.iter() {
        emit_guardian_added_event(env, &g);
    }

    Ok(())
}

/// Add a single guardian to the active guardian set.
///
/// # Errors
///
/// - `Unauthorized` if `caller` is not a multisig admin.
/// - `RecoveryInProgress` if a non-expired recovery is active.
/// - `GuardianAlreadyExists` if `guardian` is already configured.
///
/// # Security
///
/// The guardian set is only mutable outside of active recovery windows.
pub fn add_guardian(env: &Env, caller: Address, guardian: Address) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;
    ensure_no_active_recovery(env)?;

    let mut guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
        .unwrap_or_else(|| Vec::new(env));

    if guardians.contains(guardian.clone()) {
        return Err(GovernanceError::GuardianAlreadyExists);
    }

    guardians.push_back(guardian.clone());
    env.storage()
        .persistent()
        .set(&GovernanceDataKey::Guardians, &guardians);

    emit_guardian_added_event(env, &guardian);
    Ok(())
}

/// Remove a guardian while preserving a valid non-zero threshold.
///
/// # Errors
///
/// - `Unauthorized` if `caller` is not a multisig admin.
/// - `RecoveryInProgress` if a non-expired recovery is active.
/// - `GuardianNotFound` if `guardian` is not configured.
/// - `InvalidGuardianConfig` if removal would leave an empty guardian set.
///
/// # Security
///
/// Threshold is clamped downward when needed so recovery cannot become
/// permanently unexecutable after guardian rotation.
pub fn remove_guardian(
    env: &Env,
    caller: Address,
    guardian: Address,
) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;
    ensure_no_active_recovery(env)?;

    // Check if recovery is in progress - prevent guardian removal during recovery
    if env
        .storage()
        .persistent()
        .has(&GovernanceDataKey::RecoveryRequest)
    {
        return Err(GovernanceError::RecoveryInProgress);
    }

    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
        .ok_or(GovernanceError::GuardianNotFound)?;

    let mut new_guardians = Vec::new(env);
    let mut found = false;
    for g in guardians.iter() {
        if g == guardian {
            found = true;
        } else {
            new_guardians.push_back(g);
        }
    }

    if !found {
        return Err(GovernanceError::GuardianNotFound);
    }

    if new_guardians.is_empty() {
        return Err(GovernanceError::InvalidGuardianConfig);
    }

    env.storage()
        .persistent()
        .set(&GovernanceDataKey::Guardians, &new_guardians);

    let threshold = get_guardian_threshold(env).min(new_guardians.len());
    env.storage()
        .persistent()
        .set(&GovernanceDataKey::GuardianThreshold, &threshold);

    emit_guardian_removed_event(env, &guardian);
    Ok(())
}

/// Update the guardian approval threshold.
///
/// # Errors
///
/// - `Unauthorized` if `caller` is not a multisig admin.
/// - `RecoveryInProgress` if a non-expired recovery is active.
/// - `InvalidGuardianConfig` if `threshold` is zero or exceeds guardian count.
///
/// # Security
///
/// Threshold changes are blocked during recovery to prevent mid-flight quorum
/// changes after approvals have started.
pub fn set_guardian_threshold(
    env: &Env,
    caller: Address,
    threshold: u32,
) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;
    ensure_no_active_recovery(env)?;

    // Check if recovery is in progress - prevent threshold changes during recovery
    if env
        .storage()
        .persistent()
        .has(&GovernanceDataKey::RecoveryRequest)
    {
        return Err(GovernanceError::RecoveryInProgress);
    }

    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
        .unwrap_or_else(|| Vec::new(env));

    if threshold == 0 || threshold > guardians.len() {
        return Err(GovernanceError::InvalidGuardianConfig);
    }

    env.storage()
        .persistent()
        .set(&GovernanceDataKey::GuardianThreshold, &threshold);
    Ok(())
}

/// Start a guardian-approved recovery request for admin rotation.
///
/// # Errors
///
/// - `Unauthorized` if `initiator` is not a guardian.
/// - `RecoveryInProgress` if another non-expired recovery exists.
/// - `InvalidProposal` if the target admin rotation is invalid.
/// - `MathOverflow` if the expiry timestamp overflows.
///
/// # Security
///
/// The requested `old_admin` must still be an active multisig admin and
/// `new_admin` must not already be in the admin set.
pub fn start_recovery(
    env: &Env,
    initiator: Address,
    old_admin: Address,
    new_admin: Address,
) -> Result<(), GovernanceError> {
    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
        .ok_or(GovernanceError::Unauthorized)?;

    if !guardians.contains(initiator.clone()) {
        return Err(GovernanceError::Unauthorized);
    }

    ensure_no_active_recovery(env)?;

    let admins = get_multisig_admins(env)?;
    validate_recovery_targets(&admins, &old_admin, &new_admin)?;

    let now = env.ledger().timestamp();
    let recovery = RecoveryRequest {
        old_admin: old_admin.clone(),
        new_admin: new_admin.clone(),
        initiator: initiator.clone(),
        initiated_at: now,
        expires_at: now
            .checked_add(DEFAULT_RECOVERY_PERIOD)
            .ok_or(GovernanceError::MathOverflow)?,
    };

    env.storage()
        .persistent()
        .set(&GovernanceDataKey::RecoveryRequest, &recovery);

    let mut approvals = Vec::new(env);
    approvals.push_back(initiator.clone());
    env.storage()
        .persistent()
        .set(&GovernanceDataKey::RecoveryApprovals, &approvals);

    emit_recovery_started_event(env, &old_admin, &new_admin, &initiator);
    Ok(())
}

/// Approve an active recovery request as a guardian.
///
/// # Errors
///
/// - `Unauthorized` if `approver` is not a guardian.
/// - `NoRecoveryInProgress` if no active request exists.
/// - `ProposalExpired` if the recovery window elapsed.
/// - `AlreadyVoted` if `approver` already approved the request.
/// - `InvalidProposal` if the recovery target is no longer valid.
///
/// # Security
///
/// The recovery target is revalidated against the current admin set before
/// additional approvals are accepted.
pub fn approve_recovery(env: &Env, approver: Address) -> Result<(), GovernanceError> {
    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
        .ok_or(GovernanceError::Unauthorized)?;

    if !guardians.contains(approver.clone()) {
        return Err(GovernanceError::Unauthorized);
    }

    let recovery: RecoveryRequest = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryRequest)
        .ok_or(GovernanceError::NoRecoveryInProgress)?;

    let now = env.ledger().timestamp();
    if now > recovery.expires_at {
        clear_recovery_state(env);
        return Err(GovernanceError::ProposalExpired);
    }

    let admins = get_multisig_admins(env)?;
    if let Err(err) = validate_recovery_targets(&admins, &recovery.old_admin, &recovery.new_admin) {
        clear_recovery_state(env);
        return Err(err);
    }

    let mut approvals: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryApprovals)
        .unwrap_or_else(|| Vec::new(env));

    if approvals.contains(approver.clone()) {
        return Err(GovernanceError::AlreadyVoted);
    }

    approvals.push_back(approver.clone());
    env.storage()
        .persistent()
        .set(&GovernanceDataKey::RecoveryApprovals, &approvals);

    emit_recovery_approved_event(env, &approver);
    Ok(())
}

/// Execute a recovery request after guardian quorum is met.
///
/// # Errors
///
/// - `NoRecoveryInProgress` if no active request exists.
/// - `ProposalExpired` if the recovery window elapsed.
/// - `InsufficientApprovals` if unique guardian approvals are below threshold.
/// - `InvalidGuardianConfig` if the guardian threshold no longer fits the set.
/// - `InvalidProposal` if the target admin rotation is no longer valid.
/// - `InvalidMultisigConfig` if the multisig admin set is unavailable.
///
/// # Security
///
/// Approval counting only includes unique addresses that are still guardians,
/// which prevents stale or duplicated approvals from satisfying quorum.
pub fn execute_recovery(env: &Env, executor: Address) -> Result<(), GovernanceError> {
    let recovery: RecoveryRequest = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryRequest)
        .ok_or(GovernanceError::NoRecoveryInProgress)?;

    let now = env.ledger().timestamp();
    if now > recovery.expires_at {
        clear_recovery_state(env);
        return Err(GovernanceError::ProposalExpired);
    }

    let approvals: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryApprovals)
        .unwrap_or_else(|| Vec::new(env));

    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
        .unwrap_or_else(|| Vec::new(env));

    let threshold = get_guardian_threshold(env);
    if threshold == 0 || threshold > guardians.len() {
        clear_recovery_state(env);
        return Err(GovernanceError::InvalidGuardianConfig);
    }

    let valid_approvals = count_valid_unique_approvals(&guardians, &approvals);
    if valid_approvals < threshold {
        return Err(GovernanceError::InsufficientApprovals);
    }

    let admins = get_multisig_admins(env)?;
    if let Err(err) = validate_recovery_targets(&admins, &recovery.old_admin, &recovery.new_admin) {
        clear_recovery_state(env);
        return Err(err);
    }

    let mut new_admins = Vec::new(env);
    for admin in admins.iter() {
        if admin != recovery.old_admin {
            new_admins.push_back(admin);
        }
    }
    new_admins.push_back(recovery.new_admin.clone());

    config.admins = new_admins;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::MultisigConfig, &config);

    clear_recovery_state(env);

    emit_recovery_executed_event(env, &recovery.old_admin, &recovery.new_admin, &executor);
    Ok(())
}

/// Return the configured guardian set, if any.
pub fn get_guardians(env: &Env) -> Option<Vec<Address>> {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
}

/// Return the configured guardian threshold, defaulting to `1`.
pub fn get_guardian_threshold(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::GuardianThreshold)
        .unwrap_or(1u32)
}

/// Return the pending recovery request, if any.
pub fn get_recovery_request(env: &Env) -> Option<RecoveryRequest> {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryRequest)
}

/// Return recovery approvals collected so far, if any.
pub fn get_recovery_approvals(env: &Env) -> Option<Vec<Address>> {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryApprovals)
}
