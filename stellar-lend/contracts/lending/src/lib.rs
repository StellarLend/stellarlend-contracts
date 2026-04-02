#![no_std]
#![allow(deprecated)]
#![allow(clippy::absurd_extreme_comparisons)]
#![allow(unexpected_cfgs)]
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, Val, Vec};
mod borrow;
mod constants;
mod cross_asset;
mod deposit;
mod flash_loan;
mod liquidate;
mod oracle;
mod pause;
mod token_receiver;
mod withdraw;

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
    BorrowCollateral, BorrowError, DebtPosition,
};
use cross_asset::{
    borrow_asset as cross_borrow_asset, deposit_collateral_asset as cross_deposit_collateral,
    get_cross_position_summary as cross_position_summary, initialize_admin as cross_init_admin,
    repay_asset as cross_repay_asset, set_asset_params as cross_set_asset_params,
    withdraw_asset as cross_withdraw_asset, AssetParams, CrossAssetError, PositionSummary,
};
use deposit::{
    deposit as deposit_impl, get_user_collateral as get_deposit_collateral_impl,
    initialize_deposit_settings as init_deposit_settings_impl, DepositCollateral, DepositError,
};
use flash_loan::{
    flash_loan as flash_loan_impl, set_flash_loan_fee_bps as set_flash_loan_fee_impl,
    FlashLoanError,
};
use oracle::{OracleConfig, OracleError};
use pause::{
    blocks_high_risk_ops, complete_recovery as complete_recovery_logic,
    get_emergency_state as get_emergency_state_logic, get_guardian as get_guardian_logic,
    get_pause_state as get_pause_state_logic, is_paused, is_recovery,
    set_guardian as set_guardian_logic, set_pause as set_pause_impl,
    start_recovery as start_recovery_logic, trigger_shutdown as trigger_shutdown_logic,
    EmergencyState, PauseType,
};
use token_receiver::receive as receive_impl;

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
    WithdrawError,
};

mod data_store;
use stellarlend_common::upgrade;
pub use stellarlend_common::upgrade::{UpgradeError, UpgradeStage, UpgradeStatus};

#[cfg(test)]
mod borrow_test;
#[cfg(test)]
mod cross_asset_test;
#[cfg(test)]
mod deposit_test;
#[cfg(test)]
mod emergency_shutdown_test;
#[cfg(test)]
mod flash_adversarial_test;
#[cfg(test)]
mod flash_loan_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod token_receiver_test;
#[cfg(test)]
mod views_test;

#[cfg(test)]
mod constants_test;
#[cfg(test)]
mod data_store_test;
#[cfg(test)]
mod math_safety_test;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod upgrade_migration_safety_test;
#[cfg(test)]
mod upgrade_test;
#[cfg(test)]
mod withdraw_test;

