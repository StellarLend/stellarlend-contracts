//! Upgrade governance, audit log and config-store entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // Initialize timelocked multisig upgrade governance (admin-only, once).
    pub(crate) fn upgrade_init(
        env: Env,
        caller: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_init(&env, &caller, current_wasm_hash, required_approvals)
    }

    // Propose a WASM upgrade with a timelocked ETA ledger (admin-only).
    pub(crate) fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> Result<u64, LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_propose(&env, &caller, new_wasm_hash, new_version)
    }

    // Record an approval for a pending upgrade proposal (approver-only).
    pub(crate) fn upgrade_approve(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<u32, LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_approve(&env, &caller, proposal_id)
    }

    // Execute an approved upgrade after the timelock elapses (approver-only).
    pub(crate) fn upgrade_execute(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_execute(&env, &caller, proposal_id)
    }

    // Cancel a pending upgrade proposal (admin-only, issue #1940).
    pub(crate) fn upgrade_cancel(env: Env, caller: Address, proposal_id: u64) -> Result<(), LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_cancel(&env, &caller, proposal_id)
    }

    pub(crate) fn upgrade_set_required_approvals(
        env: Env,
        caller: Address,
        required_approvals: u32,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_set_required_approvals(&env, &caller, required_approvals)
    }

    // Add an upgrade approver (admin-only).
    pub(crate) fn upgrade_add_approver(
        env: Env,
        caller: Address,
        approver: Address,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_add_approver(&env, &caller, approver)
    }

    // Remove an upgrade approver (admin-only).
    pub(crate) fn upgrade_remove_approver(
        env: Env,
        caller: Address,
        approver: Address,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        upgrade::upgrade_remove_approver(&env, &caller, approver)
    }

    pub(crate) fn current_version(env: Env) -> Result<u32, LendingError> {
        upgrade::current_version(&env)
    }

    pub(crate) fn current_wasm_hash(env: Env) -> Result<BytesN<32>, LendingError> {
        upgrade::current_wasm_hash(&env)
    }

    pub(crate) fn get_required_approvals(env: Env) -> Result<u32, LendingError> {
        upgrade::get_required_approvals(&env)
    }

    pub(crate) fn get_upgrade_approvers(env: Env) -> Result<Vec<Address>, LendingError> {
        upgrade::get_upgrade_approvers(&env)
    }

    pub(crate) fn get_proposal_approvals(
        env: Env,
        proposal_id: u64,
    ) -> Result<Vec<Address>, LendingError> {
        upgrade::get_proposal_approvals(&env, proposal_id)
    }

    pub(crate) fn upgrade_status(
        env: Env,
        proposal_id: u64,
    ) -> Result<upgrade::UpgradeStatus, LendingError> {
        upgrade::upgrade_status(&env, proposal_id)
    }

    // Returns whether a proposal is in the `Cancelled` terminal state (issue #1940).
    pub(crate) fn is_upgrade_proposal_cancelled(env: Env, proposal_id: u64) -> bool {
        upgrade::is_proposal_cancelled(&env, proposal_id)
    }

    // Returns the stored domain-separated approval binding hash for
    // `(proposal_id, approver)` (issue #1940).
    pub(crate) fn get_upgrade_approval_binding(
        env: Env,
        proposal_id: u64,
        approver: Address,
    ) -> Option<BytesN<32>> {
        upgrade::get_approval_binding(&env, proposal_id, approver)
    }

    // Returns the approver-set fingerprint captured when the proposal was
    // created (issue #1940).
    pub(crate) fn get_upgrade_proposal_signer_hash(env: Env, proposal_id: u64) -> Option<BytesN<32>> {
        upgrade::get_proposal_approver_set_hash(&env, proposal_id)
    }

    // Returns the fingerprint of the live upgrade approver set (issue #1940).
    pub(crate) fn get_upgrade_approver_set_hash(env: Env) -> BytesN<32> {
        upgrade::get_approver_set_hash(&env)
    }

    pub(crate) fn get_min_upgrade_delay_ledgers(env: Env) -> u32 {
        upgrade::get_min_upgrade_delay_ledgers(&env)
    }

    // Returns the total number of governance audit log entries ever recorded.
    // This count never resets even when the circular buffer wraps.
    // 
    // Read-only, no auth required.
    pub(crate) fn get_governance_audit_count(env: Env) -> u64 {
        audit_log::get_governance_audit_count(&env)
    }

    // Returns audit log entries, most recent first.
    // Read-only, no auth required.
    // 
    // # Arguments
    // * `limit` - Max entries to return (0 = all available, max = buffer size)
    pub(crate) fn get_governance_audit_entries(env: Env, limit: u64) -> Vec<AuditLogEntry> {
        audit_log::get_governance_audit_entries(&env, limit)
    }

    // -----------------------------------------------------------------------
    // Config store
    // -----------------------------------------------------------------------

    pub(crate) fn config_set(env: Env, key: Symbol, value: Bytes) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        let mut entries: Vec<ConfigEntry> = env
            .storage()
            .instance()
            .get(&DataKey::ConfigEntries)
            .unwrap_or_else(|| Vec::new(&env));
        let mut found = false;
        for i in 0..entries.len() {
            if entries.get(i).unwrap().key == key {
                entries.set(
                    i,
                    ConfigEntry {
                        key: key.clone(),
                        value: value.clone(),
                    },
                );
                found = true;
                break;
            }
        }
        if !found {
            entries.push_back(ConfigEntry {
                key: key.clone(),
                value,
            });
        }
        env.storage()
            .instance()
            .set(&DataKey::ConfigEntries, &entries);
        emit_config_set(&env, &key);
        Ok(())
    }

    pub(crate) fn config_get(env: Env, key: Symbol) -> Option<Bytes> {
        let entries: Vec<ConfigEntry> = env
            .storage()
            .instance()
            .get(&DataKey::ConfigEntries)
            .unwrap_or_else(|| Vec::new(&env));
        for i in 0..entries.len() {
            let entry = entries.get(i).unwrap();
            if entry.key == key {
                return Some(entry.value);
            }
        }
        None
    }

    pub(crate) fn config_backup(env: Env, name: Symbol) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        let entries: Vec<ConfigEntry> = env
            .storage()
            .instance()
            .get(&DataKey::ConfigEntries)
            .unwrap_or_else(|| Vec::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::ConfigBackup(name.clone()), &entries);
        emit_config_backup(&env, &name);
        Ok(())
    }

    pub(crate) fn config_restore(env: Env, name: Symbol) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        let backup_key = DataKey::ConfigBackup(name.clone());
        let entries: Vec<ConfigEntry> = env
            .storage()
            .instance()
            .get(&backup_key)
            .ok_or(LendingError::BackupNotFound)?;
        env.storage()
            .instance()
            .set(&DataKey::ConfigEntries, &entries);
        emit_config_restore(&env, &name);
        Ok(())
    }
}
