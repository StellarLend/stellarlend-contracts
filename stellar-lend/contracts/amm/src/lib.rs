//! # StellarLend AMM Integration Contract
//!
//! Provides Automated Market Maker (AMM) integration for the lending protocol,
//! enabling token swaps, liquidity provision, and collateral optimization.
//!
//! ## Features
//! - Multi-protocol AMM support with pluggable protocol configs
//! - Slippage protection with configurable tolerances
//! - Auto-swap for collateral optimization during lending operations
//! - Callback validation with nonce-based replay protection
//! - Swap and liquidity operation history for analytics

#![no_std]
#![allow(clippy::too_many_arguments)]
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Map};

pub mod amm;
pub use crate::amm::{
    add_amm_protocol, add_liquidity, auto_swap_for_collateral, delete_amm_protocol, execute_swap,
    initialize_amm_settings, remove_liquidity, update_amm_settings, validate_amm_callback,
    AmmCallbackData, AmmError, AmmProtocolConfig, AmmSettings, LiquidityParams, SwapParams,
    TokenPair,
};

use stellarlend_common::upgrade;
pub struct DebugConfig { pub x: i128 }

#[contract]
pub struct AmmContract;

#[contract]
pub struct MockAmm;

#[contractimpl]
impl MockAmm {
    pub fn swap(
        _env: Env,
        _executor: Address,
        _token_in: Option<Address>,
        _token_out: Option<Address>,
        amount_in: i128,
        _min_amount_out: i128,
        _callback_data: AmmCallbackData,
    ) -> i128 {
        // Mock swap: 1% fee (99% return)
        amount_in * 99 / 100
    }
}

#[contractimpl]
impl AmmContract {
    // ───────────────────────────────────────────────────
    // Upgrade Management
    // ───────────────────────────────────────────────────

    pub fn upgrade_init(
        env: Env,
        admin: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) {
        upgrade::UpgradeManager::init(env, admin, current_wasm_hash, required_approvals);
    }

    pub fn upgrade_add_approver(env: Env, caller: Address, approver: Address) {
        upgrade::UpgradeManager::add_approver(env, caller, approver);
    }

    pub fn upgrade_remove_approver(env: Env, caller: Address, approver: Address) {
        upgrade::UpgradeManager::remove_approver(env, caller, approver);
    }

    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> u64 {
        upgrade::UpgradeManager::upgrade_propose(env, caller, new_wasm_hash, new_version)
    }

    pub fn upgrade_approve(env: Env, caller: Address, proposal_id: u64) -> u32 {
        upgrade::UpgradeManager::upgrade_approve(env, caller, proposal_id)
    }

    pub fn upgrade_execute(env: Env, caller: Address, proposal_id: u64) {
        upgrade::UpgradeManager::upgrade_execute(env, caller, proposal_id);
    }