#[cfg(test)]
mod bad_debt_test;
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
    /// Initialize the protocol with admin address and global risk parameters.
    ///
    /// Must be called exactly once after deployment. Subsequent calls return
    /// [`BorrowError::Unauthorized`] because an admin is already registered.
    ///
    /// # Arguments
    /// * `admin` - Address that becomes the protocol administrator.
    /// * `debt_ceiling` - Maximum aggregate debt the protocol will allow (raw token units).
    /// * `min_borrow_amount` - Minimum per-position borrow size; protects against dust attacks.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — protocol has already been initialized.
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
        Ok(())
    }

    /// Borrow assets against deposited collateral.
    ///
    /// Blocked when the protocol is in emergency shutdown or when the `Borrow`
    /// pause flag is active. The caller must have previously deposited sufficient
    /// collateral to satisfy the loan-to-value ratio.
    ///
    /// # Arguments
    /// * `user` - Address of the borrower; must authorise this call.
    /// * `asset` - Token address to borrow.
    /// * `amount` - Amount to borrow (raw token units, must be positive).
    /// * `collateral_asset` - Token address posted as collateral.
    /// * `collateral_amount` - Amount of collateral to lock (raw token units).
    ///
    /// # Errors
    /// - [`BorrowError::ProtocolPaused`] — protocol is in shutdown or borrow is paused.
    /// - Other [`BorrowError`] variants propagated from the borrow module.
    pub fn borrow(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
        collateral_asset: Address,
        collateral_amount: i128,
    ) -> Result<(), BorrowError> {
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

    /// Set protocol pause state for a specific operation (admin only).
    ///
    /// Use `PauseType::All` to freeze the entire protocol. Individual variants
    /// (`Deposit`, `Borrow`, `Repay`, `Withdraw`, `Liquidation`) freeze only
    /// that operation. Emits a pause event that off-chain monitors can observe.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    /// * `pause_type` - Which operation to pause or unpause.
    /// * `paused` - `true` to pause, `false` to unpause.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn set_pause(
        env: Env,
        admin: Address,
        pause_type: PauseType,
        paused: bool,
    ) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        set_pause_impl(&env, admin, pause_type, paused);
        Ok(())
    }

    /// Configure the guardian address authorized to trigger emergency shutdown.
    ///
    /// The guardian is a secondary privileged role (alongside admin) that can
    /// call [`Self::emergency_shutdown`] without full admin authority. Set to a
    /// multisig or monitoring bot for on-chain circuit-breaker functionality.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    /// * `guardian` - Address to register as guardian.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn set_guardian(env: Env, admin: Address, guardian: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        set_guardian_logic(&env, admin, guardian);
        Ok(())
    }

    /// Return the current guardian address, or `None` if not configured.
    pub fn get_guardian(env: Env) -> Option<Address> {
        get_guardian_logic(&env)
    }

    /// Trigger emergency shutdown (admin or guardian).
    ///
    /// Moves the protocol into [`EmergencyState::Shutdown`], which blocks all
    /// high-risk operations. Only the admin or the registered guardian may call
    /// this. The caller must provide Soroban auth (`require_auth`).
    ///
    /// # Arguments
    /// * `caller` - Must be the admin or the registered guardian.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is neither admin nor guardian.
    pub fn emergency_shutdown(env: Env, caller: Address) -> Result<(), BorrowError> {
        ensure_shutdown_authorized(&env, &caller)?;
        caller.require_auth();
        trigger_shutdown_logic(&env, caller);
        Ok(())
    }

    /// Move from hard shutdown into controlled user recovery.
    ///
    /// Transitions the protocol from [`EmergencyState::Shutdown`] to
    /// [`EmergencyState::Recovery`]. In recovery mode withdrawals and repayments
    /// remain available so users can exit positions safely, while new borrows and
    /// deposits stay blocked.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    /// - [`BorrowError::ProtocolPaused`] — protocol is not in `Shutdown` state.
    pub fn start_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        if get_emergency_state_logic(&env) != EmergencyState::Shutdown {
            return Err(BorrowError::ProtocolPaused);
        }
        start_recovery_logic(&env, admin);
        Ok(())
    }

    /// Return the protocol to normal operation after recovery procedures.
    ///
    /// Clears the emergency state so all operations become available again.
    /// Should only be called once the incident that triggered shutdown has been
    /// fully resolved and any necessary on-chain remediation is complete.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn complete_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        complete_recovery_logic(&env, admin);
        Ok(())
    }

    /// Read the current emergency lifecycle state.
    ///
    /// Returns one of [`EmergencyState::Normal`], [`EmergencyState::Shutdown`],
    /// or [`EmergencyState::Recovery`]. No authorization required.
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

    /// Repay borrowed assets.
    ///
    /// Reduces the caller's outstanding debt by `amount`. Allowed during
    /// recovery mode even though other high-risk operations are blocked, so
    /// users can always exit positions after an emergency shutdown.
    ///
    /// # Arguments
    /// * `user` - Address of the borrower; must authorise this call.
    /// * `asset` - Token address of the debt to repay.
    /// * `amount` - Amount to repay (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`BorrowError::ProtocolPaused`] — repay is paused or protocol is in
    ///   shutdown (outside of recovery).
    pub fn repay(env: Env, user: Address, asset: Address, amount: i128) -> Result<(), BorrowError> {
        user.require_auth();
        if is_paused(&env, PauseType::Repay) || (!is_recovery(&env) && blocks_high_risk_ops(&env)) {
            return Err(BorrowError::ProtocolPaused);
        }
        borrow_repay(&env, user, asset, amount)
    }

    /// Deposit collateral for a borrow position.
    ///
    /// Increases the caller's collateral balance in the borrow module. Blocked
    /// when the `Deposit` pause flag is set or the protocol is in emergency
    /// shutdown.
    ///
    /// # Arguments
    /// * `user` - Address of the depositor; must authorise this call.
    /// * `asset` - Token address to deposit as collateral.
    /// * `amount` - Amount to deposit (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`BorrowError::ProtocolPaused`] — deposit is paused or protocol is shut down.
    pub fn deposit_collateral(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        user.require_auth();
        if is_paused(&env, PauseType::Deposit) || blocks_high_risk_ops(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
        borrow_deposit(&env, user, asset, amount)
    }

    /// Deposit collateral into the protocol (deposit module).
    ///
    /// Increases the caller's collateral balance tracked by the deposit module
    /// and returns the resulting total collateral balance. Blocked when the
    /// `Deposit` pause flag is set or the protocol is in emergency shutdown.
    ///
    /// # Arguments
    /// * `user` - Address of the depositor.
    /// * `asset` - Token address to deposit as collateral.
    /// * `amount` - Amount to deposit (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`DepositError::DepositPaused`] — deposit is paused or protocol is shut down.
    /// - Other [`DepositError`] variants propagated from the deposit module.
    pub fn deposit(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, DepositError> {
        if is_paused(&env, PauseType::Deposit) || blocks_high_risk_ops(&env) {
            return Err(DepositError::DepositPaused);
        }
        deposit_impl(&env, user, asset, amount)
    }

    /// Liquidate an undercollateralised position.
    ///
    /// The liquidator repays up to `amount` of the borrower's `debt_asset` debt
    /// and receives the equivalent value of `collateral_asset` plus the
    /// liquidation incentive bonus. The close factor caps how much of a single
    /// debt position can be liquidated per call.
    ///
    /// # Arguments
    /// * `liquidator` - Address executing the liquidation; must authorise this call.
    /// * `borrower` - Address of the undercollateralised account.
    /// * `debt_asset` - Token address of the debt being repaid.
    /// * `collateral_asset` - Token address of the collateral being seized.
    /// * `amount` - Debt amount to repay (raw token units).
    ///
    /// # Errors
    /// - [`BorrowError::ProtocolPaused`] — liquidation is paused or protocol is shut down.
    /// - Other [`BorrowError`] variants propagated from the borrow module.
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        liquidator.require_auth();
        if is_paused(&env, PauseType::Liquidation) || blocks_high_risk_ops(&env) {
            return Err(BorrowError::ProtocolPaused);
        }

        // Point to the internal liquidation logic in the borrow module
        borrow::liquidate_position(
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
    ///
    /// The insurance fund absorbs bad debt that cannot be covered by seized
    /// collateral. Returns 0 if no fund has been credited for this asset.
    ///
    /// # Arguments
    /// * `asset` - Token address to query.
    pub fn get_insurance_fund_balance(env: Env, asset: Address) -> i128 {
        get_insurance_fund_impl(&env, &asset)
    }

    /// Returns the total bad debt recorded for an asset.
    ///
    /// Bad debt accumulates when a liquidated position's collateral is
    /// insufficient to cover the outstanding loan. Returns 0 if none recorded.
    ///
    /// # Arguments
    /// * `asset` - Token address to query.
    pub fn get_total_bad_debt(env: Env, asset: Address) -> i128 {
        get_bad_debt_impl(&env, &asset)
    }

    /// Credit the insurance fund for an asset (admin only).
    ///
    /// Increases the on-chain insurance fund balance for `asset` by `amount`.
    /// The fund is drawn upon by [`Self::offset_bad_debt`] to absorb protocol losses.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered admin address.
    /// * `asset` - Token address whose insurance fund to credit.
    /// * `amount` - Amount to add (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn credit_insurance_fund(
        env: Env,
        caller: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        ensure_admin(&env, &caller)?;
        credit_insurance_impl(&env, &asset, amount)
    }

    /// Manually offset bad debt using the insurance fund (admin only).
    ///
    /// Deducts `amount` from both the recorded bad-debt balance and the
    /// insurance fund for `asset`. Used after the insurance fund has been
    /// topped up to formally clear accumulated losses from protocol accounting.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered admin address.
    /// * `asset` - Token address whose bad debt to offset.
    /// * `amount` - Amount to offset (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn offset_bad_debt(
        env: Env,
        caller: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        ensure_admin(&env, &caller)?;
        offset_bad_debt_impl(&env, &asset, amount)
    }

    /// Returns gas/performance stats for the current transaction.
    ///
    /// Returns `[cpu_instructions, memory_bytes]`. Budget counters are only
    /// available in testutils builds; production builds always return `[0, 0]`
    /// to maintain a stable ABI.
    #[cfg(not(tarpaulin_include))]
    pub fn get_performance_stats(env: Env) -> Vec<u64> {
        let mut stats = Vec::new(&env);
        // Runtime budget counters are only available in testutils.
        // Keep a stable ABI by returning placeholder values in production builds.
        stats.push_back(0);
        stats.push_back(0);
        stats
    }

    /// Get the user's current debt position.
    ///
    /// Returns the [`DebtPosition`] struct containing the borrowed asset address
    /// and outstanding principal. Returns a zero-value struct if the user has no
    /// open debt.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_user_debt(env: Env, user: Address) -> DebtPosition {
        get_user_debt_impl(&env, &user)
    }

    /// Get the user's collateral position from the borrow module.
    ///
    /// Returns the [`BorrowCollateral`] struct containing the collateral asset
    /// address and deposited amount. Returns a zero-value struct if the user has
    /// no collateral on record.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_user_collateral(env: Env, user: Address) -> BorrowCollateral {
        get_borrow_collateral(&env, &user)
    }

    // ═══════════════════════════════════════════════════════════════════
    // View functions (read-only; for frontends and liquidations)
    // ═══════════════════════════════════════════════════════════════════

    /// Returns the user's collateral balance (raw token units).
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_collateral_balance(env: Env, user: Address) -> i128 {
        view_collateral_balance(&env, &user)
    }

    /// Returns the user's debt balance (principal plus accrued interest).
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_debt_balance(env: Env, user: Address) -> i128 {
        view_debt_balance(&env, &user)
    }

    /// Returns the user's collateral value in the oracle's quote currency.
    ///
    /// Expressed in the oracle's decimals (typically 8 for USD). Returns 0 if
    /// the oracle has not been configured.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_collateral_value(env: Env, user: Address) -> i128 {
        view_collateral_value(&env, &user)
    }

    /// Returns the user's debt value in the oracle's quote currency.
    ///
    /// Returns 0 if the oracle has not been configured.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_debt_value(env: Env, user: Address) -> i128 {
        view_debt_value(&env, &user)
    }

    /// Returns the user's health factor scaled by 10 000 (1.0 = 10 000).
    ///
    /// Values above 10 000 indicate a healthy position; values at or below
    /// 10 000 indicate the position is eligible for liquidation. Returns 0 if
    /// the user has no debt or the oracle is not configured.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_health_factor(env: Env, user: Address) -> i128 {
        view_health_factor(&env, &user)
    }

    /// Returns the full position summary for a user.
    ///
    /// Aggregates collateral balance, collateral value, debt balance, debt value,
    /// and health factor into a single [`UserPositionSummary`] struct. Useful for
    /// frontends and liquidation bots that need all figures in one call.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_user_position(env: Env, user: Address) -> UserPositionSummary {
        view_user_position(&env, &user)
    }

    /// Set the oracle address used for collateral price feeds (admin only).
    ///
    /// Replaces any previously configured oracle. The oracle is consulted by
    /// health-factor and liquidation calculations.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    /// * `oracle` - Address of the new oracle contract.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn set_oracle(env: Env, admin: Address, oracle: Address) -> Result<(), BorrowError> {
        set_oracle_impl(&env, &admin, oracle)
    }

    /// Configure oracle staleness parameters (admin only).
    ///
    /// # Arguments
    /// * `caller` - Must match the registered admin address.
    /// * `config` - [`OracleConfig`] containing the new staleness window.
    ///
    /// # Errors
    /// - [`OracleError::Unauthorized`] — caller is not the protocol admin.
    /// - [`OracleError::InvalidPrice`] — `max_staleness_seconds` is zero.
    pub fn configure_oracle(
        env: Env,
        caller: Address,
        config: OracleConfig,
    ) -> Result<(), OracleError> {
        oracle::configure_oracle(&env, caller, config)
    }

    /// Register the primary oracle address for `asset` (admin only).
    ///
    /// # Arguments
    /// * `caller` - Must match the registered admin address.
    /// * `asset` - Token address to configure.
    /// * `primary_oracle` - Oracle contract address to use as primary feed.
    ///
    /// # Errors
    /// - [`OracleError::Unauthorized`] — caller is not the protocol admin.
    /// - [`OracleError::InvalidOracle`] — oracle address is the contract itself.
    pub fn set_primary_oracle(
        env: Env,
        caller: Address,
        asset: Address,
        primary_oracle: Address,
    ) -> Result<(), OracleError> {
        oracle::set_primary_oracle(&env, caller, asset, primary_oracle)
    }

    /// Register the fallback oracle address for `asset` (admin only).
    ///
    /// The fallback is consulted when the primary oracle returns a stale price.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered admin address.
    /// * `asset` - Token address to configure.
    /// * `fallback_oracle` - Oracle contract address to use as fallback feed.
    ///
    /// # Errors
    /// - [`OracleError::Unauthorized`] — caller is not the protocol admin.
    /// - [`OracleError::InvalidOracle`] — oracle address is the contract itself.
    pub fn set_fallback_oracle(
        env: Env,
        caller: Address,
        asset: Address,
        fallback_oracle: Address,
    ) -> Result<(), OracleError> {
        oracle::set_fallback_oracle(&env, caller, asset, fallback_oracle)
    }

    /// Submit a price update for `asset`.
    ///
    /// Caller must be the admin, the registered primary oracle, or the registered
    /// fallback oracle for this asset.
    ///
    /// # Arguments
    /// * `caller` - Authorized submitter (admin, primary oracle, or fallback oracle).
    /// * `asset` - Token address to update.
    /// * `price` - New price in the oracle's quote currency (must be positive).
    ///
    /// # Errors
    /// - [`OracleError::OraclePaused`] — oracle updates are paused.
    /// - [`OracleError::Unauthorized`] — caller is not authorized.
    /// - [`OracleError::InvalidPrice`] — price is zero or negative.
    pub fn update_price_feed(
        env: Env,
        caller: Address,
        asset: Address,
        price: i128,
    ) -> Result<(), OracleError> {
        oracle::update_price_feed(&env, caller, asset, price)
    }

    /// Get the current price for `asset` (primary → fallback → error).
    ///
    /// # Arguments
    /// * `asset` - Token address to price.
    ///
    /// # Errors
    /// - [`OracleError::StalePrice`] — best available price is stale.
    /// - [`OracleError::NoPriceFeed`] — no price has been submitted for this asset.
    pub fn get_price(env: Env, asset: Address) -> Result<i128, OracleError> {
        oracle::get_price(&env, &asset)
    }

    /// Pause or unpause oracle price updates (admin only).
    ///
    /// When paused, calls to [`Self::update_price_feed`] return
    /// [`OracleError::OraclePaused`].
    ///
    /// # Arguments
    /// * `caller` - Must match the registered admin address.
    /// * `paused` - `true` to pause oracle updates, `false` to resume.
    ///
    /// # Errors
    /// - [`OracleError::Unauthorized`] — caller is not the admin.
    pub fn set_oracle_paused(
        env: Env,
        caller: Address,
        paused: bool,
    ) -> Result<(), OracleError> {
        oracle::set_oracle_paused(&env, caller, paused)
    }

    /// Set the liquidation threshold in basis points (admin only).
    ///
    /// The liquidation threshold is the maximum loan-to-value ratio before a
    /// position becomes eligible for liquidation, expressed in basis points
    /// (e.g. 8000 = 80%). Must be between 1 and 10 000.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    /// * `bps` - New threshold in basis points (1–10 000).
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn set_liquidation_threshold_bps(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_liq_threshold_impl(&env, &admin, bps)
    }

    /// Returns the close factor in basis points.
    ///
    /// The close factor is the maximum fraction of a single debt position that
    /// can be liquidated in one call (default 5000 = 50%).
    pub fn get_close_factor_bps(env: Env) -> i128 {
        get_close_factor_impl(&env)
    }

    /// Set the close factor in basis points (admin only).
    ///
    /// Caps the fraction of a single debt position that can be liquidated in
    /// one call (1–10 000 bps). A value of 5000 means at most 50% of the debt
    /// can be repaid per liquidation call.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    /// * `bps` - New close factor in basis points (1–10 000).
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn set_close_factor_bps(env: Env, admin: Address, bps: i128) -> Result<(), BorrowError> {
        set_close_factor_impl(&env, &admin, bps)
    }

    /// Returns the liquidation incentive in basis points (default 1000 = 10%).
    ///
    /// The incentive is the collateral bonus a liquidator receives above the
    /// repaid debt value.
    pub fn get_liquidation_incentive_bps(env: Env) -> i128 {
        get_liquidation_incentive_bps_impl(&env)
    }

    /// Set the liquidation incentive in basis points (admin only).
    ///
    /// The incentive is the bonus collateral a liquidator receives above the
    /// repaid debt value (0–10 000 bps). A value of 1000 means a 10% bonus.
    ///
    /// # Arguments
    /// * `admin` - Must match the registered admin address.
    /// * `bps` - New incentive in basis points (0–10 000).
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — caller is not the admin.
    pub fn set_liquidation_incentive_bps(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_liquidation_incentive_bps_impl(&env, &admin, bps)
    }

    /// Returns the maximum debt that can be liquidated for `user` in one call.
    ///
    /// Applies the close factor to the user's total debt. Returns 0 if the
    /// position is healthy, the user has no debt, or the oracle is not
    /// configured.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    pub fn get_max_liquidatable_amount(env: Env, user: Address) -> i128 {
        view_max_liquidatable_amount(&env, &user)
    }

    /// Returns the collateral bonus a liquidator receives for repaying `repay_amount`.
    ///
    /// Formula: `repay_amount * (10_000 + incentive_bps) / 10_000`.
    ///
    /// # Arguments
    /// * `repay_amount` - Debt amount being repaid (raw token units).
    pub fn get_liquidation_incentive_amount(env: Env, repay_amount: i128) -> i128 {
        view_liquidation_incentive_amount(&env, repay_amount)
    }

    /// Initialize borrow settings (admin only).
    ///
    /// Sets the global debt ceiling and minimum borrow amount. Must be called
    /// after [`Self::initialize`] to activate borrow functionality. Panics if
    /// no admin has been registered yet.
    ///
    /// # Arguments
    /// * `debt_ceiling` - Maximum aggregate debt the protocol will allow (raw token units).
    /// * `min_borrow_amount` - Minimum per-position borrow size.
    ///
    /// # Errors
    /// - [`BorrowError::Unauthorized`] — no admin registered or auth fails.
    #[cfg(not(tarpaulin_include))]
    pub fn initialize_borrow_settings(
        env: Env,
        debt_ceiling: i128,
        min_borrow_amount: i128,
    ) -> Result<(), BorrowError> {
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        current_admin.require_auth();
        init_borrow_settings_impl(&env, debt_ceiling, min_borrow_amount)
    }

    /// Initialize deposit settings (admin only).
    ///
    /// Sets the maximum aggregate deposit cap and the minimum per-deposit
    /// amount. Must be called after [`Self::initialize`] to activate deposit
    /// functionality.
    ///
    /// # Arguments
    /// * `deposit_cap` - Maximum total deposits the protocol will accept (raw token units).
    /// * `min_deposit_amount` - Minimum per-deposit amount; protects against dust.
    ///
    /// # Errors
    /// - [`DepositError::Unauthorized`] — no admin registered or auth fails.
    pub fn initialize_deposit_settings(
        env: Env,
        deposit_cap: i128,
        min_deposit_amount: i128,
    ) -> Result<(), DepositError> {
        let current_admin = get_protocol_admin(&env).ok_or(DepositError::Unauthorized)?;
        current_admin.require_auth();
        init_deposit_settings_impl(&env, deposit_cap, min_deposit_amount)
    }

    /// Set deposit pause state (admin only).
    ///
    /// Convenience wrapper around [`Self::set_pause`] scoped to
    /// `PauseType::Deposit`. Emits a `pause_event` so off-chain monitors can
    /// react.
    ///
    /// # Arguments
    /// * `paused` - `true` to pause deposits, `false` to resume.
    ///
    /// # Errors
    /// - [`DepositError::Unauthorized`] — caller is not the admin.
    #[cfg(not(tarpaulin_include))]
    pub fn set_deposit_paused(env: Env, paused: bool) -> Result<(), DepositError> {
        let admin = get_protocol_admin(&env).ok_or(DepositError::Unauthorized)?;
        admin.require_auth();
        set_pause_impl(&env, admin, PauseType::Deposit, paused);
        Ok(())
    }

    /// Get the user's deposit collateral position from the deposit module.
    ///
    /// Returns the [`DepositCollateral`] struct for the given `user` and `asset`
    /// pair. Returns a zero-value struct if no position exists.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    /// * `asset` - Token address to query.
    pub fn get_user_collateral_deposit(
        env: Env,
        user: Address,
        asset: Address,
    ) -> DepositCollateral {
        get_deposit_collateral_impl(&env, &user, &asset)
    }

    /// Return the current protocol admin address, or `None` if not initialized.
    #[cfg(not(tarpaulin_include))]
    pub fn get_admin(env: Env) -> Option<Address> {
        get_protocol_admin(&env)
    }

    /// Execute a flash loan.
    ///
    /// Lends `amount` of `asset` to `receiver`, invoking the receiver's callback
    /// with `params`. The receiver must repay the loan plus the configured fee
    /// within the same transaction. Blocked when `PauseType::All` is set or the
    /// protocol is in emergency shutdown.
    ///
    /// # Arguments
    /// * `receiver` - Contract address that implements the flash-loan callback.
    /// * `asset` - Token address to borrow.
    /// * `amount` - Loan amount (raw token units, must be positive).
    /// * `params` - Arbitrary bytes forwarded to the receiver's callback.
    ///
    /// # Errors
    /// - [`FlashLoanError::ProtocolPaused`] — protocol is paused or shut down.
    /// - Other [`FlashLoanError`] variants from the flash-loan module.
    #[cfg(not(tarpaulin_include))]
    pub fn flash_loan(
        env: Env,
        receiver: Address,
        asset: Address,
        amount: i128,
        params: Bytes,
    ) -> Result<(), FlashLoanError> {
        if is_paused(&env, PauseType::All) || blocks_high_risk_ops(&env) {
            return Err(FlashLoanError::ProtocolPaused);
        }
        flash_loan_impl(&env, receiver, asset, amount, params)
    }

    /// Set the flash loan fee in basis points (admin only).
    ///
    /// The fee is charged on top of the borrowed amount and must be repaid
    /// within the same transaction. A value of 30 means 0.30%.
    ///
    /// # Arguments
    /// * `fee_bps` - New fee in basis points.
    ///
    /// # Errors
    /// - [`FlashLoanError::Unauthorized`] — no admin registered or auth fails.
    pub fn set_flash_loan_fee_bps(env: Env, fee_bps: i128) -> Result<(), FlashLoanError> {
        let current_admin = get_protocol_admin(&env).ok_or(FlashLoanError::Unauthorized)?;
        current_admin.require_auth();
        set_flash_loan_fee_impl(&env, fee_bps)
    }

    /// Withdraw collateral from the protocol.
    ///
    /// Pause, emergency shutdown vs. recovery, legacy withdraw flag, and
    /// collateral-ratio checks are enforced inside [`withdraw::withdraw`] so
    /// behaviour stays aligned with the pause module. Allowed during recovery
    /// mode so users can exit positions after an emergency shutdown.
    ///
    /// # Arguments
    /// * `user` - Address of the withdrawer.
    /// * `asset` - Token address to withdraw.
    /// * `amount` - Amount to withdraw (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`WithdrawError::WithdrawPaused`] — withdraw is paused or protocol is
    ///   in hard shutdown.
    /// - Other [`WithdrawError`] variants from the withdraw module.
    pub fn withdraw(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, WithdrawError> {
        withdraw_logic(&env, user, asset, amount)
    }

    /// Initialize withdraw settings (admin only).
    ///
    /// Sets the minimum per-withdrawal amount to prevent dust withdrawals.
    /// Must be called after [`Self::initialize`] to activate withdraw
    /// functionality.
    ///
    /// # Arguments
    /// * `min_withdraw_amount` - Minimum per-withdrawal amount (raw token units).
    ///
    /// # Errors
    /// - [`WithdrawError::Unauthorized`] — no admin registered or auth fails.
    pub fn initialize_withdraw_settings(
        env: Env,
        min_withdraw_amount: i128,
    ) -> Result<(), WithdrawError> {
        let current_admin = get_protocol_admin(&env).ok_or(WithdrawError::Unauthorized)?;
        current_admin.require_auth();
        initialize_withdraw_logic(&env, min_withdraw_amount)
    }

    /// Set withdraw pause state (admin only).
    ///
    /// Convenience wrapper around [`Self::set_pause`] scoped to
    /// `PauseType::Withdraw`. Emits a `pause_event` so off-chain monitors can
    /// react.
    ///
    /// # Arguments
    /// * `paused` - `true` to pause withdrawals, `false` to resume.
    ///
    /// # Errors
    /// - [`WithdrawError::Unauthorized`] — caller is not the admin.
    pub fn set_withdraw_paused(env: Env, paused: bool) -> Result<(), WithdrawError> {
        let admin = get_protocol_admin(&env).ok_or(WithdrawError::Unauthorized)?;
        admin.require_auth();
        set_pause_impl(&env, admin, PauseType::Withdraw, paused);
        Ok(())
    }

    /// Token receiver hook called by SEP-41 compliant tokens.
    ///
    /// Invoked when a token contract pushes funds directly to this contract
    /// via `transfer`. The `payload` encodes the intended protocol operation
    /// (e.g. deposit or repay). Callers must not invoke this directly — it is
    /// part of the token callback interface.
    ///
    /// # Arguments
    /// * `token_asset` - Address of the token being received.
    /// * `from` - Address that initiated the token transfer.
    /// * `amount` - Amount of tokens received (raw token units).
    /// * `payload` - Encoded operation parameters forwarded by the token contract.
    ///
    /// # Errors
    /// - [`BorrowError`] variants propagated from the underlying operation.
    pub fn receive(
        env: Env,
        token_asset: Address,
        from: Address,
        amount: i128,
        payload: Vec<Val>,
    ) -> Result<(), BorrowError> {
        receive_impl(env, token_asset, from, amount, payload)
    }

    // ───────────────────────────────────────────────────
    // Upgrade Management (Governance)
    // ───────────────────────────────────────────────────

    /// Initialize the upgrade manager (admin only).
    ///
    /// Registers the `admin`, records the `current_wasm_hash` as the deployed
    /// baseline, and sets `required_approvals` — the number of distinct approver
    /// signatures needed before an upgrade proposal can be executed.
    ///
    /// # Arguments
    /// * `admin` - Address that becomes the upgrade admin.
    /// * `current_wasm_hash` - SHA-256 hash of the currently deployed WASM.
    /// * `required_approvals` - Minimum approvals required to execute a proposal.
    pub fn upgrade_init(
        env: Env,
        admin: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) {
        upgrade::UpgradeManager::init(env, admin, current_wasm_hash, required_approvals);
    }

    /// Add an address to the upgrade approver set (admin only).
    ///
    /// Approvers are the addresses that may call [`Self::upgrade_approve`]. The
    /// admin can expand the set at any time; removing an approver requires
    /// [`Self::upgrade_remove_approver`].
    ///
    /// # Arguments
    /// * `caller` - Must match the registered upgrade admin.
    /// * `approver` - Address to add to the approver set.
    pub fn upgrade_add_approver(env: Env, caller: Address, approver: Address) {
        upgrade::UpgradeManager::add_approver(env, caller, approver);
    }

    /// Remove an address from the upgrade approver set (admin only).
    ///
    /// # Arguments
    /// * `caller` - Must match the registered upgrade admin.
    /// * `approver` - Address to remove from the approver set.
    pub fn upgrade_remove_approver(env: Env, caller: Address, approver: Address) {
        upgrade::UpgradeManager::remove_approver(env, caller, approver);
    }

    /// Propose a new WASM upgrade (admin only).
    ///
    /// Creates a pending upgrade proposal identified by the returned `proposal_id`.
    /// The proposal must collect `required_approvals` signatures via
    /// [`Self::upgrade_approve`] before it can be executed.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered upgrade admin.
    /// * `new_wasm_hash` - SHA-256 hash of the candidate WASM to deploy.
    /// * `new_version` - Semantic version number for the new WASM.
    ///
    /// # Returns
    /// A unique `proposal_id` used to track this upgrade through approval and
    /// execution.
    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> u64 {
        upgrade::UpgradeManager::upgrade_propose(env, caller, new_wasm_hash, new_version)
    }

    /// Approve a pending upgrade proposal (approver only).
    ///
    /// Each approver may call this once per proposal. Returns the running total
    /// of approvals collected so far. Once the count reaches `required_approvals`
    /// the proposal becomes executable.
    ///
    /// # Arguments
    /// * `caller` - Must be a registered approver.
    /// * `proposal_id` - ID returned by [`Self::upgrade_propose`].
    ///
    /// # Returns
    /// Number of approvals the proposal has received after this call.
    pub fn upgrade_approve(env: Env, caller: Address, proposal_id: u64) -> u32 {
        upgrade::UpgradeManager::upgrade_approve(env, caller, proposal_id)
    }

    /// Execute an approved upgrade proposal (admin only).
    ///
    /// Atomically replaces the contract WASM with the hash recorded in the
    /// proposal. The proposal must have reached `required_approvals` before this
    /// call will succeed.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered upgrade admin.
    /// * `proposal_id` - ID of an approved proposal.
    pub fn upgrade_execute(env: Env, caller: Address, proposal_id: u64) {
        upgrade::UpgradeManager::upgrade_execute(env, caller, proposal_id);
    }

    /// Roll back a pending or approved upgrade proposal (admin only).
    ///
    /// Cancels the proposal and discards any collected approvals. Use this if
    /// the proposed WASM hash is found to be incorrect or the upgrade is no
    /// longer desired.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered upgrade admin.
    /// * `proposal_id` - ID of the proposal to cancel.
    pub fn upgrade_rollback(env: Env, caller: Address, proposal_id: u64) {
        upgrade::UpgradeManager::upgrade_rollback(env, caller, proposal_id);
    }

    /// Return the current status of an upgrade proposal.
    ///
    /// Returns an [`UpgradeStatus`] value describing the proposal stage
    /// (`Pending`, `Approved`, `Executed`, or `RolledBack`) and approval count.
    ///
    /// # Arguments
    /// * `proposal_id` - ID returned by [`Self::upgrade_propose`].
    pub fn upgrade_status(env: Env, proposal_id: u64) -> upgrade::UpgradeStatus {
        upgrade::UpgradeManager::upgrade_status(env, proposal_id)
    }

    /// Return the SHA-256 hash of the currently deployed contract WASM.
    pub fn current_wasm_hash(env: Env) -> BytesN<32> {
        upgrade::UpgradeManager::current_wasm_hash(env)
    }

    /// Return the current deployed contract version number.
    pub fn current_version(env: Env) -> u32 {
        upgrade::UpgradeManager::current_version(env)
    }

    // ───────────────────────────────────────────────────
    // Data Store Management
    // ───────────────────────────────────────────────────

    /// Initialize the data store with the given admin address.
    ///
    /// Must be called once before any data operations. Subsequent calls are
    /// silently ignored if the store is already initialized.
    ///
    /// # Arguments
    /// * `admin` - Address that becomes the data-store admin with full write
    ///   and restore privileges.
    #[cfg(not(tarpaulin_include))]
    pub fn data_store_init(env: Env, admin: Address) {
        if env.storage().persistent().has(&data_store::StoreKey::Admin) {
            return;
        }
        data_store::DataStore::init(env, admin);
    }

    /// Grant write access to an additional address (admin only).
    ///
    /// Writers may call [`Self::data_save`] and [`Self::data_backup`] but cannot
    /// call [`Self::data_restore`] or [`Self::data_migrate_bump_version`], which
    /// are reserved for the admin.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered data-store admin.
    /// * `writer` - Address to add to the writer set.
    pub fn data_grant_writer(env: Env, caller: Address, writer: Address) {
        data_store::DataStore::grant_writer(env, caller, writer);
    }

    /// Revoke write access from an address (admin only).
    ///
    /// # Arguments
    /// * `caller` - Must match the registered data-store admin.
    /// * `writer` - Address to remove from the writer set.
    #[cfg(not(tarpaulin_include))]
    pub fn data_revoke_writer(env: Env, caller: Address, writer: Address) {
        data_store::DataStore::revoke_writer(env, caller, writer);
    }

    /// Write or overwrite a key-value entry (admin or writer only).
    ///
    /// Keys are bounded to `MAX_KEY_LEN` (64 bytes) and values to
    /// `MAX_VALUE_LEN` (4096 bytes). Panics with a data-store error if either
    /// limit is exceeded or if the store is at capacity (`MAX_ENTRIES`).
    ///
    /// # Arguments
    /// * `caller` - Must be the admin or a granted writer.
    /// * `key` - UTF-8 string key (max 64 bytes).
    /// * `value` - Raw bytes to store (max 4096 bytes).
    #[cfg(not(tarpaulin_include))]
    pub fn data_save(env: Env, caller: Address, key: soroban_sdk::String, value: Bytes) {
        data_store::DataStore::data_save(env, caller, key, value);
    }

    /// Read a value by key (public, no authorization required).
    ///
    /// Returns the stored bytes for `key`, or panics with `KeyNotFound` if the
    /// key does not exist.
    ///
    /// # Arguments
    /// * `key` - UTF-8 string key to look up.
    pub fn data_load(env: Env, key: soroban_sdk::String) -> Bytes {
        data_store::DataStore::data_load(env, key)
    }

    /// Snapshot all current key-value entries under a named backup (admin or writer).
    ///
    /// Backup names are bounded to `MAX_BACKUP_NAME` (32 bytes). An existing
    /// backup with the same name is overwritten.
    ///
    /// # Arguments
    /// * `caller` - Must be the admin or a granted writer.
    /// * `backup_name` - Short identifier for this snapshot (max 32 bytes).
    pub fn data_backup(env: Env, caller: Address, backup_name: soroban_sdk::String) {
        data_store::DataStore::data_backup(env, caller, backup_name);
    }

    /// Restore all key-value entries from a named backup (admin only).
    ///
    /// Replaces the live store contents with the snapshot. Panics with
    /// `BackupNotFound` if the named backup does not exist.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered data-store admin.
    /// * `backup_name` - Name of the snapshot to restore.
    pub fn data_restore(env: Env, caller: Address, backup_name: soroban_sdk::String) {
        data_store::DataStore::data_restore(env, caller, backup_name);
    }

    /// Atomically bump the schema version and record a migration memo (admin only).
    ///
    /// Increments the on-chain schema version to `new_version` and persists
    /// `memo` as the migration description. Used to coordinate off-chain
    /// migration tooling with on-chain state.
    ///
    /// # Arguments
    /// * `caller` - Must match the registered data-store admin.
    /// * `new_version` - Target schema version (should be monotonically increasing).
    /// * `memo` - Short description of the migration for audit purposes.
    pub fn data_migrate_bump_version(
        env: Env,
        caller: Address,
        new_version: u32,
        memo: soroban_sdk::String,
    ) {
        data_store::DataStore::data_migrate_bump_version(env, caller, new_version, Some(memo));
    }

    /// Return the current data-store schema version number.
    pub fn data_schema_version(env: Env) -> u32 {
        data_store::DataStore::schema_version(env)
    }

    /// Return the number of key-value entries currently in the store.
    #[cfg(not(tarpaulin_include))]
    pub fn data_entry_count(env: Env) -> u32 {
        data_store::DataStore::entry_count(env)
    }

    /// Return whether `key` exists in the store.
    ///
    /// # Arguments
    /// * `key` - UTF-8 string key to check.
    #[cfg(not(tarpaulin_include))]
    pub fn data_key_exists(env: Env, key: soroban_sdk::String) -> bool {
        data_store::DataStore::key_exists(env, key)
    }

    // ═══════════════════════════════════════════════════════════════════
    // Cross-Asset Operations
    // ═══════════════════════════════════════════════════════════════════

    /// Initialize the admin for cross-asset operations.
    ///
    /// Must be called before any cross-asset function. The admin registered here
    /// is the sole address authorized to call [`Self::set_asset_params`]. Panics
    /// if the admin has already been set.
    ///
    /// # Arguments
    /// * `admin` - Address that becomes the cross-asset admin.
    pub fn initialize_admin(env: Env, admin: Address) {
        cross_init_admin(&env, admin);
    }

    /// Set risk parameters for a specific asset (admin only).
    ///
    /// Configures the loan-to-value ratio, liquidation threshold, price oracle,
    /// debt ceiling, and active status for `asset`. Must be called before users
    /// can deposit or borrow that asset via the cross-asset operations.
    ///
    /// # Arguments
    /// * `asset` - Token address to configure.
    /// * `params` - [`AssetParams`] struct containing the risk configuration.
    ///
    /// # Errors
    /// - [`CrossAssetError::Unauthorized`] — caller is not the cross-asset admin.
    pub fn set_asset_params(
        env: Env,
        asset: Address,
        params: AssetParams,
    ) -> Result<(), CrossAssetError> {
        cross_set_asset_params(&env, asset, params)
    }

    /// Deposit collateral for a specific asset in the cross-asset module.
    ///
    /// Increases the user's collateral balance for `asset`. The asset must have
    /// been configured with [`Self::set_asset_params`] and marked active.
    ///
    /// # Arguments
    /// * `user` - Address of the depositor.
    /// * `asset` - Token address to deposit.
    /// * `amount` - Amount to deposit (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`CrossAssetError::AssetNotSupported`] — asset not configured or inactive.
    /// - [`CrossAssetError::InvalidAmount`] — amount is zero or negative.
    /// - [`CrossAssetError::Overflow`] — balance would overflow `i128`.
    pub fn deposit_collateral_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        cross_deposit_collateral(&env, user, asset, amount)
    }

    /// Borrow a specific asset against cross-asset collateral.
    ///
    /// Increases the user's debt balance for `asset`. The total debt across all
    /// users must not exceed the per-asset debt ceiling configured in
    /// [`Self::set_asset_params`].
    ///
    /// # Arguments
    /// * `user` - Address of the borrower.
    /// * `asset` - Token address to borrow.
    /// * `amount` - Amount to borrow (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`CrossAssetError::AssetNotSupported`] — asset not configured or inactive.
    /// - [`CrossAssetError::InvalidAmount`] — amount is zero or negative.
    /// - [`CrossAssetError::DebtCeilingReached`] — borrow would exceed the debt ceiling.
    /// - [`CrossAssetError::Overflow`] — balance would overflow `i128`.
    pub fn borrow_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        cross_borrow_asset(&env, user, asset, amount)
    }

    /// Repay debt for a specific asset in the cross-asset module.
    ///
    /// Reduces the user's outstanding debt balance for `asset`. Repayment is
    /// capped at the current debt balance; excess amounts are not refunded.
    ///
    /// # Arguments
    /// * `user` - Address of the borrower repaying the debt.
    /// * `asset` - Token address of the debt to repay.
    /// * `amount` - Amount to repay (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`CrossAssetError::AssetNotSupported`] — asset not configured or inactive.
    /// - [`CrossAssetError::InvalidAmount`] — amount is zero or negative.
    pub fn repay_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        cross_repay_asset(&env, user, asset, amount)
    }

    /// Withdraw collateral for a specific asset from the cross-asset module.
    ///
    /// Reduces the user's collateral balance for `asset`. The withdrawal is
    /// capped at the deposited balance; excess amounts are not transferred.
    ///
    /// # Arguments
    /// * `user` - Address of the withdrawer.
    /// * `asset` - Token address to withdraw.
    /// * `amount` - Amount to withdraw (raw token units, must be positive).
    ///
    /// # Errors
    /// - [`CrossAssetError::AssetNotSupported`] — asset not configured or inactive.
    /// - [`CrossAssetError::InvalidAmount`] — amount is zero or negative.
    pub fn withdraw_asset(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), CrossAssetError> {
        cross_withdraw_asset(&env, user, asset, amount)
    }

    /// Get the cross-asset position summary for a user.
    ///
    /// Returns a [`PositionSummary`] containing the aggregate collateral value,
    /// aggregate debt value, and health factor across all assets tracked by the
    /// cross-asset module.
    ///
    /// # Arguments
    /// * `user` - Address to query.
    ///
    /// # Errors
    /// - [`CrossAssetError`] variants if position data cannot be computed.
    pub fn get_cross_position_summary(
        env: Env,
        user: Address,
    ) -> Result<PositionSummary, CrossAssetError> {
        cross_position_summary(&env, user)
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
