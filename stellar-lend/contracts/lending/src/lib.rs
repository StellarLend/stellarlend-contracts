#![no_std]
#![allow(deprecated)]
#![allow(clippy::absurd_extreme_comparisons)]
#![allow(unexpected_cfgs)]

#[cfg(test)]
mod math_safety_test;
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, Val, Vec};
mod borrow;
mod constants;
mod cross_asset;
mod deposit;
mod flash_loan;
mod governance_audit;
mod liquidate;
mod oracle;
mod pause;
mod reentrancy;
mod token_receiver;
mod withdraw;
mod errors;
#[cfg(test)]
mod errors_test;

use errors::{BorrowError, CrossAssetError, DepositError, FlashLoanError, OracleError, WithdrawError};
use governance_audit::{
    get_audit_count, get_recent_audit_entries, log_governance_action, GovernanceAction,
    payload_address, payload_address_bool, payload_address_i128, payload_address_u64,
    payload_empty, payload_i128, payload_string, payload_two_addresses, payload_two_u64,
};

use borrow::{
    borrow as borrow_impl, credit_insurance_fund as credit_insurance_impl,
    deposit as borrow_deposit, get_admin as get_protocol_admin,
    get_close_factor_bps as get_close_factor_impl,
    get_insurance_fund_balance as get_insurance_fund_impl,
    get_liquidation_incentive_bps as get_liquidation_incentive_bps_impl,
    get_total_bad_debt as get_bad_debt_impl, get_user_collateral as get_borrow_collateral,
    get_user_debt as get_user_debt_impl, initialize_borrow_settings as init_borrow_settings_impl,
    offset_bad_debt as offset_bad_debt_impl, repay as borrow_repay,
    set_admin as set_protocol_admin, set_close_factor_bps as set_close_factor_impl,
    set_liquidation_incentive_bps as set_liquidation_incentive_bps_impl,
    set_liquidation_threshold_bps as set_liq_threshold_impl, set_oracle as set_oracle_impl,
    BorrowCollateral, DebtPosition, OracleSetEvent,
};
use cross_asset::{
    borrow_asset as cross_borrow_asset, deposit_collateral_asset as cross_deposit_collateral,
    get_cross_position_summary as cross_position_summary, initialize_admin as cross_init_admin,
    repay_asset as cross_repay_asset, set_asset_params as cross_set_asset_params,
    withdraw_asset as cross_withdraw_asset, AssetParams, PositionSummary,
};
use deposit::{
    deposit as deposit_impl, get_user_collateral as get_deposit_collateral_impl,
    initialize_deposit_settings as init_deposit_settings_impl, DepositCollateral,
};
use flash_loan::{
    flash_loan as flash_loan_impl, set_flash_loan_fee_bps as set_flash_loan_fee_impl,
};
use oracle::{OracleConfig, OracleConfigEvent, OracleError};
use governance_audit::{
    get_audit_count, get_recent_audit_entries, log_governance_action, GovernanceAction,
    payload_address, payload_address_bool, payload_address_i128, payload_address_u64,
    payload_empty, payload_i128, payload_string, payload_two_addresses, payload_two_u64,
};
use pause::{
    blocks_high_risk_ops, complete_recovery as complete_recovery_logic,
    get_emergency_state as get_emergency_state_logic, get_guardian as get_guardian_logic,
    get_pause_state as get_pause_state_logic, is_paused, is_read_only as is_read_only_logic,
    is_recovery, set_guardian as set_guardian_logic, set_pause as set_pause_impl,
    set_read_only as set_read_only_impl, start_recovery as start_recovery_logic,
    trigger_shutdown as trigger_shutdown_logic, EmergencyState, PauseType,
};
use token_receiver::receive as receive_impl;

mod interest_rate;
pub use interest_rate::{InterestRateConfig, InterestRateError};

mod views;
use views::{
    get_collateral_balance as view_collateral_balance,
    get_collateral_value as view_collateral_value, get_debt_balance as view_debt_balance,
    get_debt_value as view_debt_value, get_health_factor as view_health_factor,
    get_liquidation_incentive_amount as view_liquidation_incentive_amount,
    get_max_liquidatable_amount as view_max_liquidatable_amount,
    get_user_position as view_user_position, UserPositionSummary,
};

use withdraw::{
    initialize_withdraw_settings as initialize_withdraw_logic, withdraw as withdraw_logic,
};

mod data_store;
use stellarlend_common::upgrade;
pub use stellarlend_common::upgrade::{UpgradeError, UpgradeStage, UpgradeStatus};

#[cfg(test)]
mod borrow_test;
#[cfg(test)]
mod borrow_withdraw_sequence_adversarial_test;
// cross_asset_test targets a different contract API; disabled until migrated
// #[cfg(test)]
// mod cross_asset_test;
#[cfg(test)]
mod cross_asset_view_invariants_test;
#[cfg(test)]
mod deposit_test;
#[cfg(test)]
mod emergency_shutdown_test;
#[cfg(test)]
mod governance_audit_test;
#[cfg(test)]
mod emergency_lifecycle_conformance_test;
#[cfg(test)]
mod flash_adversarial_test;
#[cfg(test)]
mod flash_loan_test;
// mod pause_test; // temporarily disabled - pre-existing ContractEvents API mismatch
#[cfg(test)]
mod pause_matrix_test;
#[cfg(test)]
mod read_only_test;
#[cfg(test)]
mod token_receiver_test;
#[cfg(test)]
mod views_test;

