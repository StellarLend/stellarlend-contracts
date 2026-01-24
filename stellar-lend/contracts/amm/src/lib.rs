#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, String};

mod amm;
use amm::{swap_tokens, add_liquidity, remove_liquidity, validate_callback};

mod types;
pub use types::*;

mod events;
use events::emit_lending_callback_event;

mod errors;
pub use errors::*;

#[contract]
pub struct AmmContract;

#[contractimpl]
impl AmmContract {
    /// Initialize the AMM contract
    pub fn initialize(env: Env, admin: Address) -> String {
        // Set admin
        let admin_key = AmmDataKey::Admin;
        env.storage().persistent().set(&admin_key, &admin);
        
        String::from_str(&env, "AMM initialized")
    }

    /// Swap tokens through AMM with hooks for lending operations
    ///
    /// Performs token swaps with integrated hooks for lending protocol operations.
    /// Supports multiple AMM protocols and validates callbacks.
    ///
    /// # Arguments
    /// * `user` - The address initiating the swap
    /// * `token_in` - Input token address
    /// * `token_out` - Output token address
    /// * `amount_in` - Amount of input tokens
    /// * `min_amount_out` - Minimum acceptable output amount
    /// * `amm_protocol` - AMM protocol to use for swap
    /// * `callback_data` - Optional callback data for hooks
    ///
    /// # Returns
    /// Returns the actual amount of output tokens received
    pub fn swap_with_hooks(
        env: Env,
        user: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        amm_protocol: String,
        callback_data: Option<CallbackData>,
    ) -> i128 {
        swap_tokens(
            &env,
            user,
            token_in,
            token_out,
            amount_in,
            min_amount_out,
            amm_protocol,
            callback_data,
        )
        .unwrap_or_else(|e| panic!("Swap error: {:?}", e))
    }

    /// Add liquidity to AMM pool with lending integration
    ///
    /// Adds liquidity to AMM pools with hooks for lending operations.
    /// Supports automated liquidity management for lending protocols.
    ///
    /// # Arguments
    /// * `user` - The address providing liquidity
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `amount_a` - Amount of first token
    /// * `amount_b` - Amount of second token
    /// * `min_liquidity` - Minimum liquidity tokens to receive
    /// * `amm_protocol` - AMM protocol to use
    ///
    /// # Returns
    /// Returns the amount of liquidity tokens minted
    pub fn add_liquidity_with_hooks(
        env: Env,
        user: Address,
        token_a: Address,
        token_b: Address,
        amount_a: i128,
        amount_b: i128,
        min_liquidity: i128,
        amm_protocol: String,
    ) -> i128 {
        add_liquidity(
            &env,
            user,
            token_a,
            token_b,
            amount_a,
            amount_b,
            min_liquidity,
            amm_protocol,
        )
        .unwrap_or_else(|e| panic!("Add liquidity error: {:?}", e))
    }

    /// Remove liquidity from AMM pool
    ///
    /// Removes liquidity from AMM pools with integrated hooks.
    ///
    /// # Arguments
    /// * `user` - The address removing liquidity
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `liquidity_amount` - Amount of liquidity tokens to burn
    /// * `min_amount_a` - Minimum amount of token A to receive
    /// * `min_amount_b` - Minimum amount of token B to receive
    /// * `amm_protocol` - AMM protocol to use
    ///
    /// # Returns
    /// Returns tuple (amount_a, amount_b) received
    pub fn remove_liquidity_with_hooks(
        env: Env,
        user: Address,
        token_a: Address,
        token_b: Address,
        liquidity_amount: i128,
        min_amount_a: i128,
        min_amount_b: i128,
        amm_protocol: String,
    ) -> (i128, i128) {
        remove_liquidity(
            &env,
            user,
            token_a,
            token_b,
            liquidity_amount,
            min_amount_a,
            min_amount_b,
            amm_protocol,
        )
        .unwrap_or_else(|e| panic!("Remove liquidity error: {:?}", e))
    }

    /// Validate AMM callback
    ///
    /// Validates callbacks from AMM operations to ensure security.
    ///
    /// # Arguments
    /// * `caller` - Address of the callback caller
    /// * `callback_data` - Callback data to validate
    ///
    /// # Returns
    /// Returns true if callback is valid
    pub fn validate_amm_callback(
        env: Env,
        caller: Address,
        callback_data: CallbackData,
    ) -> bool {
        validate_callback(&env, caller, callback_data)
            .unwrap_or_else(|e| panic!("Callback validation error: {:?}", e))
    }

    /// Get supported AMM protocols
    pub fn get_supported_protocols(env: Env) -> soroban_sdk::Vec<String> {
        let mut protocols = soroban_sdk::Vec::new(&env);
        protocols.push_back(String::from_str(&env, "stellar_dex"));
        protocols.push_back(String::from_str(&env, "soroswap"));
        protocols.push_back(String::from_str(&env, "phoenix"));
        protocols
    }

    /// Get AMM pool info
    pub fn get_pool_info(
        env: Env,
        token_a: Address,
        token_b: Address,
        amm_protocol: String,
    ) -> PoolInfo {
        let pool_key = AmmDataKey::PoolInfo(token_a.clone(), token_b.clone(), amm_protocol);
        env.storage()
            .persistent()
            .get(&pool_key)
            .unwrap_or(PoolInfo {
                token_a,
                token_b,
                reserve_a: 0,
                reserve_b: 0,
                total_liquidity: 0,
                fee_rate: 300, // 0.3%
            })
    }

    /// Register lending protocol for callbacks
    /// This allows lending protocols to register for AMM operation callbacks
    pub fn register_lending_protocol(
        env: Env,
        admin: Address,
        lending_contract: Address,
        _callback_types: soroban_sdk::Vec<String>,
    ) -> bool {
        admin.require_auth();
        
        // Store lending protocol registration
        let key = AmmDataKey::ProtocolConfig(String::from_str(&env, "lending_integration"));
        let config = ProtocolConfig {
            name: String::from_str(&env, "lending_protocol"),
            contract_address: lending_contract,
            enabled: true,
            fee_rate: 0, // No additional fees for lending integration
            max_slippage: 1000, // 10% max slippage for lending operations
        };
        env.storage().persistent().set(&key, &config);
        
        true
    }

    /// Execute swap with lending callback
    /// This is the main integration point for lending protocols
    pub fn swap_with_lending_callback(
        env: Env,
        user: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        amm_protocol: String,
        lending_operation: String, // "repayment", "liquidation", "rebalance"
    ) -> i128 {
        // Execute the swap
        let amount_out = swap_tokens(
            &env,
            user.clone(),
            token_in.clone(),
            token_out.clone(),
            amount_in,
            min_amount_out,
            amm_protocol.clone(),
            None,
        ).unwrap_or_else(|e| panic!("Swap error: {:?}", e));

        // Trigger lending protocol callback if registered
        let lending_key = AmmDataKey::ProtocolConfig(String::from_str(&env, "lending_integration"));
        if let Some(config) = env.storage().persistent().get::<AmmDataKey, ProtocolConfig>(&lending_key) {
            if config.enabled {
                // This would call back to the lending protocol
                // For now, we emit an event that the lending protocol can listen to
                emit_lending_callback_event(
                    &env,
                    &user,
                    &token_in,
                    &token_out,
                    amount_in,
                    amount_out,
                    &lending_operation,
                );
            }
        }

        amount_out
    }
}

#[cfg(test)]
mod test;