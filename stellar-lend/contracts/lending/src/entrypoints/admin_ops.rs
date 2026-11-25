//! Initialization, admin views and TTL keeper entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // One-time contract initialization — sets the protocol admin and seeds
    // the emergency state to [`EmergencyState::Normal`].
    // 
    // # Errors
    // - [`LendingError::AlreadyInitialized`] if `initialize` has already
    //   been called on this contract instance. This returns a typed error
    //   rather than panicking.
    // 
    // # Security
    // `initialize` is the *only* entry point that may be called before the
    // contract is initialized.  Every state-mutating entry point calls
    // [`require_initialized`] at the top and will return
    // [`LendingError::NotInitialized`] until this function succeeds.
    // 
    // `admin.require_auth()` is called as the very first statement to prevent
    // front-running: without it, any account that submits a transaction before
    // the legitimate deployer could claim the admin role permanently.
    pub(crate) fn initialize(env: Env, admin: Address) -> Result<(), LendingError> {
        // Require the supplied admin address to have signed this transaction.
        // Without this guard, any account could front-run the deployer and call
        // initialize() first, naming an arbitrary admin and permanently locking
        // out the intended owner.
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(LendingError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        ttl::extend_instance_ttl(&env);
        set_emergency_state_internal(&env, EmergencyState::Normal);
        debt::save_borrow_index(&env, debt::INDEX_SCALE);
        debt::save_last_index_update(&env, env.ledger().timestamp());
        // Emit schema version event for indexers
        emit_schema_version(&env);
        Ok(())
    }

    pub(crate) fn get_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Admin)
    }

    pub(crate) fn has_admin(env: Env) -> bool {
        env.storage().instance().has(&DataKey::Admin)
    }

    // Admin-free, permissionless keeper entrypoint to extend instance and global storage TTLs.
    //
    // Extends instance storage TTL and all core global persistent keys (TotalDebt,
    // TotalDeposits, BadDebt, DepositCap, UtilizationHistory, and configured asset keys).
    pub(crate) fn bump_ttl(env: Env) -> Result<(), LendingError> {
        require_initialized(&env)?;
        ttl::extend_instance_ttl(&env);
        ttl::bump_all_globals(&env);
        Ok(())
    }

    // Admin-free, permissionless keeper entrypoint to extend instance TTL and persistent keys
    // for a specific asset.
    pub(crate) fn bump_asset_ttl(env: Env, asset: Address) -> Result<(), LendingError> {
        require_initialized(&env)?;
        ttl::extend_instance_ttl(&env);
        ttl::bump_asset_globals(&env, &asset);
        Ok(())
    }

    // Returns the accumulated protocol bad debt.
    pub(crate) fn get_bad_debt(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::BadDebt)
            .unwrap_or(0i128)
    }

    // Return the persisted borrow-rate smoothing state without recomputing rates.
    // 
    // This view is intentionally read-only: it does not call `current_borrow_rate`,
    // does not write cache entries, and does not mutate rate-model storage. When
    // smoothing has never been initialized, all numeric state fields return `0`
    // with `schema_version = 1`.
    pub(crate) fn get_rate_smoothing_state(env: Env) -> RateSmoothingState {
        RateSmoothingState {
            schema_version: SCHEMA_VERSION_V1,
            current_rate_bps: env
                .storage()
                .instance()
                .get(&rate_model::RateModelKey::LastRate)
                .unwrap_or(0),
            last_target_rate_bps: env
                .storage()
                .instance()
                .get(&rate_model::RateModelKey::LastTargetRate)
                .unwrap_or(0),
            last_update_ledger: env
                .storage()
                .instance()
                .get(&rate_model::RateModelKey::LastRateLedger)
                .unwrap_or(0),
        }
    }
}
