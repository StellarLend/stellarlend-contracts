use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Map, Symbol, Vec, Val, String};

#![allow(deprecated)]
#![allow(unused_imports)]
#![allow(dead_code)]

pub mod admin;
pub mod amm;
pub mod analytics;
pub mod borrow;
pub mod bridge;
pub mod config;
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
pub mod repay;
pub mod reserve;
pub mod risk_management;
pub mod config_snapshot;
pub mod risk_params;
pub mod storage;
pub mod types;
pub mod withdraw;
pub mod reentrancy;
pub mod config_backup;

use crate::oracle::OracleConfig;
use crate::risk_management::{RiskConfig, RiskManagementError, initialize_risk_management, require_admin, check_emergency_pause};
use crate::risk_params::{initialize_risk_params, RiskParamsError, require_min_collateral_ratio, can_be_liquidated, get_max_liquidatable_amount, get_liquidation_incentive_amount};
use crate::interest_rate::{initialize_interest_rate_config, InterestRateError};
use crate::config_snapshot::{get_config_snapshot, ConfigSnapshot};
use crate::analytics::{ProtocolReport, UserReport, AnalyticsError, generate_protocol_report, generate_user_report, ActivityEntry, UserMetrics, ProtocolMetrics};
use crate::deposit::{DepositDataKey, ProtocolAnalytics};
use crate::cross_asset::{AssetConfig, AssetKey, AssetPosition, UserPositionSummary, CrossAssetError};
use crate::types::{ProposalType, VoteType, ProposalOutcome, Proposal, VoteInfo, GovernanceConfig, MultisigConfig, RecoveryRequest};
use crate::errors::GovernanceError;
use crate::bridge::{BridgeConfig, BridgeError};
use crate::config::ConfigError;
use crate::amm::{AmmError, AmmProtocolConfig, SwapParams, LiquidityParams};
use crate::flash_loan::{FlashLoanError, FlashLoanConfig};

#[contract]
pub struct HelloContract;

#[contractimpl]
impl HelloContract {
    /// Health-check endpoint. Returns "Hello".
    pub fn hello(env: Env) -> String {
        String::from_str(&env, "Hello")
    }

