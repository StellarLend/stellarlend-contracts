//! Governance audit and role-check helpers.

use super::*;

// Assert that the transaction signer is the protocol admin.
// Returns a typed error when the contract has not yet been initialized.
// Record a governed risk-parameter change in the shared governance audit log
// (`get_governance_audit_entries`) and emit a typed `GovernanceAuditEvent`
// carrying the old and new values.
pub(crate) fn record_governance_audit(
    env: &Env,
    parameter: GovernedRiskParameter,
    old_value: i128,
    new_value: i128,
) -> Result<(), LendingError> {
    let caller: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(LendingError::NotInitialized)?;
    let action = match parameter {
        GovernedRiskParameter::LiquidationThresholdBps => "set_liquidation_threshold_bps",
        GovernedRiskParameter::MaxLtvBps => "set_max_ltv_bps",
    };
    audit_log::record_audit_entry(env, String::from_str(env, action), caller.clone(), None);
    GovernanceAuditEvent {
        id: audit_log::get_governance_audit_count(env),
        parameter,
        caller,
        timestamp: env.ledger().timestamp(),
        old_value,
        new_value,
    }
    .publish(env);
    Ok(())
}

pub(crate) fn assert_admin(env: &Env) -> Result<(), LendingError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(LendingError::NotInitialized)?;
    admin.require_auth();
    Ok(())
}

// For Shutdown, allow the guardian (if set) or admin; for Recovery/Normal, admin only.
pub(crate) fn is_admin(env: &Env, caller: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, Address>(&DataKey::Admin)
        .is_some_and(|admin| admin == *caller)
}

pub(crate) fn is_guardian(env: &Env, caller: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, Address>(&DataKey::Guardian)
        .is_some_and(|guardian| guardian == *caller)
}
