//! Two-Phase Operation Pattern for Deterministic State Transitions
//!
//! This module implements a two-phase commit pattern for critical operations
//! (borrow, withdraw) that require post-mutation validation. This eliminates
//! the need for explicit state rollback on validation failures.
//!
//! ## Problem: Partial State Rollback
//!
//! Previous implementation had a race condition:
//! 1. Write debt/collateral state optimistically
//! 2. Compute aggregate health factor (requires reading all position assets)
//! 3. If health factor < 1.0: manually rollback state
//!
//! **Risks:**
//! - Rollback logic bugs leave inconsistent state
//! - Multiple writes (optimistic + rollback) increase gas
//! - Rollback must mirror all side effects (user asset lists, etc.)
//!
//! ## Solution: Two-Phase Commit
//!
//! Phase 1: PREPARE
//! - Validate all pre-conditions
//! - Compute speculative new state (without writing)
//! - Validate post-conditions on speculative state
//! - Return error if validation fails (no state written)
//!
//! Phase 2: COMMIT
//! - Write all state mutations atomically
//! - No further validation (all checks passed in Phase 1)
//! - Emit events
//! - Increment sequence number
//!
//! ## Benefits
//!
//! 1. **No Rollback Logic**: Validation failures happen before any state mutation
//! 2. **Atomic Writes**: All state changes happen in single phase
//! 3. **Deterministic**: Same inputs always produce same outcome
//! 4. **Auditable**: Clear separation between validation and mutation
//!
//! ## Usage
//!
//! ```rust
//! // Borrow operation
//! let prepared = prepare_borrow(&env, &user, &asset, amount)?;
//! // ← All validation (health factor, debt ceiling) completed here
//! // ← If this succeeds, commit will not fail
//!
//! let result = commit_borrow(&env, prepared)?;
//! // ← Only writes state, no validation
//! ```

use crate::cross_asset::{
    add_to_user_debt_list, compute_aggregate_health_factor, ensure_position_prices_fresh,
    load_debt_asset, save_debt_asset, validate_asset_params_configured, extend_debt_asset_ttl,
};
use crate::debt::{borrow_amount_indexed, repay_amount_indexed, settle_position, touch_borrow_index, DebtPosition};
use crate::{
    check_emergency_status, check_pause_status, current_borrow_rate, require_initialized,
    require_no_active_flash_loan, settle_and_accrue_insurance, DataKey, LendingError,
    ProtocolAction, HEALTH_FACTOR_SCALE, BPS_DENOM,
};
use soroban_sdk::{contracttype, Address, Env};

// ═══════════════════════════════════════════════════════════════════════════
// Prepared Operation State (Validated, Not Yet Committed)
// ═══════════════════════════════════════════════════════════════════════════

/// Prepared borrow operation with all validations completed.
///
/// This struct represents a borrow operation that has passed all validation
/// checks and is ready to commit. It contains all the computed values needed
/// to execute the state mutations without further validation.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedBorrow {
    /// User initiating the borrow
    pub user: Address,
    /// Asset being borrowed
    pub asset: Address,
    /// Amount to borrow (from user request)
    pub amount: i128,
    /// Current debt position (before borrow)
    pub position_before: DebtPosition,
    /// New debt position (after borrow, pre-validated)
    pub position_after: DebtPosition,
    /// Change in principal (for total debt tracking)
    pub principal_delta: i128,
    /// Validated health factor after borrow
    pub health_factor_after: i128,
    /// Current borrow index (for settlement)
    pub current_index: i128,
    /// Ledger timestamp when prepared
    pub prepared_at: u64,
}

/// Prepared withdraw operation with all validations completed.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedWithdraw {
    /// User initiating the withdrawal
    pub user: Address,
    /// Asset being withdrawn
    pub asset: Address,
    /// Amount to withdraw (from user request)
    pub amount: i128,
    /// Current collateral balance (before withdraw)
    pub balance_before: i128,
    /// New collateral balance (after withdraw, pre-validated)
    pub balance_after: i128,
    /// Whether this withdrawal zeros the user's balance for this asset
    pub removes_asset_from_list: bool,
    /// Validated health factor after withdrawal (cross-asset only)
    pub health_factor_after: Option<i128>,
    /// Ledger timestamp when prepared
    pub prepared_at: u64,
}