    pub fn upgrade_rollback(env: Env, caller: Address, proposal_id: u64) {
        upgrade::UpgradeManager::upgrade_rollback(env, caller, proposal_id);
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
    /// Initialize AMM settings (admin only)
    ///
    /// Sets up AMM integration parameters including slippage tolerances and thresholds.
    ///
    /// # Arguments
    /// * `admin` - The admin address
    /// * `default_slippage` - Default slippage tolerance in basis points (e.g., 100 = 1%)
    /// * `max_slippage` - Maximum allowed slippage in basis points
    /// * `auto_swap_threshold` - Minimum amount for auto-swap operations
    ///
    /// # Returns
    /// Returns Ok(()) on success
    pub fn initialize_amm_settings(
        env: Env,
        admin: Address,
        default_slippage: i128,
        max_slippage: i128,
        auto_swap_threshold: i128,
    ) -> Result<(), AmmError> {
        initialize_amm_settings(
            &env,
            admin,
            default_slippage,
            max_slippage,
            auto_swap_threshold,
        )
    }

    /// Add AMM protocol (admin only)
    ///
    /// Registers a new AMM protocol for swap and liquidity operations.
    ///
    /// # Arguments
    /// * `admin` - The admin address
    /// * `protocol_config` - Configuration for the AMM protocol
    ///
    /// # Returns
    /// Returns Ok(()) on success
    pub fn add_amm_protocol(
        env: Env,
        admin: Address,
        protocol_config: AmmProtocolConfig,
    ) -> Result<(), AmmError> {
        add_amm_protocol(&env, admin, protocol_config)
    }

    /// Delete AMM protocol (admin only)
    ///
    /// Removes a registered AMM protocol.
    ///
    /// # Arguments
    /// * `admin` - The admin address
    /// * `protocol` - The protocol address to remove
    ///
    /// # Returns
    /// Returns Ok(()) on success
    pub fn delete_amm_protocol(
        env: Env,
        admin: Address,
        protocol: Address,
    ) -> Result<(), AmmError> {
        delete_amm_protocol(&env, admin, &protocol)
    }

    /// Update AMM settings (admin only)
    ///
    /// Updates AMM operation settings.
    ///
    /// # Arguments
    /// * `admin` - The admin address
    /// * `settings` - New AMM settings
    ///
    /// # Returns
    /// Returns Ok(()) on success
    pub fn update_amm_settings(
        env: Env,
        admin: Address,
        settings: AmmSettings,
    ) -> Result<(), AmmError> {
        update_amm_settings(&env, admin, settings)
    }

    /// Execute swap through AMM
    ///
    /// Performs token swaps using configured AMM protocols with slippage protection.
    /// Can be used within lending operations for collateral optimization.
    ///
    /// # Arguments
    /// * `user` - The user performing the swap
    /// * `params` - Swap parameters including tokens, amounts, and slippage tolerance
    ///
    /// # Returns
    /// Returns the actual amount received from the swap
    ///
    /// # Events
    /// Emits the following events:
    /// - `swap_executed`: Swap transaction details
    /// - `amm_operation`: AMM operation tracking
    pub fn execute_swap(env: Env, user: Address, params: SwapParams) -> Result<i128, AmmError> {
        execute_swap(&env, user, params)
    }

    /// Add liquidity to AMM pool
    ///
    /// Adds liquidity to AMM pools for earning fees and supporting protocol operations.
    ///
    /// # Arguments
    /// * `user` - The user adding liquidity
    /// * `params` - Liquidity parameters including tokens and amounts
    ///
    /// # Returns
    /// Returns the amount of LP tokens received
    ///
    /// # Errors
    /// Returns [`AmmError`] when authorization fails, parameters are invalid,
    /// liquidity operations are paused, deadline is expired, or min-amount
    /// protections are not satisfied after LP-share rounding.
    ///
    /// # Security
    /// Caller authorization is required. LP share minting uses floor rounding
    /// to preserve pool solvency; any unconsumed token remainder stays with the
    /// caller-side flow and is not credited as LP shares.
    ///
    /// # Events
    /// Emits the following events:
    /// - `liquidity_added`: Liquidity addition details
    /// - `amm_operation`: AMM operation tracking
    pub fn add_liquidity(
        env: Env,
        user: Address,
        params: LiquidityParams,
    ) -> Result<i128, AmmError> {
        add_liquidity(&env, user, params)
    }

    /// Remove liquidity from AMM pool
    ///
    /// Removes liquidity from AMM pools and returns underlying tokens.
    ///
    /// # Arguments
    /// * `user` - The user removing liquidity
    /// * `protocol` - AMM protocol address
    /// * `token_a` - First token address (None for native XLM)
    /// * `token_b` - Second token address (None for native XLM)
    /// * `lp_tokens` - Amount of LP tokens to burn
    /// * `min_amount_a` - Minimum amount of token A to receive
    /// * `min_amount_b` - Minimum amount of token B to receive
    /// * `deadline` - Operation deadline timestamp
    ///
    /// # Returns
    /// Returns tuple (amount_a, amount_b) received
    ///
    /// # Errors
    /// Returns [`AmmError`] when authorization fails, parameters are invalid,
    /// pool liquidity is insufficient, deadline is expired, or minimum output
    /// constraints are violated.
    ///
    /// # Security
    /// Caller authorization is required. Redemption math burns LP shares and
    /// uses floor rounding on token outputs to avoid over-withdrawal risk.
    ///
    /// # Events
    /// Emits the following events:
    /// - `liquidity_removed`: Liquidity removal details
    /// - `amm_operation`: AMM operation tracking
    #[allow(clippy::too_many_arguments)]
    pub fn remove_liquidity(
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
        remove_liquidity(
            &env,
            user,
            protocol,
            token_a,
            token_b,
            lp_tokens,
            min_amount_a,
            min_amount_b,
            deadline,
        )
    }

    /// Validate an AMM protocol callback (nonce + expiry + registered caller).
    ///
    /// The `caller` address must authorize this invocation via Soroban auth
    /// (`require_auth`). Internal router execution uses a separate path that
    /// does not require protocol auth; see the `stellarlend_amm` crate docs.
    ///
    /// # Arguments
    /// * `caller` — Registered AMM protocol contract address
    /// * `callback_data` — Nonce, user, deadline, and operation metadata
    ///
    /// # Errors
    /// * Auth failure if `caller` did not authorize this call
    /// * `AmmError::InvalidCallback` for disabled protocol, expired deadline, or bad nonce
    /// * `AmmError::Overflow` if the nonce counter would overflow
    ///
    /// # Events
    /// Emits `callback_validated` on success
    pub fn validate_amm_callback(
        env: Env,
        caller: Address,
        callback_data: AmmCallbackData,
    ) -> Result<(), AmmError> {
        validate_amm_callback(&env, caller, callback_data)
    }

    /// Auto-swap for collateral optimization
    ///
    /// Automatically swaps assets to optimize collateral ratios during lending operations.
    /// This is typically called internally during borrow/liquidation operations.
    ///
    /// # Arguments
    /// * `user` - The user whose collateral to optimize
    /// * `target_token` - The token to swap to (None for native XLM)
    /// * `amount` - Amount to swap
    ///
    /// # Returns
    /// Returns the amount received from the swap
    ///
    /// # Events
    /// Emits swap_executed and amm_operation events
    pub fn auto_swap_for_collateral(
        env: Env,
        user: Address,
        target_token: Option<Address>,
        amount: i128,
    ) -> Result<i128, AmmError> {
        auto_swap_for_collateral(&env, user, target_token, amount)
    }

    /// Get AMM settings
    ///
    /// Returns the current AMM configuration settings.
    ///
    /// # Returns
    /// Returns the current AMM settings
    pub fn get_amm_settings(env: Env) -> Option<AmmSettings> {
        amm::get_amm_settings(&env).ok()
    }

    /// Get supported AMM protocols
    ///
    /// Returns a list of all registered AMM protocols.
    ///
    /// # Returns
    /// Returns a map of protocol addresses to their configurations
    pub fn get_amm_protocols(env: Env) -> Option<Map<Address, AmmProtocolConfig>> {
        amm::get_amm_protocols(&env).ok()
    }

    /// Get swap history
    ///
    /// Returns recent swap operations for analytics.
    ///
    /// # Arguments
    /// * `user` - Optional user address to filter by
    /// * `limit` - Maximum number of records to return
    ///
    /// # Returns
    /// Returns a vector of swap records
    pub fn get_swap_history(
        env: Env,
        user: Option<Address>,
        limit: u32,
    ) -> Option<soroban_sdk::Vec<amm::SwapRecord>> {
        amm::get_swap_history(&env, user, limit).ok()
    }

    /// Get liquidity history
    ///
    /// Returns recent liquidity operations for analytics.
    ///
    /// # Arguments
    /// * `user` - Optional user address to filter by
    /// * `limit` - Maximum number of records to return
    ///
    /// # Returns
    /// Returns a vector of liquidity records
    pub fn get_liquidity_history(
        env: Env,
        user: Option<Address>,
        limit: u32,
    ) -> Option<soroban_sdk::Vec<amm::LiquidityRecord>> {
        amm::get_liquidity_history(&env, user, limit).ok()
    }
}

// Liquidation integration tests require lending crate; enable with feature "liquidate_integration"
// when lending is available as a dependency.
#[cfg(test)]
mod amm_coverage_booster;
#[cfg(all(test, feature = "liquidate_integration"))]
mod liquidate_test;
#[cfg(test)]
mod integration_test;
#[cfg(test)]
mod math_safety_test;
#[cfg(test)]
mod test;

//