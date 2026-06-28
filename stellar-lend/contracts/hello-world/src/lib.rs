#![allow(deprecated)]
#![allow(unused_imports)]
#![allow(dead_code)]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HelloError {
    InvalidAmount = 1,
}

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Env, Map, Symbol, Vec,
};
use soroban_sdk::{contract, contractimpl, Address, Env, Map, Symbol, Vec};

pub mod admin;
pub mod amm;
pub mod amm_twap;
pub mod analytics;
pub mod borrow;
pub mod bridge;
pub mod config;
pub mod config_snapshot;
pub mod cross_asset;
pub mod deposit;
pub mod errors;
pub mod events;
pub mod flash_loan;
pub mod governance;
pub mod interest_rate;
pub mod liquidate;
pub mod multisig;
pub mod oracle;
pub mod recovery;
pub mod reentrancy;
pub mod repay;
pub mod reserve;
pub mod risk_management;
pub mod risk_params;
pub mod storage;
pub mod types;
pub mod withdraw;

// Legacy test suite references oracle symbols (ExternalOracle,
// get_price_with_fallback, set_oracle_config) that predate the current
// oracle.rs API and no longer exist anywhere in this crate. It currently
// fails to compile under `cargo test`. Excluded from compilation, mirroring
// the precedent already set below for `mod tests;`. Rewriting it to match
// the current oracle API is a separate task from issue #1128.
// #[cfg(test)]
// mod twap_tests;

#[cfg(test)]
mod twap_eviction_test;
#[cfg(test)]
mod twap_fallback_event_test;
#[cfg(test)]
mod twap_tests;

#[cfg(test)]
mod bridge_fee_test;

#[cfg(test)]
mod amm_integration_test;

#[cfg(test)]
mod cross_asset_decimals_test;
#[cfg(test)]
mod rate_clamp_test;
#[cfg(test)]
mod twap_view_test;
#[cfg(test)]
mod twap_maxbuffer_perf_test;

// Legacy test suite currently mismatches contract API and is excluded from CI compile.
// #[cfg(test)]
// mod tests;

use crate::oracle::FullOracleConfig;
fn require_admin(env: &Env, caller: &Address) -> Result<(), RiskManagementError> {
    caller.require_auth();
    let admin_key = DepositDataKey::Admin;
    let admin = env
        .storage()
        .persistent()
        .get::<DepositDataKey, Address>(&admin_key)
        .ok_or(RiskManagementError::Unauthorized)?;

    if caller != &admin {
        return Err(RiskManagementError::Unauthorized);
    }
    Ok(())
}

use borrow::borrow_asset;
use deposit::deposit_collateral;
use repay::repay_debt;

use risk_management::{
    check_emergency_pause, initialize_risk_management, is_emergency_paused, is_operation_paused,
};

use crate::config_snapshot::{get_config_snapshot, ConfigSnapshot};
use crate::deposit::{DepositDataKey, ProtocolAnalytics};
use risk_params::{
    can_be_liquidated, get_liquidation_incentive_amount, get_max_liquidatable_amount,
    initialize_risk_params, require_min_collateral_ratio, RiskParamsError,
};
use withdraw::withdraw_collateral;

use crate::analytics::{
    generate_protocol_report, generate_user_report, get_recent_activity, get_user_activity_feed,
    AnalyticsError, ProtocolReport, UserReport,
};
use crate::bridge::{BridgeConfig, BridgeError};
use crate::config::{config_backup, config_get, config_restore, config_set, ConfigError};
use crate::config_snapshot::{get_config_snapshot, ConfigSnapshot};
use crate::cross_asset::{
    get_asset_config_by_address, get_asset_list, get_total_borrow_for, get_total_supply_for,
    get_user_asset_position, get_user_position_summary, initialize_asset, update_asset_config,
    update_asset_price, AssetConfig, AssetKey, AssetPosition, CrossAssetError, UserPositionSummary,
};
use crate::deposit::{DepositDataKey, ProtocolAnalytics};
use crate::flash_loan::{
    configure_flash_loan, execute_flash_loan, repay_flash_loan, set_flash_loan_fee, FlashLoanConfig,
};

#[allow(unused_imports)]
use bridge::{
    bridge_deposit, bridge_withdraw, get_bridge_config, list_bridges, register_bridge,
    set_bridge_fee, BridgeConfig, BridgeError,
};

#[allow(unused_imports)]
use crate::interest_rate::{
    initialize_interest_rate_config, update_interest_rate_config, InterestRateConfig,
    InterestRateError,
};
use crate::liquidate::liquidate;
use crate::oracle::FullOracleConfig;
use crate::risk_management::{
    check_emergency_pause, initialize_risk_management, is_emergency_paused, is_operation_paused,
    require_admin, set_pause_switch, set_pause_switches, RiskConfig, RiskManagementError,
};
use crate::risk_params::{
    can_be_liquidated, get_liquidation_incentive_amount, get_max_liquidatable_amount,
    initialize_risk_params, require_min_collateral_ratio, RiskParamsError,
};
use crate::storage::GuardianConfig;
use crate::types::{
    GovernanceConfig, MultisigConfig, Proposal, ProposalOutcome, ProposalType, RecoveryRequest,
    VoteInfo, VoteType,
};

// AMM types (temporary stubs until stellarlend_amm types are made public)
#[derive(Clone)]
#[contracttype]
pub struct AmmProtocolConfig {
    // Placeholder fields
}