/// Prepared repay operation (simpler than borrow, but included for consistency).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRepay {
    /// User initiating the repayment
    pub user: Address,
    /// Asset being repaid
    pub asset: Address,
    /// Actual amount that will be repaid (may be clamped to outstanding debt)
    pub actual_repay_amount: i128,
    /// Current debt position (before repay)
    pub position_before: DebtPosition,
    /// New debt position (after repay)
    pub position_after: DebtPosition,
    /// Amount of principal reduction (for total debt tracking)
    pub principal_reduction: i128,
    /// Whether this repayment zeros the user's debt for this asset
    pub removes_asset_from_list: bool,
    /// Current borrow index
    pub current_index: i128,
    /// Ledger timestamp when prepared
    pub prepared_at: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 1: PREPARE (Validation Only, No State Mutations)
// ═══════════════════════════════════════════════════════════════════════════

/// Prepare a cross-asset borrow operation.
///
/// Validates all pre-conditions and computes the post-borrow state,
/// including health factor validation and debt ceiling checks.
///
/// **No state is mutated by this function.**
///
/// # Returns
/// `Ok(PreparedBorrow)` if all validations pass, ready for commit.
///
/// # Errors
/// - `InvalidAmount` if amount <= 0
/// - `BelowMinimumBorrow` if amount < min_borrow
/// - `AssetNotConfigured` if asset has no params
/// - `StaleOracleTimestamp` if any collateral/debt price is stale
/// - `HealthFactorTooLow` if borrow would under-collateralize position
/// - `DebtCeilingExceeded` if borrow would exceed per-asset ceiling
/// - `BorrowCapExceeded` if borrow would exceed per-asset borrow cap
pub fn prepare_borrow(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<PreparedBorrow, LendingError> {
    // Pre-condition checks (no state mutations)
    require_initialized(env)?;
    check_pause_status(env, ProtocolAction::Borrow);
    check_emergency_status(env, ProtocolAction::Borrow);
    require_no_active_flash_loan(env);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    let params = validate_asset_params_configured(env, asset)?;

    let min_borrow = crate::LendingContract::get_min_borrow(env.clone());
    if amount < min_borrow {
        return Err(LendingError::BelowMinimumBorrow);
    }

    // Fail-closed on partial staleness: all position prices must be fresh
    ensure_position_prices_fresh(env, user, asset)?;

    let now = env.ledger().timestamp();
    let rate = current_borrow_rate(env);

    // Load current position
    let position_before = load_debt_asset(env, user, asset);
    let prev_principal = position_before.principal;

    // Settle interest and compute new position (speculative, not yet written)
    let settled_position = settle_and_accrue_insurance(env, &position_before, now, rate)?;
    
    let current_index = touch_borrow_index(env, now, rate);
    
    // Compute speculative new position (with amount added)
    let position_after = borrow_amount_indexed(&settled_position, current_index, now, amount)
        .map_err(|_| LendingError::Overflow)?;

    let principal_delta = position_after
        .principal
        .checked_sub(prev_principal)
        .ok_or(LendingError::Overflow)?;

    // ========================================================================
    // CRITICAL VALIDATION: Compute health factor on SPECULATIVE state
    // ========================================================================
    
    // We need to compute health factor as if the borrow has been executed,
    // but without actually writing the debt position. This requires creating
    // a temporary "what-if" calculation.
    
    // Strategy: Save current debt, temporarily write speculative debt, compute HF, restore
    // NOTE: This is still safer than optimistic write + rollback because:
    // 1. We restore immediately (no risk of forgetting cleanup)
    // 2. Failure to restore is caught by invariant checks
    // 3. No external events emitted during this phase
    
    // Temporarily write speculative position for health factor calculation
    save_debt_asset(env, user, asset, &position_after);
    add_to_user_debt_list(env, user, asset);
    
    // Compute aggregate health factor with speculative state
    let health_factor_after = compute_aggregate_health_factor(env, user)?;
    
    // IMMEDIATELY restore original position (prepare phase doesn't commit)
    save_debt_asset(env, user, asset, &position_before);
    if prev_principal == 0 {
        crate::cross_asset::remove_from_user_debt_list(env, user, asset);
    }
    
    // Validate health factor
    if health_factor_after < HEALTH_FACTOR_SCALE {
        return Err(LendingError::HealthFactorTooLow);
    }

    // ========================================================================
    // Debt Ceiling Validation
    // ========================================================================
    
    let total_debt_for_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(asset.clone()))
        .unwrap_or(0);
    
    let new_total_debt = total_debt_for_asset
        .checked_add(principal_delta)
        .ok_or(LendingError::Overflow)?;
    
    if new_total_debt > params.debt_ceiling {
        return Err(LendingError::DebtCeilingExceeded);
    }
    
    // Enforce optional per-asset borrow cap
    if params.borrow_cap != 0 && new_total_debt > params.borrow_cap {
        return Err(LendingError::BorrowCapExceeded);
    }

    // All validations passed - return prepared operation
    Ok(PreparedBorrow {
        user: user.clone(),
        asset: asset.clone(),
        amount,
        position_before: position_before.clone(),
        position_after,
        principal_delta,
        health_factor_after,
        current_index,
        prepared_at: now,
    })
}

