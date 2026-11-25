//! Asset isolation-mode entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // -----------------------------------------------------------------------
    // Isolation-mode API
    // -----------------------------------------------------------------------

    // Configure isolation mode for `asset` (admin-only).
    // 
    // Setting `isolated = true` with a positive `isolation_debt_ceiling` marks
    // the asset as isolated.  An isolated asset's collateral contribution is
    // capped so that the total debt backed by it across all users never
    // exceeds the ceiling.  Setting `isolated = false` removes the
    // restriction; the ceiling value is preserved but ignored.
    // 
    // # Errors
    // - `Unauthorized` — caller is not the admin.
    // - `InvalidIsolationCeiling` — `isolation_debt_ceiling` is negative or
    //   zero while `isolated = true`.
    pub(crate) fn set_asset_isolation(
        env: Env,
        asset: Address,
        isolated: bool,
        isolation_debt_ceiling: i128,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if isolation_debt_ceiling < 0 || (isolated && isolation_debt_ceiling == 0) {
            return Err(LendingError::InvalidIsolationCeiling);
        }
        let current_debt = Self::get_isolation_debt(env.clone(), asset.clone());
        if isolated && isolation_debt_ceiling < current_debt {
            return Err(LendingError::IsolationCeilingBelowDebt);
        }
        let config = IsolationConfig {
            isolated,
            isolation_debt_ceiling,
        };
        env.storage()
            .persistent()
            .set(&DataKey::AssetIsolation(asset.clone()), &config);
        crate::ttl::bump_global(&env, &DataKey::AssetIsolation(asset.clone()));

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_asset_isolation"),
            admin,
            None,
        );

        emit_asset_isolation_set(&env, &asset, isolated, isolation_debt_ceiling);
        Ok(())
    }

    // Return the isolation configuration for `asset`.
    // 
    // Returns `None` when no configuration has been set (equivalent to
    // `isolated = false`).
    pub(crate) fn get_asset_isolation(env: Env, asset: Address) -> Option<IsolationConfig> {
        env.storage()
            .persistent()
            .get(&DataKey::AssetIsolation(asset))
    }

    // Return the current running isolation-debt total for `asset`.
    // 
    // This is the sum of all outstanding debt that has been attributed to
    // `asset` acting as isolated collateral.  Returns `0` when no debt has
    // been recorded (either the asset is not isolated or no borrows have
    // been made against it).
    pub(crate) fn get_isolation_debt(env: Env, asset: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::IsolationDebt(asset))
            .unwrap_or(0)
    }

    // Enforce the isolation-mode debt ceiling for `collateral_asset` when a
    // user attempts to borrow `borrow_amount` additional units.
    // 
    // This is a **read-only check** — it does not mutate state.  Callers
    // must update `IsolationDebt` themselves after a successful borrow.
    // 
    // Returns `Ok(())` when:
    // - the collateral asset is not isolated, or
    // - the borrow does not breach the ceiling.
    // 
    // Returns `Err(IsolationCeilingExceeded)` when the ceiling would be
    // breached.
    pub(crate) fn check_isolation_ceiling(
        env: Env,
        collateral_asset: Address,
        borrow_amount: i128,
    ) -> Result<(), LendingError> {
        check_isolation_ceiling_internal(&env, &collateral_asset, borrow_amount)
    }
}
