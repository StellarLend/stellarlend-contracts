#![allow(unused)]
use soroban_sdk::{Address, Env, Vec};

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

pub fn set_guardians(
    env: &Env,
    caller: Address,
    guardians: Vec<Address>,
    threshold: u32,
) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;

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

pub fn add_guardian(env: &Env, caller: Address, guardian: Address) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;

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

pub fn remove_guardian(
    env: &Env,
    caller: Address,
    guardian: Address,
) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;

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

    env.storage()
        .persistent()
        .set(&GovernanceDataKey::Guardians, &new_guardians);
    emit_guardian_removed_event(env, &guardian);
    Ok(())
}

pub fn set_guardian_threshold(
    env: &Env,
    caller: Address,
    threshold: u32,
) -> Result<(), GovernanceError> {
    require_multisig_admin(env, &caller)?;

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

pub fn start_recovery(
    env: &Env,
    initiator: Address,
    old_admin: Address,
    new_admin: Address,
) -> Result<(), GovernanceError> {
    initiator.require_auth();

    let guardians: Vec<Address> = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
        .ok_or(GovernanceError::Unauthorized)?;

    if !guardians.contains(initiator.clone()) {
        return Err(GovernanceError::Unauthorized);
    }

    if env
        .storage()
        .persistent()
        .has(&GovernanceDataKey::RecoveryRequest)
    {
        return Err(GovernanceError::RecoveryInProgress);
    }

    let now = env.ledger().timestamp();
    let recovery = RecoveryRequest {
        old_admin: old_admin.clone(),
        new_admin: new_admin.clone(),
        initiator: initiator.clone(),
        initiated_at: now,
        expires_at: now + DEFAULT_RECOVERY_PERIOD,
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

pub fn approve_recovery(env: &Env, approver: Address) -> Result<(), GovernanceError> {
    approver.require_auth();

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
        env.storage()
            .persistent()
            .remove(&GovernanceDataKey::RecoveryRequest);
        return Err(GovernanceError::ProposalExpired);
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

pub fn execute_recovery(env: &Env, executor: Address) -> Result<(), GovernanceError> {
    executor.require_auth();

    let recovery: RecoveryRequest = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryRequest)
        .ok_or(GovernanceError::NoRecoveryInProgress)?;

    let now = env.ledger().timestamp();
    if now > recovery.expires_at {
        env.storage()
            .persistent()
            .remove(&GovernanceDataKey::RecoveryRequest);
        return Err(GovernanceError::ProposalExpired);
    }

    let threshold: u32 = env
        .storage()
        .persistent()
        .get(&GovernanceDataKey::GuardianThreshold)
        .unwrap_or(1u32);

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

    let mut valid_approvals = 0;
    for approval in approvals.iter() {
        if guardians.contains(approval) {
            valid_approvals += 1;
        }
    }

    if valid_approvals < threshold {
        return Err(GovernanceError::InsufficientApprovals);
    }

    let mut config =
        crate::governance::get_multisig_config(env).ok_or(GovernanceError::NotInitialized)?;

    let mut new_admins = Vec::new(env);
    for a in config.admins.iter() {
        if a != recovery.old_admin {
            new_admins.push_back(a);
        }
    }
    new_admins.push_back(recovery.new_admin.clone());

    config.admins = new_admins;
    env.storage()
        .instance()
        .set(&GovernanceDataKey::MultisigConfig, &config);

    // Also update the centralized protocol admin if it was recovered
    if let Some(current_super_admin) = crate::admin::get_admin(env) {
        if current_super_admin == recovery.old_admin {
            // Force update the super admin since this is a recovery
            env.storage()
                .persistent()
                .set(&crate::admin::AdminDataKey::Admin, &recovery.new_admin);
        }
    }

    env.storage()
        .persistent()
        .remove(&GovernanceDataKey::RecoveryRequest);
    env.storage()
        .persistent()
        .remove(&GovernanceDataKey::RecoveryApprovals);

    emit_recovery_executed_event(env, &recovery.old_admin, &recovery.new_admin, &executor);
    Ok(())
}

pub fn get_guardians(env: &Env) -> Option<Vec<Address>> {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::Guardians)
}

pub fn get_guardian_threshold(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::GuardianThreshold)
        .unwrap_or(1u32)
}

pub fn get_recovery_request(env: &Env) -> Option<RecoveryRequest> {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryRequest)
}

pub fn get_recovery_approvals(env: &Env) -> Option<Vec<Address>> {
    env.storage()
        .persistent()
        .get(&GovernanceDataKey::RecoveryApprovals)
}