// mod withdraw_test; // temporarily disabled - pre-existing ContractEvents API mismatch
#[cfg(test)]
mod bad_debt_test;
#[cfg(test)]
mod constants_test;
#[cfg(test)]
mod data_store_test;
#[cfg(test)]
mod liquidation_boundary_test;
#[cfg(test)]
mod math_safety_test;
#[cfg(test)]
mod multi_user_contention_test;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod proposal_race_test;
#[cfg(test)]
mod upgrade_migration_safety_test;
#[cfg(test)]
mod view_upgrade_consistency_test;
// #[cfg(test)]
// mod upgrade_test;
// #[cfg(test)]
// mod withdraw_test;
// 
// #[cfg(test)]
// mod bad_debt_test;
// #[cfg(test)]
// mod liquidation_boundary_test;
// #[cfg(test)]
// mod multi_user_contention_test;
// #[cfg(test)]
// mod stress_test;
#[cfg(test)]
mod upgrade_test;
// #[cfg(test)]
// mod withdraw_test;

#[cfg(test)]
mod zero_amount_semantics_test;
#[cfg(test)]
mod guardian_scope_test;
#[cfg(test)]
mod liquidation_boundary_test;
#[cfg(test)]
mod multi_user_contention_test;
#[cfg(test)]
mod stress_test;

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    /// Initialize the protocol with admin and settings
    pub fn initialize(
        env: Env,
        admin: Address,
        debt_ceiling: i128,
        min_borrow_amount: i128,
    ) -> Result<(), BorrowError> {
        if get_protocol_admin(&env).is_some() {
            return Err(BorrowError::Unauthorized);
        }
        set_protocol_admin(&env, &admin);
        init_borrow_settings_impl(&env, debt_ceiling, min_borrow_amount)?;
        
        // Log governance action
        let mut payload_data = Vec::new(&env);
        payload_data.push_back(admin.into_val(&env));
        payload_data.push_back(debt_ceiling.into_val(&env));
        payload_data.push_back(min_borrow_amount.into_val(&env));
        let payload = governance_audit::GovernancePayload { data: payload_data };
        log_governance_action(&env, GovernanceAction::Initialize, admin, payload);
        
        Ok(())
    }

    /// Register an asset in the allowlist (admin only).
    pub fn register_asset(env: Env, admin: Address, asset: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        let result = asset_registry::register(&env, &asset);
        if result.is_ok() {
            // Log governance action
            let payload = payload_address(&env, asset);
            log_governance_action(&env, GovernanceAction::SetAssetParams, admin, payload);
        }
        result
    }

    /// Remove an asset from the allowlist (admin only).
    pub fn deregister_asset(env: Env, admin: Address, asset: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        let result = asset_registry::deregister(&env, &asset);
        if result.is_ok() {
            // Log governance action
            let payload = payload_address(&env, asset);
            log_governance_action(&env, GovernanceAction::SetAssetParams, admin, payload);
        }
        result
    }

    /// Query whether an asset is registered (read-only).
    pub fn is_asset_registered(env: Env, asset: Address) -> bool {
        asset_registry::is_registered(&env, &asset)
    }

    /// Borrow assets against deposited collateral
    pub fn borrow(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
        collateral_asset: Address,
        collateral_amount: i128,
    ) -> Result<(), BorrowError> {
        let _guard = reentrancy::ReentrancyGuard::new(&env).map_err(|_| BorrowError::Reentrancy)?;
        if blocks_high_risk_ops(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
        borrow_impl(
            &env,
            user,
            asset,
            amount,
            collateral_asset,
            collateral_amount,
        )
    }

    /// Set protocol pause state for a specific operation (admin only)
    pub fn set_pause(
        env: Env,
        admin: Address,
        pause_type: PauseType,
        paused: bool,
    ) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        set_pause_impl(&env, admin, pause_type, paused);
        
        // Log governance action
        let mut payload_data = Vec::new(&env);
        payload_data.push_back((pause_type as u32).into_val(&env));
        payload_data.push_back(paused.into_val(&env));
        let payload = governance_audit::GovernancePayload { data: payload_data };
        log_governance_action(&env, GovernanceAction::SetPause, admin, payload);
        
        Ok(())
    }

    /// Toggle protocol-level read-only mode (admin only).
    pub fn set_read_only(env: Env, admin: Address, read_only: bool) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        set_read_only_impl(&env, admin, read_only);
        Ok(())
    }

    /// Return true if the protocol is currently in read-only mode.
    pub fn is_read_only(env: Env) -> bool {
        is_read_only_logic(&env)
    }

    /// Configure guardian address authorized to trigger emergency shutdown.
    pub fn set_guardian(env: Env, admin: Address, guardian: Address) -> Result<(), BorrowError> {
        admin.require_auth();
        let stored_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != stored_admin {
            return Err(BorrowError::Unauthorized);
        }
        set_guardian_logic(&env, admin, guardian);
        
        // Log governance action
        let payload = payload_address(&env, guardian);
        log_governance_action(&env, GovernanceAction::SetGuardian, admin, payload);
        
        Ok(())
    }

    /// Return current guardian address if configured.
    pub fn get_guardian(env: Env) -> Option<Address> {
        get_guardian_logic(&env)
    }

    /// Trigger emergency shutdown (admin or guardian).
    pub fn emergency_shutdown(env: Env, caller: Address) -> Result<(), BorrowError> {
        caller.require_auth();
        let admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        let guardian = get_guardian_logic(&env);

        if caller != admin && Some(caller.clone()) != guardian {
            return Err(BorrowError::Unauthorized);
        }

        trigger_shutdown_logic(&env, caller);
        
        // Log governance action
        let payload = payload_empty(&env);
        log_governance_action(&env, GovernanceAction::EmergencyShutdown, caller, payload);
        
        Ok(())
    }

    /// Move from hard shutdown into controlled user recovery.
    pub fn start_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        admin.require_auth();
        let stored_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != stored_admin {
            return Err(BorrowError::Unauthorized);
        }
        if get_emergency_state_logic(&env) != EmergencyState::Shutdown {
            return Err(BorrowError::ProtocolPaused);
        }
        start_recovery_logic(&env, admin);
        
        // Log governance action
        let payload = payload_empty(&env);
        log_governance_action(&env, GovernanceAction::StartRecovery, admin, payload);
        
        Ok(())
    }

    /// Return protocol to normal operation after recovery procedures.
    pub fn complete_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        admin.require_auth();
        let stored_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != stored_admin {
            return Err(BorrowError::Unauthorized);
        }
        complete_recovery_logic(&env, admin);
        
        // Log governance action
        let payload = payload_empty(&env);
        log_governance_action(&env, GovernanceAction::CompleteRecovery, admin, payload);
        
        Ok(())
    }

    /// Read current emergency lifecycle state.
    pub fn get_emergency_state(env: Env) -> EmergencyState {
        get_emergency_state_logic(&env)
    }

    /// Query whether a specific operation is currently paused.
    ///
    /// Returns `true` if the operation is paused either by its own granular flag
    /// or by the global `All` flag. This is a read-only function; no authorization
    /// is required. Frontends and off-chain monitors should use this to surface
    /// live pause state to users before they attempt a transaction.
    ///
    /// # Arguments
    /// * `pause_type` - The operation type to query (`Deposit`, `Borrow`, `Repay`,
    ///                  `Withdraw`, `Liquidation`, or `All`)
    pub fn get_pause_state(env: Env, pause_type: PauseType) -> bool {
        get_pause_state_logic(&env, pause_type)
    }

    /// Repay borrowed assets
    pub fn repay(env: Env, user: Address, asset: Address, amount: i128) -> Result<(), BorrowError> {
        let _guard = reentrancy::ReentrancyGuard::new(&env).map_err(|_| BorrowError::Reentrancy)?;
        user.require_auth();
        if is_read_only_logic(&env)
            || is_paused(&env, PauseType::Repay)
            || (!is_recovery(&env) && blocks_high_risk_ops(&env))
        {
            return Err(BorrowError::ProtocolPaused);
        }
        borrow_repay(&env, user, asset, amount)
    }

    /// Deposit collateral for a borrow position
    pub fn deposit_collateral(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        let _guard = reentrancy::ReentrancyGuard::new(&env).map_err(|_| BorrowError::Reentrancy)?;
        user.require_auth();
        if is_read_only_logic(&env)
            || is_paused(&env, PauseType::Deposit)
            || blocks_high_risk_ops(&env)
        {
            return Err(BorrowError::ProtocolPaused);
        }
        borrow_deposit(&env, user, asset, amount)
    }

    /// Deposit collateral into the protocol
    pub fn deposit(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, DepositError> {
        let _guard =
            reentrancy::ReentrancyGuard::new(&env).map_err(|_| DepositError::Reentrancy)?;
        if is_paused(&env, PauseType::Deposit) || blocks_high_risk_ops(&env) {
            return Err(DepositError::DepositPaused);
        }
        deposit_impl(&env, user, asset, amount)
    }

    /// Liquidate a position
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        let _guard = reentrancy::ReentrancyGuard::new(&env).map_err(|_| BorrowError::Reentrancy)?;
        liquidator.require_auth();
        if is_read_only_logic(&env)
            || is_paused(&env, PauseType::Liquidation)
            || blocks_high_risk_ops(&env)
        {
            return Err(BorrowError::ProtocolPaused);
        }

        // Delegate to the full liquidation implementation which enforces
        // close-factor capping, incentive-based collateral seizure, health
        // factor eligibility checks, and post-liquidation event emission.
        liquidate::liquidate_position(
            &env,
            liquidator,
            borrower,
            debt_asset,
            collateral_asset,
            amount,
        )?;

        Ok(())
    }

    /// Returns the insurance fund balance for an asset.
    pub fn get_insurance_fund_balance(env: Env, asset: Address) -> i128 {
        get_insurance_fund_impl(&env, &asset)
    }

    /// Returns the total bad debt recorded for an asset.
    pub fn get_total_bad_debt(env: Env, asset: Address) -> i128 {
        get_bad_debt_impl(&env, &asset)
    }

    /// Credits the insurance fund for an asset (Admin only).
    pub fn credit_insurance_fund(
        env: Env,
        caller: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        caller.require_auth();
        let admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if caller != admin {
            return Err(BorrowError::Unauthorized);
        }
        if is_read_only_logic(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
        credit_insurance_impl(&env, &asset, amount)
    }

    /// Manually offsets bad debt using the insurance fund (Admin only).
    pub fn offset_bad_debt(
        env: Env,
        caller: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        caller.require_auth();
        let admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if caller != admin {
            return Err(BorrowError::Unauthorized);
        }
        if is_read_only_logic(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
        offset_bad_debt_impl(&env, &asset, amount)
    }

    /// Returns gas/performance stats for the current transaction (Issue #391)
    /// [CPU Instructions, Memory Bytes]
    #[cfg(not(tarpaulin_include))]
    pub fn get_performance_stats(env: Env) -> Vec<u64> {
        let mut stats = Vec::new(&env);
        // Runtime budget counters are only available in testutils.
        // Keep a stable ABI by returning placeholder values in production builds.
        stats.push_back(0);
        stats.push_back(0);
        stats
    }

    /// Get user's debt position
    pub fn get_user_debt(env: &Env, user: &Address) -> DebtPosition {
        let mut position = get_debt_position(env, user);
        let accrued = calculate_interest(env, &position);
        position.interest_accrued = position.interest_accrued.saturating_add(accrued);
        position
    }
pub(crate) fn calculate_interest(env: &Env, position: &DebtPosition) -> i128 {
    if position.borrowed_amount == 0 {
        return 0;
    }

    let time_elapsed = env
        .ledger()
        .timestamp()
        .saturating_sub(position.last_update);

    let borrowed_256 = I256::from_i128(env, position.borrowed_amount);
    let rate_256 = I256::from_i128(env, INTEREST_RATE_PER_YEAR);
    let time_256 = I256::from_i128(env, time_elapsed as i128);

    let denominator =
        I256::from_i128(env, 10000).mul(&I256::from_i128(env, SECONDS_PER_YEAR as i128));

    let numerator = borrowed_256.mul(&rate_256).mul(&time_256);

    let interest_256 = if numerator > I256::from_i128(env, 0) {
        numerator
            .add(&denominator.sub(&I256::from_i128(env, 1)))
            .div(&denominator)
    } else {
        numerator.div(&denominator)
    };

    interest_256.to_i128().unwrap_or(i128::MAX)
}

    /// Get user's collateral position (borrow module)
    pub fn get_user_collateral(env: Env, user: Address) -> BorrowCollateral {
        get_borrow_collateral(&env, &user)
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // View functions (read-only; for frontends and liquidations)
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    /// Returns the user's collateral balance (raw amount).
    pub fn get_collateral_balance(env: Env, user: Address) -> i128 {
        view_collateral_balance(&env, &user)
    }

    /// Returns the user's debt balance (principal + accrued interest).
    pub fn get_debt_balance(env: Env, user: Address) -> i128 {
        view_debt_balance(&env, &user)
    }

    /// Returns the user's collateral value in common unit (e.g. USD 8 decimals). 0 if oracle not set.
    pub fn get_collateral_value(env: Env, user: Address) -> i128 {
        view_collateral_value(&env, &user)
    }

    /// Returns the user's debt value in common unit. 0 if oracle not set.
    pub fn get_debt_value(env: Env, user: Address) -> i128 {
        view_debt_value(&env, &user)
    }

    /// Returns health factor (scaled 10000 = 1.0). Above 10000 = healthy; below = liquidatable.
    pub fn get_health_factor(env: Env, user: Address) -> i128 {
        view_health_factor(&env, &user)
    }

    /// Returns full position summary: collateral/debt balances and values, and health factor.
    pub fn get_user_position(env: Env, user: Address) -> UserPositionSummary {
        view_user_position(&env, &user)
    }

    /// Set oracle address for price feeds (admin only).
    pub fn set_oracle(env: Env, admin: Address, oracle: Address) -> Result<(), BorrowError> {
        if is_read_only_logic(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
        set_oracle_impl(&env, &admin, oracle)
    }

    /// Configure oracle staleness parameters (admin only).
    ///
    /// # Errors
    /// - `OracleError::Unauthorized` â€” caller is not the protocol admin.
    /// - `OracleError::InvalidPrice` â€” `max_staleness_seconds` is zero.
    pub fn configure_oracle(
        env: Env,
        caller: Address,
        config: OracleConfig,
    ) -> Result<(), OracleError> {
        if is_read_only_logic(&env) {
            return Err(OracleError::OraclePaused);
        }
        oracle::configure_oracle(&env, caller, config)
    }

    /// Register the primary oracle address for `asset` (admin only).
    ///
    /// # Errors
    /// - `OracleError::Unauthorized` â€” caller is not the protocol admin.
    /// - `OracleError::InvalidOracle` â€” oracle address is the contract itself.
    pub fn set_primary_oracle(
        env: Env,
        caller: Address,
        asset: Address,
        primary_oracle: Address,
    ) -> Result<(), OracleError> {
        if is_read_only_logic(&env) {
            return Err(OracleError::OraclePaused);
        }
        oracle::set_primary_oracle(&env, caller, asset, primary_oracle)
    }

    /// Register the fallback oracle address for `asset` (admin only).
    ///
    /// # Errors
    /// - `OracleError::Unauthorized` â€” caller is not the protocol admin.
    /// - `OracleError::InvalidOracle` â€” oracle address is the contract itself.
    pub fn set_fallback_oracle(
        env: Env,
        caller: Address,
        asset: Address,
        fallback_oracle: Address,
    ) -> Result<(), OracleError> {
        if is_read_only_logic(&env) {
            return Err(OracleError::OraclePaused);
        }
        let result = oracle::set_fallback_oracle(&env, caller, asset, fallback_oracle);
        if result.is_ok() {
            // Log governance action
            let payload = payload_two_addresses(&env, asset, fallback_oracle);
            log_governance_action(&env, GovernanceAction::SetFallbackOracle, caller, payload);
        }
        result
    }

    /// Submit a price update for `asset`.
    ///
    /// Caller must be the admin, the registered primary oracle, or the registered
    /// fallback oracle for this asset.
    ///
    /// # Errors
    /// - `OracleError::OraclePaused` â€” oracle updates are paused.
    /// - `OracleError::Unauthorized` â€” caller is not authorized.
    /// - `OracleError::InvalidPrice` â€” price is zero or negative.
    pub fn update_price_feed(
        env: Env,
        caller: Address,
        asset: Address,
        price: i128,
    ) -> Result<(), OracleError> {
        if is_read_only_logic(&env) {
            return Err(OracleError::OraclePaused);
        }
        let result = oracle::update_price_feed(&env, caller, asset, price);
        if result.is_ok() {
            // Log governance action
            let payload = payload_address_asset_i128(&env, asset, asset, price);
            log_governance_action(&env, GovernanceAction::UpdatePriceFeed, caller, payload);
        }
        result
    }

    /// Get the current price for `asset` (primary â†’ fallback â†’ error).
    ///
    /// # Errors
    /// - `OracleError::StalePrice` â€” best available price is stale.
    /// - `OracleError::NoPriceFeed` â€” no price has been submitted for this asset.
    pub fn get_price(env: Env, asset: Address) -> Result<i128, OracleError> {
        oracle::get_price(&env, &asset)
    }

    /// Pause or unpause oracle price updates (admin only).
    pub fn set_oracle_paused(env: Env, caller: Address, paused: bool) -> Result<(), OracleError> {
        if is_read_only_logic(&env) {
            return Err(OracleError::OraclePaused);
        }
        let result = oracle::set_oracle_paused(&env, caller, paused);
        if result.is_ok() {
            // Log governance action
            let payload = payload_address_bool(&env, caller, paused);
            log_governance_action(&env, GovernanceAction::SetOraclePaused, caller, payload);
        }
        result
    }

    /// Set a per-asset maximum staleness override (admin only).
    ///
    /// Overrides the global `OracleConfig.max_staleness_seconds` for `asset`.
    /// Useful when different assets have different oracle update cadences.
    ///
    /// # Errors
    /// - `OracleError::Unauthorized` — caller is not the protocol admin.
    /// - `OracleError::InvalidPrice` — `max_staleness_seconds` is zero.
    pub fn set_asset_max_staleness(
        env: Env,
        caller: Address,
        asset: Address,
        max_staleness_seconds: u64,
    ) -> Result<(), OracleError> {
        oracle::set_asset_max_staleness(&env, caller, asset, max_staleness_seconds)
    }

    /// Remove the per-asset staleness override for `asset` (admin only).
    ///
    /// After this call the global `OracleConfig.max_staleness_seconds` applies.
    ///
    /// # Errors
    /// - `OracleError::Unauthorized` — caller is not the protocol admin.
    pub fn clear_asset_max_staleness(
        env: Env,
        caller: Address,
        asset: Address,
    ) -> Result<(), OracleError> {
        oracle::clear_asset_max_staleness(&env, caller, asset)
    }

    /// Return the effective max-staleness for `asset` in seconds.
    ///
    /// Returns the per-asset override if set, otherwise the global config value
    /// (default 3 600 s).
    pub fn get_asset_max_staleness(env: Env, asset: Address) -> u64 {
        oracle::get_asset_max_staleness(&env, &asset)
    }

    /// Set liquidation threshold in basis points, e.g. 8000 = 80% (admin only).
    pub fn set_liquidation_threshold_bps(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        let result = set_liq_threshold_impl(&env, &admin, bps);
        if result.is_ok() {
            // Log governance action
            let payload = payload_i128(&env, bps);
            log_governance_action(&env, GovernanceAction::SetLiquidationThreshold, admin, payload);
        }
        result
    }

    /// Returns the close factor in basis points (default 5000 = 50%).
    /// Max fraction of a debt position that can be liquidated per call.
    pub fn get_close_factor_bps(env: Env) -> i128 {
        get_close_factor_impl(&env)
    }

    /// Sets the close factor in basis points (1â€“10000). Admin only.
    pub fn set_close_factor_bps(env: Env, admin: Address, bps: i128) -> Result<(), BorrowError> {
        let result = set_close_factor_impl(&env, &admin, bps);
        if result.is_ok() {
            // Log governance action
            let payload = payload_i128(&env, bps);
            log_governance_action(&env, GovernanceAction::SetCloseFactor, admin, payload);
        }
        result
    }

    /// Returns the liquidation incentive in basis points (default 1000 = 10%).
    pub fn get_liquidation_incentive_bps(env: Env) -> i128 {
        get_liquidation_incentive_bps_impl(&env)
    }

    /// Sets the liquidation incentive in basis points (0â€“10000). Admin only.
    pub fn set_liquidation_incentive_bps(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        let result = set_liquidation_incentive_bps_impl(&env, &admin, bps);
        if result.is_ok() {
            // Log governance action
            let payload = payload_i128(&env, bps);
            log_governance_action(&env, GovernanceAction::SetLiquidationIncentive, admin, payload);
        }
        result
    }

    /// Returns the maximum debt that can be liquidated for `user` in one call.
    /// Returns 0 if healthy, no debt, or oracle not configured.
    pub fn get_max_liquidatable_amount(env: Env, user: Address) -> i128 {
        view_max_liquidatable_amount(&env, &user)
    }

    /// Returns the collateral bonus amount a liquidator receives for repaying `repay_amount`.
    /// Formula: repay_amount * (10000 + incentive_bps) / 10000
    pub fn get_liquidation_incentive_amount(env: Env, repay_amount: i128) -> i128 {
        view_liquidation_incentive_amount(&env, repay_amount)
    }

    /// Initialize borrow settings (admin only)
    #[cfg(not(tarpaulin_include))]
    pub fn initialize_borrow_settings(
        env: Env,
        debt_ceiling: i128,
        min_borrow_amount: i128,
    ) -> Result<(), BorrowError> {
        if is_read_only_logic(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        current_admin.require_auth();
        let result = init_borrow_settings_impl(&env, debt_ceiling, min_borrow_amount);
        if result.is_ok() {
            // Log governance action
            let mut payload_data = Vec::new(&env);
            payload_data.push_back(debt_ceiling.into_val(&env));
            payload_data.push_back(min_borrow_amount.into_val(&env));
            let payload = governance_audit::GovernancePayload { data: payload_data };
            log_governance_action(&env, GovernanceAction::InitializeBorrowSettings, current_admin, payload);
        }
        result
    }

    /// Initialize deposit settings (admin only)
    pub fn initialize_deposit_settings(
        env: Env,
        deposit_cap: i128,
        min_deposit_amount: i128,
    ) -> Result<(), DepositError> {
        if is_read_only_logic(&env) {
            return Err(DepositError::DepositPaused);
        }
        let current_admin = get_protocol_admin(&env).ok_or(DepositError::Unauthorized)?;
        current_admin.require_auth();
        let result = init_deposit_settings_impl(&env, deposit_cap, min_deposit_amount);
        if result.is_ok() {
            // Log governance action
            let mut payload_data = Vec::new(&env);
            payload_data.push_back(deposit_cap.into_val(&env));
            payload_data.push_back(min_deposit_amount.into_val(&env));
            let payload = governance_audit::GovernancePayload { data: payload_data };
            log_governance_action(&env, GovernanceAction::InitializeDepositSettings, current_admin, payload);
        }
        result
    }

    /// Set deposit pause state (admin only)
    #[cfg(not(tarpaulin_include))]
    /// Set deposit pause state (admin only).
    ///
    /// Convenience wrapper around [`set_pause`] scoped to `PauseType::Deposit`.
    /// Emits a `pause_event` so off-chain monitors can react.
    ///
    /// # Errors
    /// Returns [`DepositError::Unauthorized`] if the caller is not the admin.
    pub fn set_deposit_paused(env: Env, admin: Address, paused: bool) -> Result<(), DepositError> {
        ensure_admin(&env, &admin).map_err(|_| DepositError::Unauthorized)?;
        set_pause_impl(&env, admin, PauseType::Deposit, paused);
        Ok(())
    }

    /// Get user's deposit collateral position
    pub fn get_user_collateral_deposit(
        env: Env,
        user: Address,
        asset: Address,
    ) -> DepositCollateral {
        get_deposit_collateral_impl(&env, &user, &asset)
    }
    /// Set protocol admin (admin only)
    pub fn set_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &current_admin)?;
        set_protocol_admin(&env, &new_admin);
        
        // Log governance action
        let payload = payload_address(&env, new_admin);
        log_governance_action(&env, GovernanceAction::SetAdmin, current_admin, payload);
        
        Ok(())
    }

    /// Get protocol admin
    #[cfg(not(tarpaulin_include))]
    pub fn get_admin(env: Env) -> Option<Address> {
        get_protocol_admin(&env)
    }

    /// Execute a flash loan
    #[cfg(not(tarpaulin_include))]
    pub fn flash_loan(
        env: Env,
        receiver: Address,
        asset: Address,
        amount: i128,
        params: Bytes,
    ) -> Result<(), FlashLoanError> {
        if is_read_only_logic(&env) || is_paused(&env, PauseType::All) || blocks_high_risk_ops(&env)
        {
            return Err(FlashLoanError::ProtocolPaused);
        }
        flash_loan_impl(&env, receiver, asset, amount, params)
    }

    /// Set the flash loan fee in basis points (admin only)
    pub fn set_flash_loan_fee_bps(env: Env, fee_bps: i128) -> Result<(), FlashLoanError> {
        let current_admin = get_protocol_admin(&env).ok_or(FlashLoanError::Unauthorized)?;
        current_admin.require_auth();
        let result = set_flash_loan_fee_impl(&env, fee_bps);
        if result.is_ok() {
            // Log governance action
            let payload = payload_i128(&env, fee_bps);
            log_governance_action(&env, GovernanceAction::SetFlashLoanFee, current_admin, payload);
        }
        result
    }

    /// Withdraw collateral from the protocol.
    ///
    /// Pause, emergency shutdown vs recovery, legacy withdraw flag, and collateral-ratio checks
    /// are enforced inside [`withdraw::withdraw`] so behavior stays aligned with the pause module.
    pub fn withdraw(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, WithdrawError> {
        let _guard =
            reentrancy::ReentrancyGuard::new(&env).map_err(|_| WithdrawError::Reentrancy)?;
        withdraw_logic(&env, user, asset, amount)
    }

    /// Initialize withdraw settings (admin only)
    pub fn initialize_withdraw_settings(
        env: Env,
        min_withdraw_amount: i128,
    ) -> Result<(), WithdrawError> {
        let current_admin = get_protocol_admin(&env).ok_or(WithdrawError::Unauthorized)?;
        current_admin.require_auth();
        let result = initialize_withdraw_logic(&env, min_withdraw_amount);
        if result.is_ok() {
            // Log governance action
            let payload = payload_i128(&env, min_withdraw_amount);
            log_governance_action(&env, GovernanceAction::InitializeWithdrawSettings, current_admin, payload);
        }
        result
    }

    /// Set withdraw pause state (admin only).
    ///
    /// Convenience wrapper around [`set_pause`] scoped to `PauseType::Withdraw`.
    /// Emits a `pause_event` so off-chain monitors can react.
    ///
    /// # Errors
    /// Returns [`WithdrawError::Unauthorized`] if the caller is not the admin.
    pub fn set_withdraw_paused(
        env: Env,
        admin: Address,
        paused: bool,
    ) -> Result<(), WithdrawError> {
        ensure_admin(&env, &admin).map_err(|_| WithdrawError::Unauthorized)?;
        set_pause_impl(&env, admin, PauseType::Withdraw, paused);
        Ok(())
    }

    /// Token receiver hook
    pub fn receive(
        env: Env,
        token_asset: Address,
        from: Address,
        amount: i128,
        payload: Vec<Val>,
    ) -> Result<(), BorrowError> {
        // Reentrancy guard - prevents callback-based reentry during token transfers
        let _guard = reentrancy::ReentrancyGuard::new(&env).map_err(|_| BorrowError::Reentrancy)?;

        receive_impl(env, token_asset, from, amount, payload)
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Upgrade Management (Governance)
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    pub fn upgrade_init(
        env: Env,
        admin: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) {
        upgrade::UpgradeManager::init(env, admin, current_wasm_hash, required_approvals);
        
        // Log governance action
        let mut payload_data = Vec::new(&env);
        payload_data.push_back(current_wasm_hash.into_val(&env));
        payload_data.push_back(required_approvals.into_val(&env));
        let payload = governance_audit::GovernancePayload { data: payload_data };
        log_governance_action(&env, GovernanceAction::UpgradeInit, admin, payload);
    }

    pub fn upgrade_add_approver(env: Env, caller: Address, approver: Address) {
        upgrade::UpgradeManager::add_approver(env, caller, approver);
        
        // Log governance action
        let payload = payload_two_addresses(&env, caller, approver);
        log_governance_action(&env, GovernanceAction::UpgradeAddApprover, caller, payload);
    }

    pub fn upgrade_remove_approver(env: Env, caller: Address, approver: Address) {
        upgrade::UpgradeManager::remove_approver(env, caller, approver);
        
        // Log governance action
        let payload = payload_two_addresses(&env, caller, approver);
        log_governance_action(&env, GovernanceAction::UpgradeRemoveApprover, caller, payload);
    }

    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> u64 {
        let proposal_id = upgrade::UpgradeManager::upgrade_propose(env, caller, new_wasm_hash, new_version);
        
        // Log governance action
        let mut payload_data = Vec::new(&env);
        payload_data.push_back(new_wasm_hash.into_val(&env));
        payload_data.push_back(new_version.into_val(&env));
        payload_data.push_back(proposal_id.into_val(&env));
        let payload = governance_audit::GovernancePayload { data: payload_data };
        log_governance_action(&env, GovernanceAction::UpgradePropose, caller, payload);
        
        proposal_id
    }

    pub fn upgrade_approve(env: Env, caller: Address, proposal_id: u64) -> u32 {
        let approval_count = upgrade::UpgradeManager::upgrade_approve(env, caller, proposal_id);
        
        // Log governance action
        let payload = payload_two_u64(&env, proposal_id, approval_count as u64);
        log_governance_action(&env, GovernanceAction::UpgradeApprove, caller, payload);
        
        approval_count
    }

    pub fn upgrade_execute(env: Env, caller: Address, proposal_id: u64) {
        upgrade::UpgradeManager::upgrade_execute(env, caller, proposal_id);
        
        // Log governance action
        let payload = payload_u64(&env, proposal_id);
        log_governance_action(&env, GovernanceAction::UpgradeExecute, caller, payload);
    }

    pub fn upgrade_rollback(env: Env, caller: Address, proposal_id: u64) {
        upgrade::UpgradeManager::upgrade_rollback(env, caller, proposal_id);
        
        // Log governance action
        let payload = payload_u64(&env, proposal_id);
        log_governance_action(&env, GovernanceAction::UpgradeRollback, caller, payload);
    }

    pub fn upgrade_status(env: Env, proposal_id: u64) -> upgrade::UpgradeStatus {
        upgrade::UpgradeManager::upgrade_status(env, proposal_id)
    }

    pub fn current_wasm_hash(env: Env) -> BytesN<32> {
        upgrade::UpgradeManager::current_wasm_hash(env)
    }

    pub fn current_version(env: Env) -> u32 {
        upgrade::UpgradeManager::current_version(env)
    }

    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    // Data Store Management
    // â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[cfg(not(tarpaulin_include))]
    pub fn data_store_init(env: Env, admin: Address) {
        if env.storage().persistent().has(&data_store::StoreKey::StoreAdmin) {
            return;
        }
        data_store::DataStore::init(env, admin);
    }

    pub fn data_grant_writer(env: Env, caller: Address, writer: Address) {
        data_store::DataStore::grant_writer(env, caller, writer);
        
        // Log governance action
        let payload = payload_two_addresses(&env, caller, writer);
        log_governance_action(&env, GovernanceAction::GrantDataWriter, caller, payload);
    }

    #[cfg(not(tarpaulin_include))]
    pub fn data_revoke_writer(env: Env, caller: Address, writer: Address) {
        data_store::DataStore::revoke_writer(env, caller, writer);
        
        // Log governance action
        let payload = payload_two_addresses(&env, caller, writer);
        log_governance_action(&env, GovernanceAction::RevokeDataWriter, caller, payload);
    }

    #[cfg(not(tarpaulin_include))]
    pub fn data_save(env: Env, caller: Address, key: soroban_sdk::String, value: Bytes) {
        data_store::DataStore::data_save(env, caller, key, value);
    }

    pub fn data_load(env: Env, key: soroban_sdk::String) -> Bytes {
        data_store::DataStore::data_load(env, key)
    }

    pub fn data_backup(env: Env, caller: Address, backup_name: soroban_sdk::String) {
        data_store::DataStore::data_backup(env, caller, backup_name);
        
        // Log governance action
        let payload = payload_string(&env, backup_name);
        log_governance_action(&env, GovernanceAction::DataBackup, caller, payload);
    }

    pub fn data_restore(env: Env, caller: Address, backup_name: soroban_sdk::String) {
        data_store::DataStore::data_restore(env, caller, backup_name);
        
        // Log governance action
        let payload = payload_string(&env, backup_name);
        log_governance_action(&env, GovernanceAction::DataRestore, caller, payload);
    }

    pub fn data_migrate_bump_version(
        env: Env,
        caller: Address,
        new_version: u32,
        memo: soroban_sdk::String,
    ) {
        data_store::DataStore::data_migrate_bump_version(env, caller, new_version, Some(memo));
        
        // Log governance action
        let mut payload_data = Vec::new(&env);
        payload_data.push_back(new_version.into_val(&env));
        payload_data.push_back(memo.into_val(&env));
        let payload = governance_audit::GovernancePayload { data: payload_data };
        log_governance_action(&env, GovernanceAction::DataMigrate, caller, payload);
    }

    pub fn data_schema_version(env: Env) -> u32 {
        data_store::DataStore::schema_version(env)
    }

    #[cfg(not(tarpaulin_include))]
    pub fn data_entry_count(env: Env) -> u32 {
        data_store::DataStore::entry_count(env)
    }

    #[cfg(not(tarpaulin_include))]
    pub fn data_key_exists(env: Env, key: soroban_sdk::String) -> bool {
        data_store::DataStore::key_exists(env, key)
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Cross-Asset Operations
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    /// Initialize admin for cross-asset operations
    pub fn initialize_admin(env: Env, admin: Address) -> Result<(), CrossAssetError> {
        cross_init_admin(&env, admin);
        Ok(())
    }

    /// Set parameters for a specific asset (admin only)
    pub fn set_asset_params(
        env: Env,
        asset: Address,
        params: AssetParams,
    ) -> Result<(), CrossAssetError> {
        if is_read_only_logic(&env) {
            return Err(CrossAssetError::ProtocolPaused);
        }
        cross_set_asset_params(&env, asset, params)
    }

    /// Deposit collateral for a specific asset
    pub fn deposit_collateral_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        if is_read_only_logic(&env) {
            return Err(CrossAssetError::ProtocolPaused);
        }
        cross_deposit_collateral(&env, user, asset, amount)
    }

    /// Borrow a specific asset against cross-asset collateral
    pub fn borrow_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        if is_read_only_logic(&env) {
            return Err(CrossAssetError::ProtocolPaused);
        }
        cross_borrow_asset(&env, user, asset, amount)
    }

    /// Repay debt for a specific asset
    pub fn repay_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        if is_read_only_logic(&env) {
            return Err(CrossAssetError::ProtocolPaused);
        }
        cross_repay_asset(&env, user, asset, amount)
    }

    /// Withdraw collateral for a specific asset
    pub fn withdraw_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        if is_read_only_logic(&env) {
            return Err(CrossAssetError::ProtocolPaused);
        }
        cross_withdraw_asset(&env, user, asset, amount)
    }

    /// Get cross-asset position summary for a user
    pub fn get_cross_position_summary(
        env: Env,
        user: Address,
    ) -> Result<PositionSummary, CrossAssetError> {
        cross_position_summary(&env, user)
    }

    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
    // Governance Audit Log Functions
    // â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

    /// Get recent governance audit entries.
    ///
    /// Returns up to `limit` most recent audit entries in reverse chronological
    /// order (newest first). Useful for monitoring and compliance.
    ///
    /// # Arguments
    /// * `limit` - Maximum number of entries to return (1-100)
    ///
    /// # Returns
    /// Vector of audit entries ordered from newest to oldest
    ///
    /// # Security
    /// This function is read-only and requires no authorization.
    pub fn get_governance_audit_entries(env: Env, limit: u32) -> Vec<governance_audit::AuditEntry> {
        get_recent_audit_entries(&env, limit)
    }

    /// Get the total count of governance audit entries.
    ///
    /// Returns the total number of governance actions that have been logged
    /// since contract deployment. Useful for pagination.
    ///
    /// # Returns
    /// Total count of audit entries
    ///
    /// # Security
    /// This function is read-only and requires no authorization.
    pub fn get_governance_audit_count(env: Env) -> u64 {
        get_audit_count(&env)
    }
}

fn ensure_admin(env: &Env, admin: &Address) -> Result<(), BorrowError> {
    let current_admin = get_protocol_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *admin != current_admin {
        return Err(BorrowError::Unauthorized);
    }
    admin.require_auth();
    Ok(())
}

fn ensure_shutdown_authorized(env: &Env, caller: &Address) -> Result<(), BorrowError> {
    let admin = get_protocol_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *caller == admin {
        return Ok(());
    }

    let guardian = get_guardian_logic(env).ok_or(BorrowError::Unauthorized)?;
    if *caller != guardian {
        return Err(BorrowError::Unauthorized);
    }

    Ok(())
}