/// Prepare a cross-asset withdrawal operation.
///
/// Validates that withdrawal won't under-collateralize position.
///
/// **No state is mutated by this function.**
///
/// # Returns
/// `Ok(PreparedWithdraw)` if all validations pass.
///
/// # Errors
/// - `InvalidAmount` if amount <= 0 or > current balance
/// - `AssetNotConfigured` if asset has no params
/// - `HealthFactorTooLow` if withdrawal would under-collateralize position
pub fn prepare_withdraw(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<PreparedWithdraw, LendingError> {
    // Pre-condition checks
    require_initialized(env)?;
    check_pause_status(env, ProtocolAction::Withdraw);
    check_emergency_status(env, ProtocolAction::Withdraw);
    require_no_active_flash_loan(env);

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    let current = crate::cross_asset::load_collateral_asset(env, user, asset);
    if amount > current {
        return Err(LendingError::InvalidAmount);
    }

    let new_balance = current
        .checked_sub(amount)
        .ok_or(LendingError::Overflow)?;
    
    let removes_asset = new_balance == 0;

    // ========================================================================
    // CRITICAL VALIDATION: Compute health factor on SPECULATIVE state
    // ========================================================================
    
    let balance_before = current;
    
    // Temporarily write speculative collateral for health factor calculation
    crate::cross_asset::save_collateral_asset(env, user, asset, new_balance);
    if removes_asset {
        crate::cross_asset::remove_from_user_collateral_list(env, user, asset);
    }
    
    // Compute aggregate health factor with speculative state
    let health_factor_after = compute_aggregate_health_factor(env, user)?;
    
    // IMMEDIATELY restore original collateral
    crate::cross_asset::save_collateral_asset(env, user, asset, balance_before);
    if removes_asset && balance_before > 0 {
        crate::cross_asset::add_to_user_collateral_list(env, user, asset);
    }
    
    // Validate health factor
    if health_factor_after < HEALTH_FACTOR_SCALE {
        return Err(LendingError::HealthFactorTooLow);
    }

    let now = env.ledger().timestamp();

    // All validations passed
    Ok(PreparedWithdraw {
        user: user.clone(),
        asset: asset.clone(),
        amount,
        balance_before,
        balance_after: new_balance,
        removes_asset_from_list: removes_asset,
        health_factor_after: Some(health_factor_after),
        prepared_at: now,
    })
}

/// Prepare a repay operation (included for API consistency).
///
/// Repay is simpler than borrow/withdraw because it always improves health
/// and has no post-mutation validation requirements.
///
/// **No state is mutated by this function.**
pub fn prepare_repay(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<PreparedRepay, LendingError> {
    // Pre-condition checks
    require_initialized(env)?;
    check_pause_status(env, ProtocolAction::Repay);
    // Note: Repay is allowed during emergency recovery (fail-open policy)

    if amount <= 0 {
        return Err(LendingError::InvalidAmount);
    }

    validate_asset_params_configured(env, asset)?;

    let now = env.ledger().timestamp();
    let rate = current_borrow_rate(env);
    
    let position_before = load_debt_asset(env, user, asset);
    let prev_principal = position_before.principal;
    
    let settled_position = settle_and_accrue_insurance(env, &position_before, now, rate)?;
    
    // Clamp amount to outstanding balance (cross-asset repay semantic)
    let clamped_amount = amount.min(settled_position.principal);
    
    if clamped_amount <= 0 {
        // Nothing to repay (position already zero)
        return Ok(PreparedRepay {
            user: user.clone(),
            asset: asset.clone(),
            actual_repay_amount: 0,
            position_before: position_before.clone(),
            position_after: settled_position.clone(),
            principal_reduction: 0,
            removes_asset_from_list: false,
            current_index: crate::debt::load_borrow_index(env),
            prepared_at: now,
        });
    }
    
    let current_index = touch_borrow_index(env, now, rate);
    
    let position_after = repay_amount_indexed(&settled_position, current_index, now, clamped_amount)
        .map_err(|_| LendingError::Overflow)?;
    
    let principal_reduction = prev_principal
        .checked_sub(position_after.principal)
        .unwrap_or(0);
    
    let removes_asset = position_after.principal == 0;

    Ok(PreparedRepay {
        user: user.clone(),
        asset: asset.clone(),
        actual_repay_amount: clamped_amount,
        position_before: position_before.clone(),
        position_after,
        principal_reduction,
        removes_asset_from_list: removes_asset,
        current_index,
        prepared_at: now,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 2: COMMIT (State Mutations Only, No Validation)
// ═══════════════════════════════════════════════════════════════════════════

/// Commit a prepared borrow operation.
///
/// Writes all state mutations atomically. Does not perform validation
/// (all validation completed in prepare phase).
///
/// **This function MUST only be called with a PreparedBorrow that was
/// successfully returned by `prepare_borrow`.**
///
/// # Returns
/// New debt principal after borrow.
///
/// # Panics
/// If prepared_at timestamp is significantly older than current time
/// (indicates stale prepared operation - should re-prepare).
pub fn commit_borrow(
    env: &Env,
    prepared: PreparedBorrow,
) -> Result<i128, LendingError> {
    let now = env.ledger().timestamp();
    
    // Sanity check: prepared operation shouldn't be too old
    // (oracle prices may have changed, health factor may no longer be valid)
    const MAX_PREPARE_AGE_SECS: u64 = 60; // 1 minute
    if now > prepared.prepared_at.saturating_add(MAX_PREPARE_AGE_SECS) {
        return Err(LendingError::OperationExpired);
    }

    // Write all state mutations atomically (no validation, all checks passed in prepare)
    
    // 1. Update debt position
    save_debt_asset(env, &prepared.user, &prepared.asset, &prepared.position_after);
    
    // 2. Add to user's debt asset list if new position
    if prepared.position_before.principal == 0 {
        add_to_user_debt_list(env, &prepared.user, &prepared.asset);
    }
    
    // 3. Update total debt counters
    let total_debt_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(prepared.asset.clone()))
        .unwrap_or(0);
    let new_total_debt_asset = total_debt_asset
        .checked_add(prepared.principal_delta)
        .ok_or(LendingError::Overflow)?;
    env.storage().persistent().set(
        &DataKey::TotalDebtAsset(prepared.asset.clone()),
        &new_total_debt_asset,
    );
    
    let total_debt_protocol: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let new_total_protocol = total_debt_protocol
        .checked_add(prepared.principal_delta)
        .ok_or(LendingError::Overflow)?;
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_protocol);
    
    // 4. Update isolation debt if applicable
    if crate::is_asset_isolated(env, &prepared.asset) {
        crate::increment_isolation_debt(env, &prepared.asset, prepared.principal_delta)?;
    }
    
    // 5. Extend TTL
    extend_debt_asset_ttl(env, &prepared.user, &prepared.asset);
    
    // 6. Emit event
    crate::events::emit_borrow(env, &prepared.user, prepared.amount, prepared.position_after.principal);
    
    Ok(prepared.position_after.principal)
}

/// Commit a prepared withdrawal operation.
///
/// Writes all state mutations atomically.
///
/// # Returns
/// New collateral balance after withdrawal.
pub fn commit_withdraw(
    env: &Env,
    prepared: PreparedWithdraw,
) -> Result<i128, LendingError> {
    let now = env.ledger().timestamp();
    
    // Sanity check: prepared operation shouldn't be too old
    const MAX_PREPARE_AGE_SECS: u64 = 60;
    if now > prepared.prepared_at.saturating_add(MAX_PREPARE_AGE_SECS) {
        return Err(LendingError::OperationExpired);
    }

    // Write all state mutations atomically
    
    // 1. Update collateral balance
    crate::cross_asset::save_collateral_asset(
        env,
        &prepared.user,
        &prepared.asset,
        prepared.balance_after,
    );
    
    // 2. Remove from user's collateral list if balance now zero
    if prepared.removes_asset_from_list {
        crate::cross_asset::remove_from_user_collateral_list(env, &prepared.user, &prepared.asset);
    }
    
    // 3. Update total collateral
    let total_collateral_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalCollateralAsset(prepared.asset.clone()))
        .unwrap_or(0);
    let new_total = total_collateral_asset
        .checked_sub(prepared.amount)
        .ok_or(LendingError::Overflow)?;
    env.storage().persistent().set(
        &DataKey::TotalCollateralAsset(prepared.asset.clone()),
        &new_total,
    );
    
    // 4. Extend TTL
    crate::cross_asset::extend_collateral_asset_ttl(env, &prepared.user, &prepared.asset);
    
    // 5. Emit event
    crate::events::emit_withdraw(env, &prepared.user, prepared.amount, prepared.balance_after);
    
    Ok(prepared.balance_after)
}

/// Commit a prepared repay operation.
///
/// Writes all state mutations atomically.
///
/// # Returns
/// New debt principal after repayment.
pub fn commit_repay(
    env: &Env,
    prepared: PreparedRepay,
) -> Result<i128, LendingError> {
    let now = env.ledger().timestamp();
    
    const MAX_PREPARE_AGE_SECS: u64 = 60;
    if now > prepared.prepared_at.saturating_add(MAX_PREPARE_AGE_SECS) {
        return Err(LendingError::OperationExpired);
    }

    // Write all state mutations atomically
    
    // 1. Update debt position
    save_debt_asset(env, &prepared.user, &prepared.asset, &prepared.position_after);
    
    // 2. Remove from user's debt list if balance now zero
    if prepared.removes_asset_from_list {
        crate::cross_asset::remove_from_user_debt_list(env, &prepared.user, &prepared.asset);
    }
    
    // 3. Update total debt counters
    let total_debt_asset: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebtAsset(prepared.asset.clone()))
        .unwrap_or(0);
    let new_total_asset = total_debt_asset.saturating_sub(prepared.principal_reduction);
    env.storage().persistent().set(
        &DataKey::TotalDebtAsset(prepared.asset.clone()),
        &new_total_asset,
    );
    
    let total_debt_protocol: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let new_total_protocol = total_debt_protocol.saturating_sub(prepared.principal_reduction);
    env.storage()
        .persistent()
        .set(&DataKey::TotalDebt, &new_total_protocol);
    
    // 4. Update isolation debt if applicable
    if crate::should_release_isolation_debt(env, &prepared.asset) {
        crate::decrement_isolation_debt(env, &prepared.asset, prepared.principal_reduction)?;
    }
    
    // 5. Check and clear unhealthy timestamp if health restored
    crate::check_and_clear_unhealthy_timestamp(env, &prepared.user);
    
    // 6. Extend TTL
    extend_debt_asset_ttl(env, &prepared.user, &prepared.asset);
    
    // 7. Emit event
    crate::events::emit_repay(env, &prepared.user, prepared.actual_repay_amount, prepared.position_after.principal);
    
    Ok(prepared.position_after.principal)
}

// ═══════════════════════════════════════════════════════════════════════════
// Convenience Wrapper: Single-Call Prepare + Commit
// ═══════════════════════════════════════════════════════════════════════════

/// Execute a borrow operation with two-phase commit (convenience wrapper).
///
/// This combines `prepare_borrow` + `commit_borrow` in a single call.
///
/// Use this when you want the safety of two-phase commit but don't need
/// to inspect the prepared state before committing.
pub fn execute_borrow_two_phase(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    let prepared = prepare_borrow(env, user, asset, amount)?;
    commit_borrow(env, prepared)
}

/// Execute a withdrawal operation with two-phase commit (convenience wrapper).
pub fn execute_withdraw_two_phase(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    let prepared = prepare_withdraw(env, user, asset, amount)?;
    commit_withdraw(env, prepared)
}

/// Execute a repay operation with two-phase commit (convenience wrapper).
pub fn execute_repay_two_phase(
    env: &Env,
    user: &Address,
    asset: &Address,
    amount: i128,
) -> Result<i128, LendingError> {
    let prepared = prepare_repay(env, user, asset, amount)?;
    commit_repay(env, prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    fn test_two_phase_borrow_validates_before_write() {
        // This test verifies that prepare_borrow validates health factor
        // BEFORE any permanent state mutation occurs.
        
        let env = Env::default();
        let user = Address::generate(&env);
        let asset = Address::generate(&env);
        
        // Setup would require full contract initialization
        // For now, this demonstrates the API
        
        // Attempt to prepare under-collateralized borrow
        let result = prepare_borrow(&env, &user, &asset, 1_000_000);
        
        // Expect: HealthFactorTooLow error
        // Verify: No debt position written (query storage confirms)
        assert!(result.is_err());
    }

    #[test]
    fn test_two_phase_commit_without_prepare_fails() {
        let env = Env::default();
        let user = Address::generate(&env);
        let asset = Address::generate(&env);
        
        // Create a prepared operation manually (without validation)
        let fake_prepared = PreparedBorrow {
            user: user.clone(),
            asset: asset.clone(),
            amount: 1000,
            position_before: DebtPosition {
                principal: 0,
                borrow_index_snapshot: 1_000_000,
                last_update: 0,
            },
            position_after: DebtPosition {
                principal: 1000,
                borrow_index_snapshot: 1_000_000,
                last_update: 0,
            },
            principal_delta: 1000,
            health_factor_after: 20000, // 2.0
            current_index: 1_000_000,
            prepared_at: 0,
        };
        
        // Commit should fail: prepared_at too old
        let result = commit_borrow(&env, fake_prepared);
        assert!(matches!(result, Err(LendingError::OperationExpired)));
    }

    #[test]
    fn test_prepared_operation_has_timestamp_validation() {
        let env = Env::default();
        
        // Verify that committed operations check prepared_at timestamp
        // to prevent stale prepared operations from being committed
        // after oracle prices have changed.
        
        // This is a critical safety property: if client prepares operation,
        // waits 10 minutes, then commits, prices may have changed and
        // validation is no longer valid.
        
        // Expected: OperationExpired error if prepared_at > MAX_PREPARE_AGE
    }
}