#[derive(Clone)]
#[contracttype]
pub struct SwapParams {
    // Placeholder fields
}

#[derive(Clone, Debug)]
#[contracterror]
pub enum AmmError {
    InvalidParams = 1,
    InsufficientLiquidity = 2,
    SlippageExceeded = 3,
}

pub mod reentrancy;

/// The StellarLend core contract.
#[contract]
pub struct HelloContract;

#[contractimpl]
impl HelloContract {
    /// Deposit assets into the protocol
    /// Health-check endpoint.
    ///
    /// Returns the string `"Hello"` to verify the contract is deployed and callable.
    /// Health-check endpoint. Returns "Hello".
    pub fn hello(env: Env) -> soroban_sdk::String {
        soroban_sdk::String::from_str(&env, "Hello")
    }

    /// Initialize the contract with admin address.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RiskManagementError> {
        // Check if already initialized (comprehensive check)
        if crate::admin::has_admin(&env)
            || crate::risk_management::get_risk_config(&env).is_some()
            || crate::interest_rate::get_interest_rate_config(&env).is_some()
        {
            return Err(RiskManagementError::AlreadyInitialized);
        }

        crate::admin::set_admin(&env, admin.clone(), None)
            .map_err(|_| RiskManagementError::Unauthorized)?;
        initialize_risk_management(&env, admin.clone())?;
        initialize_risk_params(&env).map_err(|_| RiskManagementError::InvalidParameter)?;
        initialize_interest_rate_config(&env, admin.clone()).map_err(|e| {
            if e == InterestRateError::AlreadyInitialized {
                RiskManagementError::AlreadyInitialized
            } else {
                RiskManagementError::Unauthorized
            }
        })?;
        Ok(())
    }

    /// Transfer super admin rights.
    pub fn transfer_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), crate::admin::AdminError> {
        crate::admin::set_admin(&env, new_admin, Some(caller))
    }

    /// Increment the user's deposit balance.
    pub fn deposit(env: Env, user: Address, amount: i128) -> i128 {
        if amount <= 0 {
            panic_with_error!(env, HelloError::InvalidAmount);
        }
        user.require_auth();
        let key = DataKey::Balance(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_bal = current + amount;
        env.storage().persistent().set(&key, &new_bal);
        new_bal
    }

    /// Decrement the user's deposit balance.
    pub fn withdraw(env: Env, user: Address, amount: i128) -> i128 {
        if amount <= 0 {
            panic_with_error!(env, HelloError::InvalidAmount);
        }
        user.require_auth();
        let key = DataKey::Balance(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_bal = current - amount;
        env.storage().persistent().set(&key, &new_bal);
        new_bal
    }

    /// Borrow increases the user's debt.
    pub fn borrow(env: Env, user: Address, amount: i128) -> i128 {
        if amount <= 0 {
            panic_with_error!(env, HelloError::InvalidAmount);
        }
        user.require_auth();
        let key = DataKey::Debt(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_debt = current + amount;
        env.storage().persistent().set(&key, &new_debt);
        new_debt
    }

    /// Repay decreases the user's debt.
    pub fn repay(env: Env, user: Address, amount: i128) -> i128 {
        if amount <= 0 {
            panic_with_error!(env, HelloError::InvalidAmount);
        }
        user.require_auth();
        let key = DataKey::Debt(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_debt = current - amount;
        env.storage().persistent().set(&key, &new_debt);
        new_debt
    }

    /// Set native asset address (admin only).
    pub fn set_native_asset_address(
        env: Env,
        caller: Address,
        native_asset: Address,
    ) -> Result<(), crate::deposit::DepositError> {
        crate::deposit::set_native_asset_address(&env, caller, native_asset)
    }

    /// Withdraw collateral from the protocol.
    pub fn withdraw_collateral(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<i128, crate::withdraw::WithdrawError> {
        crate::withdraw::withdraw_collateral(&env, user, asset, amount)
    }

    /// Set risk parameters (admin only).
    pub fn set_risk_params(
        env: Env,
        caller: Address,
        min_collateral_ratio: Option<i128>,
        liquidation_threshold: Option<i128>,
        close_factor: Option<i128>,
        liquidation_incentive: Option<i128>,
    ) -> Result<(), RiskManagementError> {
        require_admin(&env, &caller)?;
        check_emergency_pause(&env)?;
        risk_params::set_risk_params(
            &env,
            min_collateral_ratio,
            liquidation_threshold,
            close_factor,
            liquidation_incentive,
        )
        .map_err(|e| match e {
            RiskParamsError::ParameterChangeTooLarge => {
                RiskManagementError::ParameterChangeTooLarge
            }
            RiskParamsError::InvalidCollateralRatio => RiskManagementError::InvalidCollateralRatio,
            RiskParamsError::InvalidLiquidationThreshold => {
                RiskManagementError::InvalidLiquidationThreshold
            }
            RiskParamsError::InvalidCloseFactor => RiskManagementError::InvalidCloseFactor,
            RiskParamsError::InvalidLiquidationIncentive => {
                RiskManagementError::InvalidLiquidationIncentive
            }
            _ => RiskManagementError::InvalidParameter,
        })
    }

    pub fn set_guardians(
        env: Env,
        caller: Address,
        guardians: soroban_sdk::Vec<Address>,
        threshold: u32,
    ) -> Result<(), errors::GovernanceError> {
        recovery::set_guardians(&env, caller, guardians, threshold)
    }

    pub fn start_recovery(
        env: Env,
        initiator: Address,
        old_admin: Address,
        new_admin: Address,
    ) -> Result<(), errors::GovernanceError> {
        recovery::start_recovery(&env, initiator, old_admin, new_admin)
    }

    pub fn approve_recovery(env: Env, approver: Address) -> Result<(), errors::GovernanceError> {
        recovery::approve_recovery(&env, approver)
    }

    pub fn execute_recovery(env: Env, executor: Address) -> Result<(), errors::GovernanceError> {
        recovery::execute_recovery(&env, executor)
    }

    pub fn ms_set_admins(
        env: Env,
        caller: Address,
        admins: soroban_sdk::Vec<Address>,
        threshold: u32,
    ) -> Result<(), errors::GovernanceError> {
        multisig::ms_set_admins(&env, caller, admins, threshold)
    }

    pub fn ms_propose_set_min_cr(
        env: Env,
        proposer: Address,
        new_ratio: i128,
    ) -> Result<u64, errors::GovernanceError> {
        multisig::ms_propose_set_min_cr(&env, proposer, new_ratio)
    }

    pub fn ms_approve(
        env: Env,
        approver: Address,
        proposal_id: u64,
    ) -> Result<(), errors::GovernanceError> {
        multisig::ms_approve(&env, approver, proposal_id)
    }

    pub fn ms_execute(
        env: Env,
        executor: Address,
        proposal_id: u64,
    ) -> Result<(), errors::GovernanceError> {
        multisig::ms_execute(&env, executor, proposal_id)
    }

    /// Borrow assets from the protocol.
    pub fn borrow_asset(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<i128, crate::borrow::BorrowError> {
        crate::borrow::borrow_asset(&env, user, asset, amount)
    }

    /// Repay borrowed assets.
    pub fn repay_debt(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<(i128, i128, i128), crate::repay::RepayError> {
        crate::repay::repay_debt(&env, user, asset, amount)
    }

    /// Liquidate an undercollateralized position.
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Option<Address>,
        collateral_asset: Option<Address>,
        amount: i128,
    ) -> Result<i128, crate::liquidate::LiquidationError> {
        let (repaid, _seized, _fee) = liquidate(&env, caller, borrower, asset, None, amount)?;
        Ok(repaid)
    }

    /// Get current risk configuration.
    pub fn get_risk_config(env: Env) -> Option<RiskConfig> {
        risk_management::get_risk_config(&env)
    }

    pub fn set_pause_switch(
        env: Env,
        admin: Address,
        operation: Symbol,
        paused: bool,
    ) -> Result<(), RiskManagementError> {
        risk_management::set_pause_switch(&env, admin, operation, paused)
    }

    pub fn is_operation_paused(env: Env, operation: Symbol) -> bool {
        risk_management::is_operation_paused(&env, operation)
    }

    pub fn is_emergency_paused(env: Env) -> bool {
        risk_management::is_emergency_paused(&env)
    }

    pub fn set_emergency_pause(
        env: Env,
        admin: Address,
        paused: bool,
    ) -> Result<(), RiskManagementError> {
        risk_management::set_emergency_pause(&env, admin, paused)
    }

    /// Get minimum collateral ratio.
    /// Get a read-only configuration snapshot of the protocol
    ///
    /// # Returns
    /// Returns Some(ConfigSnapshot) if initialized, None otherwise.
    /// No authorization required - safe for any caller.
    pub fn get_config_snapshot(env: Env) -> Option<ConfigSnapshot> {
        get_config_snapshot(&env)
    }

    /// Get minimum collateral ratio
    ///
    /// # Returns
    /// Returns the minimum collateral ratio in basis points
    pub fn get_min_collateral_ratio(env: Env) -> Result<i128, RiskManagementError> {
        risk_params::get_min_collateral_ratio(&env)
            .map_err(|_| RiskManagementError::InvalidParameter)
    }

    /// Get liquidation threshold.
    pub fn get_liquidation_threshold(env: Env) -> Result<i128, RiskManagementError> {
        risk_params::get_liquidation_threshold(&env)
            .map_err(|_| RiskManagementError::InvalidParameter)
    }

    /// Get close factor.
    pub fn get_close_factor(env: Env) -> Result<i128, RiskManagementError> {
        risk_params::get_close_factor(&env).map_err(|_| RiskManagementError::InvalidParameter)
    }

    /// Get liquidation incentive.
    pub fn get_liquidation_incentive(env: Env) -> Result<i128, RiskManagementError> {
        risk_params::get_liquidation_incentive(&env)
            .map_err(|_| RiskManagementError::InvalidParameter)
    }

    /// Get current utilization (in basis points).
    pub fn get_utilization(env: Env) -> i128 {
        interest_rate::calculate_utilization(&env).unwrap_or(0)
    }

    /// Get current borrow rate (in basis points).
    pub fn get_borrow_rate(env: Env) -> i128 {
        interest_rate::calculate_borrow_rate(&env).unwrap_or(0)
    }

    /// Get current supply rate (in basis points).
    pub fn get_supply_rate(env: Env) -> i128 {
        interest_rate::calculate_supply_rate(&env).unwrap_or(0)
    }

    /// Get protocol utilization in basis points.
    pub fn get_utilization(env: Env) -> i128 {
        analytics::get_protocol_utilization(&env).unwrap_or(0)
    }

    /// Configure flash-loan parameters (admin only).
    pub fn configure_flash_loan(
        env: Env,
        caller: Address,
        config: FlashLoanConfig,
    ) -> Result<(), crate::flash_loan::FlashLoanError> {
        flash_loan::configure_flash_loan(&env, caller, config)
    }

    /// Set flash-loan fee in basis points (admin only).
    pub fn set_flash_loan_fee(
        env: Env,
        caller: Address,
        fee_bps: i128,
    ) -> Result<(), crate::flash_loan::FlashLoanError> {
        flash_loan::set_flash_loan_fee(&env, caller, fee_bps)
    }

    /// Set emergency interest-rate adjustment in basis points (admin only).
    pub fn set_emergency_rate_adjustment(
        env: Env,
        caller: Address,
        adjustment_bps: i128,
    ) -> Result<(), crate::interest_rate::InterestRateError> {
        interest_rate::set_emergency_rate_adjustment(&env, caller, adjustment_bps)
    }

    /// Update interest rate model configuration (admin only).
    #[allow(clippy::too_many_arguments)]
    pub fn update_interest_rate_config(
        env: Env,
        admin: Address,
        base_rate: Option<i128>,
        kink: Option<i128>,
        multiplier: Option<i128>,
        jump_multiplier: Option<i128>,
        rate_floor: Option<i128>,
        rate_ceiling: Option<i128>,
        spread: Option<i128>,
    ) -> Result<(), RiskManagementError> {
        require_admin(&env, &admin)?;
        interest_rate::update_interest_rate_config(
            &env,
            admin,
            base_rate,
            kink,
            multiplier,
            jump_multiplier,
            rate_floor,
            rate_ceiling,
            spread,
        )
        .map_err(|_| RiskManagementError::InvalidParameter)
    }

    /// Get current protocol utilization in basis points (0–10 000).
    pub fn get_utilization(env: Env) -> i128 {
        interest_rate::calculate_utilization(&env).unwrap_or(0)
    }

    /// Set an emergency rate adjustment (admin only).
    ///
    /// The adjustment is added to the calculated borrow rate.
    /// Bounded to ±10 000 bps (±100%).
    pub fn set_emergency_rate_adjustment(
        env: Env,
        admin: Address,
        adjustment_bps: i128,
    ) -> Result<(), RiskManagementError> {
        require_admin(&env, &admin)?;
        interest_rate::set_emergency_rate_adjustment(&env, admin, adjustment_bps)
            .map_err(|_| RiskManagementError::InvalidParameter)
    }

    /// Get the current interest rate configuration.
    pub fn get_interest_rate_config(env: Env) -> Option<InterestRateConfig> {
        interest_rate::get_interest_rate_config(&env)
    }

    /// Check if a position meets minimum collateral ratio.
    pub fn require_min_collateral_ratio(
        env: Env,
        collateral_value: i128,
        debt_value: i128,
    ) -> Result<(), RiskManagementError> {
        crate::risk_params::require_min_collateral_ratio(&env, collateral_value, debt_value)
            .map_err(|_| RiskManagementError::InsufficientCollateralRatio)
    }

    /// Enforce minimum collateral ratio.
    pub fn require_min_collateral_ratio(
        env: Env,
        collateral_value: i128,
        debt_value: i128,
    ) -> Result<(), RiskManagementError> {
        risk_params::require_min_collateral_ratio(&env, collateral_value, debt_value)
            .map_err(|_| RiskManagementError::InsufficientCollateralRatio)
    }

    /// Check if position can be liquidated.
    pub fn can_be_liquidated(
        env: Env,
        collateral_value: i128,
        debt_value: i128,
    ) -> Result<bool, RiskManagementError> {
        can_be_liquidated(&env, collateral_value, debt_value)
            .map_err(|_| RiskManagementError::InvalidParameter)
    }

    /// Get maximum liquidatable amount.
    pub fn get_max_liquidatable_amount(
        env: Env,
        debt_value: i128,
    ) -> Result<i128, RiskManagementError> {
        get_max_liquidatable_amount(&env, debt_value).map_err(|_| RiskManagementError::Overflow)
    }

    /// Calculate liquidation incentive amount.
    pub fn get_liquidation_incentive_amount(
        env: Env,
        liquidated_amount: i128,
    ) -> Result<i128, RiskManagementError> {
        get_liquidation_incentive_amount(&env, liquidated_amount)
            .map_err(|_| RiskManagementError::Overflow)
    }

    /// Refresh analytics for a user.
    pub fn refresh_user_analytics(_env: Env, _user: Address) -> Result<(), RiskManagementError> {
        Ok(())
    }

    /// Claim accumulated protocol reserves (admin only)
    /// Claim accumulated protocol reserves (admin only).
    pub fn claim_reserves(
        env: Env,
        caller: Address,
        asset: Option<Address>,
        _to: Address,
        amount: i128,
    ) -> Result<(), RiskManagementError> {
        require_admin(&env, &caller)?;

        let reserve_key = DepositDataKey::ProtocolReserve(asset.clone());
        let mut reserve_balance = env
            .storage()
            .persistent()
            .get::<DepositDataKey, i128>(&reserve_key)
            .unwrap_or(0);

        if amount > reserve_balance {
            return Err(RiskManagementError::InvalidParameter);
        }

        if let Some(_asset_addr) = asset {
            #[cfg(not(test))]
            {
                let token_client = soroban_sdk::token::Client::new(&env, &_asset_addr);
                token_client.transfer(&env.current_contract_address(), &to, &amount);
            }
        }

        reserve_balance -= amount;
        env.storage()
            .persistent()
            .set(&reserve_key, &reserve_balance);
        Ok(())
    }

    /// Get current protocol reserve balance for an asset.
    pub fn get_reserve_balance(env: Env, asset: Option<Address>) -> i128 {
        let reserve_key = DepositDataKey::ProtocolReserve(asset);
        env.storage()
            .persistent()
            .get::<DepositDataKey, i128>(&reserve_key)
            .unwrap_or(0)
    }

    /// Generate a comprehensive protocol report.
    pub fn get_protocol_report(env: Env) -> Result<ProtocolReport, AnalyticsError> {
        generate_protocol_report(&env)
    }

    /// Generate a comprehensive report for a specific user.
    pub fn get_user_report(env: Env, user: Address) -> Result<UserReport, AnalyticsError> {
        generate_user_report(&env, &user)
    }

    /// Retrieve recent protocol activity entries.
    pub fn get_recent_activity(
        env: Env,
        limit: u32,
        offset: u32,
    ) -> Result<soroban_sdk::Vec<analytics::ActivityEntry>, AnalyticsError> {
        get_recent_activity(&env, limit, offset)
    }

    /// Retrieve activity entries for a specific user.
    pub fn get_user_activity(
        env: Env,
        user: Address,
        limit: u32,
        offset: u32,
    ) -> Result<soroban_sdk::Vec<analytics::ActivityEntry>, AnalyticsError> {
        get_user_activity_feed(&env, &user, limit, offset)
    }

    /// Get user analytics metrics.
    pub fn get_user_analytics(
        env: Env,
        user: Address,
    ) -> Result<crate::analytics::UserMetrics, crate::analytics::AnalyticsError> {
        analytics::get_user_activity_summary(&env, &user)
    }

    /// Get protocol analytics metrics.
    pub fn get_protocol_analytics(
        env: Env,
    ) -> Result<crate::analytics::ProtocolMetrics, crate::analytics::AnalyticsError> {
        analytics::get_protocol_stats(&env)
    }

    // ============================================================================
    // Oracle Methods
    // ============================================================================

    /// Update price feed from oracle.
    pub fn update_price_feed(
        env: Env,
        caller: Address,
        asset: Address,
        price: i128,
        decimals: u32,
        oracle: Address,
    ) -> i128 {
        oracle::update_price_feed(&env, caller, asset, price, decimals, oracle)
            .expect("Oracle error")
    }

    /// Get current price for an asset.
    pub fn get_price(env: Env, asset: Address) -> i128 {
        oracle::get_price(&env, &asset).expect("Oracle error")
    }

    /// Configure oracle parameters (admin only)
    /// Configure oracle parameters (admin only).
    pub fn configure_oracle(env: Env, caller: Address, config: FullOracleConfig) {
        oracle::configure_oracle(&env, caller, config).expect("Oracle error")
    }

    /// Set primary oracle for an asset (admin only).
    pub fn set_primary_oracle(env: Env, caller: Address, asset: Address, primary_oracle: Address) {
        oracle::set_primary_oracle(&env, caller, asset, primary_oracle)
            .unwrap_or_else(|e| panic!("Oracle error: {:?}", e))
    }

    /// Set fallback oracle for an asset (admin only).
    pub fn set_fallback_oracle(
        env: Env,
        caller: Address,
        asset: Address,
        fallback_oracle: Address,
    ) {
        oracle::set_fallback_oracle(&env, caller, asset, fallback_oracle).expect("Oracle error")
    }

    /// Read-only view: the AMM TWAP fallback price for `asset` over
    /// `window_secs`, at the AMM accumulator's native scale (1e18 — see
    /// `oracle::TWAP_PRICE_SCALE`). Calls the same `amm_twap::get_twap` path
    /// the oracle's stale-primary fallback uses internally.
    ///
    /// Returns `None` (never panics/aborts) when no snapshot covers the
    /// requested window — e.g. the pool is too new, has no TWAP history yet,
    /// or `window_secs` is below the protocol minimum. Pure read; does not
    /// mutate contract state.
    pub fn get_pool_twap_price(env: Env, asset: Address, window_secs: u64) -> Option<u128> {
        oracle::get_pool_twap_price(&env, &asset, window_secs)
    }

    // ============================================================================
    // Risk Management Methods
    // ============================================================================

    /// Initialize risk management (admin only).
    pub fn initialize_risk_management(env: Env, admin: Address) -> Result<(), RiskManagementError> {
        risk_management::initialize_risk_management(&env, admin)
    }

    /// Set a pause switch for an operation (admin only).
    pub fn set_pause_switch(
        env: Env,
        admin: Address,
        operation: Symbol,
        paused: bool,
    ) -> Result<(), RiskManagementError> {
        risk_management::set_pause_switch(&env, admin, operation, paused)
    }

    /// Check if an operation is paused.
    pub fn is_operation_paused(env: Env, operation: Symbol) -> bool {
        risk_management::is_operation_paused(&env, operation)
    }

    /// Check if emergency pause is active.
    pub fn is_emergency_paused(env: Env) -> bool {
        risk_management::is_emergency_paused(&env)
    }

    /// Set emergency pause (admin only).
    pub fn set_emergency_pause(
        env: Env,
        admin: Address,
        paused: bool,
    ) -> Result<(), RiskManagementError> {
        risk_management::set_emergency_pause(&env, admin, paused)
    }

    // ============================================================================
    // AMM Methods
    // ============================================================================

    /// Initialize AMM settings (admin only).
    pub fn initialize_amm(
        env: Env,
        admin: Address,
        default_slippage: i128,
        max_slippage: i128,
        auto_swap_threshold: i128,
    ) -> Result<(), amm::AmmError> {
        amm::initialize_amm(
            env,
            admin,
            default_slippage,
            max_slippage,
            auto_swap_threshold,
        )
    }

    /// Set AMM pool configuration (admin only).
    pub fn set_amm_pool(
        env: Env,
        admin: Address,
        protocol_config: amm::AmmProtocolConfig,
    ) -> Result<(), amm::AmmError> {
        amm::set_amm_pool(env, admin, protocol_config)
    }

    /// Execute swap through AMM.
    pub fn amm_swap(env: Env, user: Address, params: SwapParams) -> Result<i128, AmmError> {
        amm::amm_swap(env, user, params)
    }

    // ============================================================================
    // Bridge Methods
    // ============================================================================

    /// Register a bridge (admin only).
    pub fn register_bridge(
        env: Env,
        caller: Address,
        network_id: u32,
        bridge: Address,
        fee_bps: i128,
    ) -> Result<(), BridgeError> {
        bridge::register_bridge(&env, caller, network_id, bridge, fee_bps)
    }

    /// Set bridge fee
    ///
    /// # Arguments
    /// * `caller` - Admin address for authorization
    /// * `network_id` - ID of the remote network
    /// * `fee_bps` - New fee in basis points
    /// Set bridge fee (admin only).
    pub fn set_bridge_fee(
        env: Env,
        caller: Address,
        network_id: u32,
        fee_bps: i128,
    ) -> Result<(), BridgeError> {
        bridge::set_bridge_fee(&env, caller, network_id, fee_bps)
    }

    /// Deposit through a bridge.
    pub fn bridge_deposit(
        env: Env,
        user: Address,
        network_id: u32,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<i128, BridgeError> {
        bridge::bridge_deposit(&env, user, network_id, asset, amount)
    }

    /// Withdraw through a bridge.
    pub fn bridge_withdraw(
        env: Env,
        user: Address,
        network_id: u32,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<i128, BridgeError> {
        bridge::bridge_withdraw(&env, user, network_id, asset, amount)
    }

    /// List all bridges.
    pub fn list_bridges(env: Env) -> Map<u32, BridgeConfig> {
        bridge::list_bridges(&env)
    }

    /// Get configuration of a specific bridge.
    pub fn get_bridge_config(env: Env, network_id: u32) -> Result<BridgeConfig, BridgeError> {
        bridge::get_bridge_config(&env, network_id)
    }

    // ============================================================================
    // Config Methods
    // ============================================================================

    /// Set a configuration value (admin only).
    pub fn config_set(
        env: Env,
        caller: Address,
        key: soroban_sdk::Symbol,
        value: soroban_sdk::Val,
    ) -> Result<(), ConfigError> {
        config_set(&env, caller, key, value)
    }

    /// Get a configuration value.
    pub fn config_get(env: Env, key: soroban_sdk::Symbol) -> Option<soroban_sdk::Val> {
        config_get(&env, key)
    }

    /// Backup configuration parameters (admin only).
    pub fn config_backup(
        env: Env,
        caller: Address,
        keys: soroban_sdk::Vec<soroban_sdk::Symbol>,
    ) -> Result<soroban_sdk::Vec<(soroban_sdk::Symbol, soroban_sdk::Val)>, ConfigError> {
        config_backup(&env, caller, keys)
    }

    /// Restore configuration parameters (admin only).
    pub fn config_restore(
        env: Env,
        caller: Address,
        backup: soroban_sdk::Vec<(soroban_sdk::Symbol, soroban_sdk::Val)>,
    ) -> Result<(), ConfigError> {
        config_restore(&env, caller, backup)
    }

    // ============================================================================
    // Cross-Asset Methods
    // ============================================================================

    /// Initialize cross-asset lending module (admin only).
    pub fn initialize_ca(env: Env, admin: Address) -> Result<(), CrossAssetError> {
        cross_asset::initialize(&env, admin)
    }

    /// Initialize/register a new asset with configuration.
    pub fn initialize_asset(
        env: Env,
        asset: Option<Address>,
        config: AssetConfig,
    ) -> Result<(), CrossAssetError> {
        initialize_asset(&env, asset, config)
    }

    /// Update asset configuration (admin only).
    #[allow(clippy::too_many_arguments)]
    pub fn update_asset_config(
        env: Env,
        asset: Option<Address>,
        collateral_factor: Option<i128>,
        liquidation_threshold: Option<i128>,
        max_supply: Option<i128>,
        max_borrow: Option<i128>,
        can_collateralize: Option<bool>,
        can_borrow: Option<bool>,
    ) -> Result<(), CrossAssetError> {
        update_asset_config(
            &env,
            asset,
            collateral_factor,
            liquidation_threshold,
            max_supply,
            max_borrow,
            can_collateralize,
            can_borrow,
        )
    }

    /// Update asset price (admin/oracle only).
    pub fn update_asset_price(
        env: Env,
        asset: Option<Address>,
        price: i128,
    ) -> Result<(), CrossAssetError> {
        update_asset_price(&env, asset, price)
    }

    /// Get asset configuration.
    pub fn get_asset_config(
        env: Env,
        asset: Option<Address>,
    ) -> Result<AssetConfig, CrossAssetError> {
        get_asset_config_by_address(&env, asset)
    }

    /// Get list of all configured assets.
    pub fn get_asset_list(env: Env) -> soroban_sdk::Vec<AssetKey> {
        get_asset_list(&env)
    }

    /// Deposit collateral for cross-asset lending.
    pub fn cross_asset_deposit(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_deposit(&env, user, asset, amount)
    }

    /// Withdraw collateral from cross-asset lending.
    pub fn cross_asset_withdraw(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_withdraw(&env, user, asset, amount)
    }

    /// Borrow asset in cross-asset lending.
    pub fn cross_asset_borrow(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_borrow(&env, user, asset, amount)
    }

    /// Repay borrowed asset in cross-asset lending.
    pub fn cross_asset_repay(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_repay(&env, user, asset, amount)
    }

    /// Get user's position for a specific asset.
    pub fn get_user_asset_position(
        env: Env,
        user: Address,
        asset: Option<Address>,
    ) -> AssetPosition {
        get_user_asset_position(&env, &user, asset)
    }

    /// Get user's unified position summary across all assets.
    pub fn get_user_position_summary(
        env: Env,
        user: Address,
    ) -> Result<UserPositionSummary, CrossAssetError> {
        get_user_position_summary(&env, &user)
    }

    /// Get total supply for a specific asset.
    pub fn get_total_supply_for(env: Env, asset: Option<Address>) -> i128 {
        get_total_supply_for(&env, asset)
    }

    /// Get total borrows for a specific asset.
    pub fn get_total_borrow_for(env: Env, asset: Option<Address>) -> i128 {
        get_total_borrow_for(&env, asset)
    }

    // ============================================================================
    // Governance Entrypoints
    // ============================================================================

    /// Initialize governance module.
    pub fn gov_initialize(
        env: Env,
        admin: Address,
        vote_token: Address,
        voting_period: Option<u64>,
        execution_delay: Option<u64>,
        quorum_bps: Option<u32>,
        proposal_threshold: Option<i128>,
        timelock_duration: Option<u64>,
        default_voting_threshold: Option<i128>,
    ) -> Result<(), errors::GovernanceError> {
        governance::initialize(
            &env,
            admin,
            vote_token,
            voting_period,
            execution_delay,
            quorum_bps,
            proposal_threshold,
            timelock_duration,
            default_voting_threshold,
        )
    }

    /// Create a new governance proposal.
    pub fn gov_create_proposal(
        env: Env,
        proposer: Address,
        proposal_type: ProposalType,
        description: soroban_sdk::String,
        voting_threshold: Option<i128>,
    ) -> Result<u64, errors::GovernanceError> {
        let soroban_desc = soroban_sdk::String::from_str(&env, &description.to_string());
        governance::create_proposal(
            &env,
            proposer,
            proposal_type,
            soroban_desc,
            voting_threshold,
        )
    }

    /// Cast a vote on a proposal.
    pub fn gov_vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        vote_type: VoteType,
    ) -> Result<(), errors::GovernanceError> {
        governance::vote(&env, voter, proposal_id, vote_type)
    }

    /// Queue a successful proposal for execution.
    pub fn gov_queue_proposal(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<ProposalOutcome, errors::GovernanceError> {
        governance::queue_proposal(&env, caller, proposal_id)
    }

    /// Execute a queued proposal.
    pub fn gov_execute_proposal(
        env: Env,
        executor: Address,
        proposal_id: u64,
    ) -> Result<(), errors::GovernanceError> {
        governance::execute_proposal(&env, executor, proposal_id)
    }

    /// Cancel a proposal.
    pub fn gov_cancel_proposal(
        env: Env,
        caller: Address,
        proposal_id: u64,
    ) -> Result<(), errors::GovernanceError> {
        governance::cancel_proposal(&env, caller, proposal_id)
    }

    /// Approve a proposal as multisig admin.
    pub fn gov_approve_proposal(
        env: Env,
        approver: Address,
        proposal_id: u64,
    ) -> Result<(), errors::GovernanceError> {
        governance::approve_proposal(&env, approver, proposal_id)
    }

    /// Set multisig configuration.
    pub fn gov_set_multisig_config(
        env: Env,
        caller: Address,
        admins: Vec<Address>,
        threshold: u32,
    ) -> Result<(), errors::GovernanceError> {
        governance::set_multisig_config(&env, caller, admins, threshold)
    }

    /// Add a guardian.
    pub fn gov_add_guardian(
        env: Env,
        caller: Address,
        guardian: Address,
    ) -> Result<(), errors::GovernanceError> {
        governance::add_guardian(&env, caller, guardian)
    }

    /// Remove a guardian.
    pub fn gov_remove_guardian(
        env: Env,
        caller: Address,
        guardian: Address,
    ) -> Result<(), errors::GovernanceError> {
        governance::remove_guardian(&env, caller, guardian)
    }

    /// Set guardian threshold.
    pub fn gov_set_guardian_threshold(
        env: Env,
        caller: Address,
        threshold: u32,
    ) -> Result<(), errors::GovernanceError> {
        governance::set_guardian_threshold(&env, caller, threshold)
    }

    /// Start recovery process.
    pub fn gov_start_recovery(
        env: Env,
        initiator: Address,
        old_admin: Address,
        new_admin: Address,
    ) -> Result<(), errors::GovernanceError> {
        governance::start_recovery(&env, initiator, old_admin, new_admin)
    }

    /// Approve recovery.
    pub fn gov_approve_recovery(
        env: Env,
        approver: Address,
    ) -> Result<(), errors::GovernanceError> {
        governance::approve_recovery(&env, approver)
    }

    /// Execute recovery.
    pub fn gov_execute_recovery(
        env: Env,
        executor: Address,
    ) -> Result<(), errors::GovernanceError> {
        governance::execute_recovery(&env, executor)
    }

    // ============================================================================
    /// Deposit collateral for a specific asset (cross-asset lending).
    pub fn ca_deposit_collateral(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_deposit(&env, user, asset, amount)
    }

    /// Withdraw collateral for a specific asset (cross-asset lending).
    pub fn ca_withdraw_collateral(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_withdraw(&env, user, asset, amount)
    }

    /// Borrow a specific asset (cross-asset lending).
    pub fn ca_borrow_asset(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_borrow(&env, user, asset, amount)
    }

    /// Repay debt for a specific asset (cross-asset lending).
    pub fn ca_repay_debt(
        env: Env,
        user: Address,
        asset: Option<Address>,
        amount: i128,
    ) -> Result<AssetPosition, CrossAssetError> {
        cross_asset::cross_asset_repay(&env, user, asset, amount)
    }

    // Governance Query Functions
    // ============================================================================
    // Governance Query Functions
    // ============================================================================

    /// Get proposal by ID.
    pub fn gov_get_proposal(env: Env, proposal_id: u64) -> Option<Proposal> {
        governance::get_proposal(&env, proposal_id)
    }

    /// Get vote information.
    pub fn gov_get_vote(env: Env, proposal_id: u64, voter: Address) -> Option<VoteInfo> {
        governance::get_vote(&env, proposal_id, voter)
    }

    /// Get governance configuration.
    pub fn gov_get_config(env: Env) -> Option<GovernanceConfig> {
        governance::get_config(&env)
    }

    /// Get governance admin.
    pub fn gov_get_admin(env: Env) -> Option<Address> {
        governance::get_admin(&env)
    }

    /// Get multisig configuration.
    pub fn gov_get_multisig_config(env: Env) -> Option<MultisigConfig> {
        governance::get_multisig_config(&env)
    }

    /// Get guardian configuration.
    pub fn gov_get_guardian_config(env: Env) -> Option<GuardianConfig> {
        governance::get_guardian_config(&env)
    }

    /// Get proposal approvals.
    pub fn gov_get_proposal_approvals(env: Env, proposal_id: u64) -> Option<Vec<Address>> {
        governance::get_proposal_approvals(&env, proposal_id)
    }

    /// Get current recovery request.
    pub fn gov_get_recovery_request(env: Env) -> Option<RecoveryRequest> {
        governance::get_recovery_request(&env)
    }

    /// Get recovery approvals.
    pub fn gov_get_recovery_approvals(env: Env) -> Option<Vec<Address>> {
        governance::get_recovery_approvals(&env)
    }

    /// Get paginated list of proposals.
    pub fn gov_get_proposals(env: Env, start_id: u64, limit: u32) -> Vec<Proposal> {
        governance::get_proposals(&env, start_id, limit)
    }

    /// Check if an address can vote on a proposal.
    pub fn gov_can_vote(env: Env, voter: Address, proposal_id: u64) -> bool {
        governance::can_vote(&env, voter, proposal_id)
    }
}

mod flash_loan_test;
#[cfg(test)]
#[cfg(test)]
mod test_reentrancy;

#[cfg(test)]
mod amm_pause_integration_test;
#[cfg(test)]
mod claim_reserves_test;

// mod governance_test;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_borrow_and_repay() {
        let (_env, client, _admin, user) = setup();
        assert_eq!(client.borrow(&user, &200), 200);
        assert_eq!(client.repay(&user, &75), 125);
    }

    #[test]
    fn test_get_state_default() {
        let (_env, client, _admin, user) = setup();
        let s = client.get_state(&user);
        assert_eq!(s.balance, 0);
        assert_eq!(s.debt, 0);
    }

    #[test]
    fn test_get_state_after_actions() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &500);
        client.borrow(&user, &100);
        let s = client.get_state(&user);
        assert_eq!(s.balance, 500);
        assert_eq!(s.debt, 100);
    }

    #[test]
    fn test_deposit_rejects_zero_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.deposit(&user, &0);
        }));
        assert!(result.is_err(), "deposit should panic with zero amount");
    }

    #[test]
    fn test_deposit_rejects_negative_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.deposit(&user, &-100);
        }));
        assert!(result.is_err(), "deposit should panic with negative amount");
    }

    #[test]
    fn test_withdraw_rejects_zero_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.withdraw(&user, &0);
        }));
        assert!(result.is_err(), "withdraw should panic with zero amount");
    }

    #[test]
    fn test_withdraw_rejects_negative_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.withdraw(&user, &-100);
        }));
        assert!(
            result.is_err(),
            "withdraw should panic with negative amount"
        );
    }

    #[test]
    fn test_borrow_rejects_zero_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.borrow(&user, &0);
        }));
        assert!(result.is_err(), "borrow should panic with zero amount");
    }

    #[test]
    fn test_borrow_rejects_negative_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.borrow(&user, &-100);
        }));
        assert!(result.is_err(), "borrow should panic with negative amount");
    }

    #[test]
    fn test_repay_rejects_zero_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.repay(&user, &0);
        }));
        assert!(result.is_err(), "repay should panic with zero amount");
    }

    #[test]
    fn test_repay_rejects_negative_amount() {
        let (env, client, _admin, user) = setup();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.repay(&user, &-100);
        }));
        assert!(result.is_err(), "repay should panic with negative amount");
    }
}