    /// Initialize the contract with admin address.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RiskManagementError> {
        if crate::admin::has_admin(&env) {
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

    // --- Admin & Roles ---
    pub fn transfer_admin(env: Env, caller: Address, new_admin: Address) -> Result<(), crate::admin::AdminError> {
        crate::admin::set_admin(&env, new_admin, Some(caller))
    }
    pub fn grant_role(env: Env, caller: Address, role: Symbol, account: Address) -> Result<(), crate::admin::AdminError> {
        crate::admin::grant_role(&env, caller, role, account)
    }
    pub fn revoke_role(env: Env, caller: Address, role: Symbol, account: Address) -> Result<(), crate::admin::AdminError> {
        crate::admin::revoke_role(&env, caller, role, account)
    }
    pub fn has_admin(env: Env) -> bool { crate::admin::has_admin(&env) }
    pub fn get_admin_address(env: Env) -> Option<Address> { crate::admin::get_admin(&env) }
    pub fn has_role(env: Env, account: Address, role: Symbol) -> bool { crate::admin::has_role(&env, account, role) }

    // --- Core Lending & Operations ---
    pub fn deposit_collateral(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<i128, crate::deposit::DepositError> {
        crate::deposit::deposit_collateral(&env, user, asset, amount)
    }
    pub fn set_native_asset_address(env: Env, caller: Address, native_asset: Address) -> Result<(), crate::deposit::DepositError> {
        crate::deposit::set_native_asset_address(&env, caller, native_asset)
    }
    pub fn borrow_asset(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<i128, crate::borrow::BorrowError> {
        crate::borrow::borrow_asset(&env, user, asset, amount)
    }
    pub fn repay_debt(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<(i128, i128, i128), crate::repay::RepayError> {
        crate::repay::repay_debt(&env, user, asset, amount)
    }
    pub fn withdraw_collateral(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<i128, crate::withdraw::WithdrawError> {
        crate::withdraw::withdraw_collateral(&env, user, asset, amount)
    }
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Option<Address>,
        collateral_asset: Option<Address>,
        debt_amount: i128,
    ) -> Result<(i128, i128, i128), crate::liquidate::LiquidationError> {
        crate::liquidate::liquidate(&env, liquidator, borrower, debt_asset, collateral_asset, debt_amount)
    }

    // --- Risk Management & Health ---
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
        crate::risk_params::set_risk_params(&env, min_collateral_ratio, liquidation_threshold, close_factor, liquidation_incentive)
            .map_err(|e| match e {
                RiskParamsError::ParameterChangeTooLarge => RiskManagementError::ParameterChangeTooLarge,
                RiskParamsError::InvalidCollateralRatio => RiskManagementError::InvalidCollateralRatio,
                RiskParamsError::InvalidLiquidationThreshold => RiskManagementError::InvalidLiquidationThreshold,
                RiskParamsError::InvalidCloseFactor => RiskManagementError::InvalidCloseFactor,
                RiskParamsError::InvalidLiquidationIncentive => RiskManagementError::InvalidLiquidationIncentive,
                _ => RiskManagementError::InvalidParameter,
            })
    }
    pub fn get_config_snapshot(env: Env) -> Option<ConfigSnapshot> { crate::config_snapshot::get_config_snapshot(&env) }
    pub fn get_risk_config(env: Env) -> Option<RiskConfig> { crate::risk_management::get_risk_config(&env) }
    pub fn get_min_collateral_ratio(env: Env) -> Result<i128, RiskManagementError> {
        crate::risk_params::get_min_collateral_ratio(&env).map_err(|_| RiskManagementError::InvalidParameter)
    }
    pub fn get_liquidation_threshold(env: Env) -> Result<i128, RiskManagementError> {
        crate::risk_params::get_liquidation_threshold(&env).map_err(|_| RiskManagementError::InvalidParameter)
    }
    pub fn get_close_factor(env: Env) -> Result<i128, RiskManagementError> {
        crate::risk_params::get_close_factor(&env).map_err(|_| RiskManagementError::InvalidParameter)
    }
    pub fn get_liquidation_incentive(env: Env) -> Result<i128, RiskManagementError> {
        crate::risk_params::get_liquidation_incentive(&env).map_err(|_| RiskManagementError::InvalidParameter)
    }
    pub fn get_borrow_rate(env: Env) -> i128 { crate::interest_rate::calculate_borrow_rate(&env).unwrap_or(0) }
    pub fn get_supply_rate(env: Env) -> i128 { crate::interest_rate::calculate_supply_rate(&env).unwrap_or(0) }

    pub fn check_min_collateral_ratio(env: Env, collateral_value: i128, debt_value: i128) -> Result<(), RiskManagementError> {
        require_min_collateral_ratio(&env, collateral_value, debt_value).map_err(|_| RiskManagementError::InsufficientCollateralRatio)
    }
    pub fn can_be_liquidated_check(env: Env, collateral_value: i128, debt_value: i128) -> Result<bool, RiskManagementError> {
        can_be_liquidated(&env, collateral_value, debt_value).map_err(|_| RiskManagementError::InvalidParameter)
    }
    pub fn get_max_liquidatable_amount(env: Env, debt_value: i128) -> Result<i128, RiskManagementError> {
        get_max_liquidatable_amount(&env, debt_value).map_err(|_| RiskManagementError::Overflow)
    }
    pub fn get_liquidation_incentive_amount(env: Env, liquidated_amount: i128) -> Result<i128, RiskManagementError> {
        get_liquidation_incentive_amount(&env, liquidated_amount).map_err(|_| RiskManagementError::Overflow)
    }

    pub fn set_pause_switch(env: Env, admin: Address, operation: Symbol, paused: bool) -> Result<(), RiskManagementError> {
        crate::risk_management::set_pause_switch(&env, admin, operation, paused)
    }
    pub fn is_operation_paused(env: Env, operation: Symbol) -> bool { crate::risk_management::is_operation_paused(&env, operation) }
    pub fn is_emergency_paused(env: Env) -> bool { crate::risk_management::is_emergency_paused(&env) }
    pub fn set_emergency_pause(env: Env, admin: Address, paused: bool) -> Result<(), RiskManagementError> {
        crate::risk_management::set_emergency_pause(&env, admin, paused)
    }

    // --- Governance & Recovery Operations ---
    pub fn gov_initialize(env: Env, admin: Address, vote_token: Address, voting_period: Option<u64>, execution_delay: Option<u64>, quorum_bps: Option<u32>, proposal_threshold: Option<i128>, timelock_duration: Option<u64>, default_voting_threshold: Option<i128>) -> Result<(), GovernanceError> {
        crate::governance::initialize(&env, admin, vote_token, voting_period, execution_delay, quorum_bps, proposal_threshold, timelock_duration, default_voting_threshold)
    }
    pub fn gov_create_proposal(env: Env, proposer: Address, proposal_type: ProposalType, description: String, voting_threshold: Option<i128>) -> Result<u64, GovernanceError> {
        crate::governance::create_proposal(&env, proposer, proposal_type, description, voting_threshold)
    }
    pub fn gov_vote(env: Env, voter: Address, proposal_id: u64, vote_type: VoteType) -> Result<(), GovernanceError> {
        crate::governance::vote(&env, voter, proposal_id, vote_type)
    }
    pub fn gov_queue_proposal(env: Env, caller: Address, proposal_id: u64) -> Result<ProposalOutcome, GovernanceError> {
        crate::governance::queue_proposal(&env, caller, proposal_id)
    }
    pub fn gov_execute_proposal(env: Env, executor: Address, proposal_id: u64) -> Result<(), GovernanceError> {
        crate::governance::execute_proposal(&env, executor, proposal_id)
    }
    pub fn gov_cancel_proposal(env: Env, caller: Address, proposal_id: u64) -> Result<(), GovernanceError> {
        crate::governance::cancel_proposal(&env, caller, proposal_id)
    }
    pub fn gov_approve_proposal(env: Env, approver: Address, proposal_id: u64) -> Result<(), GovernanceError> {
        crate::governance::approve_proposal(&env, approver, proposal_id)
    }
    pub fn gov_set_multisig_config(env: Env, caller: Address, admins: Vec<Address>, threshold: u32) -> Result<(), GovernanceError> {
        crate::governance::set_multisig_config(&env, caller, admins, threshold)
    }

    pub fn set_guardians(env: Env, caller: Address, guardians: Vec<Address>, threshold: u32) -> Result<(), GovernanceError> {
        crate::recovery::set_guardians(&env, caller, guardians, threshold)
    }
    pub fn start_recovery(env: Env, initiator: Address, old_admin: Address, new_admin: Address) -> Result<(), GovernanceError> {
        crate::recovery::start_recovery(&env, initiator, old_admin, new_admin)
    }
    pub fn approve_recovery(env: Env, approver: Address) -> Result<(), GovernanceError> {
        crate::recovery::approve_recovery(&env, approver)
    }
    pub fn execute_recovery(env: Env, executor: Address) -> Result<(), GovernanceError> {
        crate::recovery::execute_recovery(&env, executor)
    }
    pub fn get_recovery_request(env: Env) -> Option<RecoveryRequest> { crate::governance::get_recovery_request(&env) }
    pub fn get_recovery_approvals(env: Env) -> Option<Vec<Address>> { crate::governance::get_recovery_approvals(&env) }
    pub fn get_guardians(env: Env) -> Option<Vec<Address>> { 
        crate::governance::get_guardian_config(&env).map(|c| c.guardians)
    }
    pub fn get_guardian_threshold(env: Env) -> u32 {
        crate::governance::get_guardian_config(&env).map(|c| c.threshold).unwrap_or(0)
    }

    pub fn ms_set_admins(env: Env, caller: Address, admins: Vec<Address>, threshold: u32) -> Result<(), GovernanceError> {
        crate::multisig::ms_set_admins(&env, caller, admins, threshold)
    }
    pub fn ms_propose_set_min_cr(env: Env, proposer: Address, new_ratio: i128) -> Result<u64, GovernanceError> {
        crate::multisig::ms_propose_set_min_cr(&env, proposer, new_ratio)
    }
    pub fn ms_approve(env: Env, approver: Address, proposal_id: u64) -> Result<(), GovernanceError> {
        crate::multisig::ms_approve(&env, approver, proposal_id)
    }
    pub fn ms_execute(env: Env, executor: Address, proposal_id: u64) -> Result<(), GovernanceError> {
        crate::multisig::ms_execute(&env, executor, proposal_id)
    }

    // --- Governance Queries ---
    pub fn gov_get_proposal(env: Env, proposal_id: u64) -> Option<Proposal> { crate::governance::get_proposal(&env, proposal_id) }
    pub fn gov_get_vote(env: Env, proposal_id: u64, voter: Address) -> Option<VoteInfo> { crate::governance::get_vote(&env, proposal_id, voter) }
    pub fn gov_get_config(env: Env) -> Option<GovernanceConfig> { crate::governance::get_config(&env) }
    pub fn gov_get_admin(env: Env) -> Option<Address> { crate::governance::get_admin(&env) }
    pub fn gov_get_multisig_config(env: Env) -> Option<MultisigConfig> { crate::governance::get_multisig_config(&env) }
    pub fn gov_get_guardian_config(env: Env) -> Option<crate::storage::GuardianConfig> { crate::governance::get_guardian_config(&env) }
    pub fn gov_get_proposal_approvals(env: Env, proposal_id: u64) -> Option<Vec<Address>> { crate::governance::get_proposal_approvals(&env, proposal_id) }
    pub fn gov_get_proposals(env: Env, start_id: u64, limit: u32) -> Vec<Proposal> { crate::governance::get_proposals(&env, start_id, limit) }
    pub fn gov_can_vote(env: Env, voter: Address, proposal_id: u64) -> bool { crate::governance::can_vote(&env, voter, proposal_id) }

    // --- Cross-Asset Operations ---
    pub fn initialize_ca(env: Env, admin: Address) -> Result<(), CrossAssetError> { crate::cross_asset::initialize(&env, admin) }
    pub fn initialize_asset(env: Env, asset: Option<Address>, config: AssetConfig) -> Result<(), CrossAssetError> { crate::cross_asset::initialize_asset(&env, asset, config) }
    pub fn ca_deposit_collateral(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<AssetPosition, CrossAssetError> { crate::cross_asset::cross_asset_deposit(&env, user, asset, amount) }
    pub fn ca_withdraw_collateral(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<AssetPosition, CrossAssetError> { crate::cross_asset::cross_asset_withdraw(&env, user, asset, amount) }
    pub fn ca_borrow_asset(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<AssetPosition, CrossAssetError> { crate::cross_asset::cross_asset_borrow(&env, user, asset, amount) }
    pub fn ca_repay_debt(env: Env, user: Address, asset: Option<Address>, amount: i128) -> Result<AssetPosition, CrossAssetError> { crate::cross_asset::cross_asset_repay(&env, user, asset, amount) }
    pub fn get_user_asset_position(env: Env, user: Address, asset: Option<Address>) -> AssetPosition { crate::cross_asset::get_user_asset_position(&env, &user, asset) }
    pub fn get_user_position_summary(env: Env, user: Address) -> Result<UserPositionSummary, CrossAssetError> { crate::cross_asset::get_user_position_summary(&env, &user) }
    pub fn get_asset_list(env: Env) -> Vec<AssetKey> { crate::cross_asset::get_asset_list(&env) }
    pub fn get_asset_config(env: Env, asset: Option<Address>) -> Result<AssetConfig, CrossAssetError> { crate::cross_asset::get_asset_config_by_address(&env, asset) }
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
        crate::cross_asset::update_asset_config(&env, asset, collateral_factor, liquidation_threshold, max_supply, max_borrow, can_collateralize, can_borrow)
    }

    // --- Analytics Reports ---
    pub fn get_protocol_report(env: Env) -> Result<ProtocolReport, AnalyticsError> { generate_protocol_report(&env) }
    pub fn get_user_report(env: Env, user: Address) -> Result<UserReport, AnalyticsError> { generate_user_report(&env, &user) }
    pub fn get_recent_activity(env: Env, limit: u32, offset: u32) -> Result<Vec<ActivityEntry>, AnalyticsError> { crate::analytics::get_recent_activity(&env, limit, offset) }
    pub fn get_user_activity(env: Env, user: Address, limit: u32, offset: u32) -> Result<Vec<ActivityEntry>, AnalyticsError> { crate::analytics::get_user_activity_feed(&env, &user, limit, offset) }
    pub fn get_user_analytics(env: Env, user: Address) -> Result<UserMetrics, AnalyticsError> { crate::analytics::get_user_activity_summary(&env, &user) }
    pub fn get_protocol_analytics(env: Env) -> Result<ProtocolMetrics, AnalyticsError> { crate::analytics::get_protocol_stats(&env) }

    // --- Oracle & Prices ---
    pub fn update_price_feed(env: Env, caller: Address, asset: Address, price: i128, decimals: u32, oracle: Address) -> i128 { 
        crate::oracle::update_price_feed(&env, caller, asset, price, decimals, oracle).expect("Oracle error")
    }
    pub fn get_price(env: Env, asset: Address) -> i128 { crate::oracle::get_price(&env, &asset).expect("Oracle error") }
    pub fn configure_oracle(env: Env, caller: Address, config: OracleConfig) { crate::oracle::configure_oracle(&env, caller, config).expect("Oracle error") }
    pub fn set_primary_oracle(env: Env, caller: Address, asset: Address, primary_oracle: Address) { crate::oracle::set_primary_oracle(&env, caller, asset, primary_oracle).expect("Oracle error") }
    pub fn set_fallback_oracle(env: Env, caller: Address, asset: Address, fallback_oracle: Address) { crate::oracle::set_fallback_oracle(&env, caller, asset, fallback_oracle).expect("Oracle error") }

    // --- AMM & Flash Loans ---
    pub fn initialize_amm(env: Env, admin: Address, default_slippage: i128, max_slippage: i128, auto_swap_threshold: i128) -> Result<(), AmmError> { crate::amm::initialize_amm(env, admin, default_slippage, max_slippage, auto_swap_threshold) }
    pub fn set_amm_pool(env: Env, admin: Address, protocol_config: AmmProtocolConfig) -> Result<(), AmmError> { crate::amm::set_amm_pool(env, admin, protocol_config) }
    pub fn amm_swap(env: Env, user: Address, params: SwapParams) -> Result<i128, AmmError> { crate::amm::amm_swap(env, user, params) }
    pub fn amm_add_liquidity(env: Env, user: Address, params: LiquidityParams) -> Result<i128, AmmError> { crate::amm::amm_add_liquidity(env, user, params) }
    pub fn amm_remove_liquidity(
        env: Env,
        user: Address,
        protocol: Address,
        token_a: Option<Address>,
        token_b: Option<Address>,
        lp_tokens: i128,
        min_amount_a: i128,
        min_amount_b: i128,
        deadline: u64,
    ) -> Result<(i128, i128), AmmError> {
        crate::amm::amm_remove_liquidity(env, user, protocol, token_a, token_b, lp_tokens, min_amount_a, min_amount_b, deadline)
    }

    pub fn flash_loan(env: Env, receiver_id: Address, assets: Vec<Address>, amounts: Vec<i128>, params: Vec<Val>) -> Result<(), FlashLoanError> { crate::flash_loan::flash_loan(&env, receiver_id, assets, amounts, params) }
    pub fn execute_flash_loan(env: Env, receiver_id: Address, asset: Address, amount: i128, params: Vec<Val>) -> Result<(), FlashLoanError> {
        let assets = Vec::from_array(&env, [asset]);
        let amounts = Vec::from_array(&env, [amount]);
        crate::flash_loan::flash_loan(&env, receiver_id, assets, amounts, params)
    }
    pub fn repay_flash_loan(env: Env, caller: Address, asset: Address, amount: i128) -> Result<(), FlashLoanError> {
        crate::flash_loan::repay_flash_loan(&env, caller, asset, amount)
    }
    pub fn set_flash_loan_fee(env: Env, caller: Address, fee: i128) -> Result<(), FlashLoanError> {
        crate::flash_loan::set_flash_loan_fee(&env, caller, fee)
    }
    pub fn configure_flash_loan(env: Env, caller: Address, config: FlashLoanConfig) -> Result<(), FlashLoanError> {
        crate::flash_loan::configure_flash_loan(&env, caller, config)
    }

    // --- Bridge Operations ---
    pub fn register_bridge(env: Env, caller: Address, network_id: u32, bridge: Address, fee_bps: i128) -> Result<(), BridgeError> { crate::bridge::register_bridge(&env, caller, network_id, bridge, fee_bps) }
    pub fn set_bridge_fee(env: Env, caller: Address, network_id: u32, fee_bps: i128) -> Result<(), BridgeError> { crate::bridge::set_bridge_fee(&env, caller, network_id, fee_bps) }
    pub fn bridge_deposit(env: Env, user: Address, network_id: u32, asset: Option<Address>, amount: i128) -> Result<i128, BridgeError> { crate::bridge::bridge_deposit(&env, user, network_id, asset, amount) }
    pub fn bridge_withdraw(env: Env, user: Address, network_id: u32, asset: Option<Address>, amount: i128) -> Result<i128, BridgeError> { crate::bridge::bridge_withdraw(&env, user, network_id, asset, amount) }
    pub fn list_bridges(env: Env) -> Map<u32, BridgeConfig> { crate::bridge::list_bridges(&env) }
    pub fn get_bridge_config(env: Env, network_id: u32) -> Result<BridgeConfig, BridgeError> { crate::bridge::get_bridge_config(&env, network_id) }

    // --- Standalone Configuration ---
    pub fn config_set(env: Env, caller: Address, key: Symbol, value: Val) -> Result<(), ConfigError> { crate::config::config_set(&env, caller, key, value) }
    pub fn config_get(env: Env, key: Symbol) -> Option<Val> { crate::config::config_get(&env, key) }
    pub fn config_backup(env: Env, caller: Address, keys: Vec<Symbol>) -> Result<Vec<(Symbol, Val)>, ConfigError> { crate::config::config_backup(&env, caller, keys) }
    pub fn config_restore(env: Env, caller: Address, backup: Vec<(Symbol, Val)>) -> Result<(), ConfigError> { crate::config::config_restore(&env, caller, backup) }

    // --- Reserve Management ---
    pub fn get_reserve_balance(env: Env, asset: Option<Address>) -> i128 {
        let reserve_key = DepositDataKey::ProtocolReserve(asset);
        env.storage().persistent().get::<DepositDataKey, i128>(&reserve_key).unwrap_or(0)
    }
    pub fn claim_reserves(env: Env, caller: Address, asset: Option<Address>, to: Address, amount: i128) -> Result<(), RiskManagementError> {
        require_admin(&env, &caller)?;
        let reserve_key = DepositDataKey::ProtocolReserve(asset.clone());
        let mut balance = env.storage().persistent().get::<DepositDataKey, i128>(&reserve_key).unwrap_or(0);
        if amount > balance { return Err(RiskManagementError::InvalidParameter); }
        // In real world, we would transfer tokens here if asset is Some
        balance -= amount;
        env.storage().persistent().set(&reserve_key, &balance);
        Ok(())
    }
}

// Re-enable tests in mod.rs or individually
#[cfg(test)]
mod tests;
