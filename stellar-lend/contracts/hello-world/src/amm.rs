//! AMM (Automated Market Maker) Registry and Swap Hooks Module
//!
//! This module provides AMM integration for the StellarLend protocol, including:
//! - Asset pair registration for supported AMMs
//! - Swap hooks for deleveraging and liquidation flows
//! - Event emissions for AMM usage tracking
//! - Integration with liquidation mechanisms

use crate::{Position, ProtocolError, ProtocolEvent, ReentrancyGuard, StateHelper};
use soroban_sdk::{contracterror, contracttype, Address, Env, Map, Symbol, Vec};

/// AMM-specific error types
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AMMError {
    /// AMM pair not registered
    PairNotRegistered = 7001,
    /// AMM pair already exists
    PairAlreadyExists = 7002,
    /// Invalid AMM address
    InvalidAMMAddress = 7003,
    /// Insufficient liquidity for swap
    InsufficientLiquidity = 7004,
    /// Slippage tolerance exceeded
    SlippageExceeded = 7005,
    /// Invalid swap parameters
    InvalidSwapParams = 7006,
    /// AMM operation not authorized
    Unauthorized = 7007,
    /// Swap failed
    SwapFailed = 7008,
}

impl From<AMMError> for ProtocolError {
    fn from(err: AMMError) -> Self {
        match err {
            AMMError::PairNotRegistered => ProtocolError::NotFound,
            AMMError::PairAlreadyExists => ProtocolError::AlreadyExists,
            AMMError::InvalidAMMAddress => ProtocolError::InvalidAddress,
            AMMError::InsufficientLiquidity => ProtocolError::InvalidAmount,
            AMMError::SlippageExceeded => ProtocolError::InvalidAmount,
            AMMError::InvalidSwapParams => ProtocolError::InvalidParameters,
            AMMError::Unauthorized => ProtocolError::Unauthorized,
            AMMError::SwapFailed => ProtocolError::InvalidAmount,
        }
    }
}

/// Asset pair information for AMM
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct AssetPair {
    /// First asset in the pair
    pub asset_a: Address,
    /// Second asset in the pair
    pub asset_b: Address,
    /// AMM contract address managing this pair
    pub amm_address: Address,
    /// Liquidity pool address (if different from AMM)
    pub pool_address: Option<Address>,
    /// Whether this pair is active
    pub is_active: bool,
    /// Registration timestamp
    pub registered_at: u64,
    /// Last updated timestamp
    pub last_updated: u64,
}

impl AssetPair {
    pub fn new(asset_a: Address, asset_b: Address, amm_address: Address, timestamp: u64) -> Self {
        Self {
            asset_a,
            asset_b,
            amm_address,
            pool_address: None,
            is_active: true,
            registered_at: timestamp,
            last_updated: timestamp,
        }
    }

    pub fn with_pool(
        asset_a: Address,
        asset_b: Address,
        amm_address: Address,
        pool_address: Address,
        timestamp: u64,
    ) -> Self {
        Self {
            asset_a,
            asset_b,
            amm_address,
            pool_address: Some(pool_address),
            is_active: true,
            registered_at: timestamp,
            last_updated: timestamp,
        }
    }
}

/// Swap parameters for AMM operations
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct SwapParams {
    /// User initiating the swap
    pub user: Address,
    /// Asset to swap from
    pub asset_in: Address,
    /// Asset to swap to
    pub asset_out: Address,
    /// Amount of asset_in to swap
    pub amount_in: i128,
    /// Minimum amount of asset_out expected
    pub min_amount_out: i128,
    /// Maximum slippage tolerance in basis points (e.g., 50 = 0.5%)
    pub max_slippage_bps: i128,
    /// Deadline timestamp for the swap
    pub deadline: u64,
}

impl SwapParams {
    pub fn new(
        user: Address,
        asset_in: Address,
        asset_out: Address,
        amount_in: i128,
        min_amount_out: i128,
    ) -> Self {
        Self {
            user,
            asset_in,
            asset_out,
            amount_in,
            min_amount_out,
            max_slippage_bps: 100, // Default 1% slippage
            deadline: 0,           // No deadline by default
        }
    }

    pub fn with_slippage(mut self, max_slippage_bps: i128) -> Self {
        self.max_slippage_bps = max_slippage_bps;
        self
    }

    pub fn with_deadline(mut self, deadline: u64) -> Self {
        self.deadline = deadline;
        self
    }
}

/// Swap result information
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct SwapResult {
    /// Amount of asset_in swapped
    pub amount_in: i128,
    /// Amount of asset_out received
    pub amount_out: i128,
    /// Effective exchange rate (scaled by 1e8)
    pub exchange_rate: i128,
    /// Slippage experienced in basis points
    pub slippage_bps: i128,
    /// Swap fee paid
    pub fee_paid: i128,
    /// Timestamp of the swap
    pub timestamp: u64,
}

impl SwapResult {
    pub fn new(amount_in: i128, amount_out: i128, fee_paid: i128, timestamp: u64) -> Self {
        // Calculate exchange rate (amount_out / amount_in * 1e8)
        let exchange_rate = if amount_in > 0 {
            (amount_out * 100_000_000) / amount_in
        } else {
            0
        };

        Self {
            amount_in,
            amount_out,
            exchange_rate,
            slippage_bps: 0, // Calculated separately
            fee_paid,
            timestamp,
        }
    }

    pub fn with_slippage(mut self, slippage_bps: i128) -> Self {
        self.slippage_bps = slippage_bps;
        self
    }
}

/// Pair key type for storage
#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct PairKey {
    pub asset_a: Address,
    pub asset_b: Address,
}

impl PairKey {
    pub fn new(asset_a: Address, asset_b: Address) -> Self {
        // Normalize order
        if asset_a < asset_b {
            Self { asset_a, asset_b }
        } else {
            Self {
                asset_a: asset_b,
                asset_b: asset_a,
            }
        }
    }
}

/// AMM registry storage management
pub struct AMMStorage;

impl AMMStorage {
    // Storage keys
    fn pairs_key(env: &Env) -> Symbol {
        Symbol::new(env, "amm_pairs")
    }

    fn pair_count_key(env: &Env) -> Symbol {
        Symbol::new(env, "amm_pair_count")
    }

    fn swap_history_key(env: &Env) -> Symbol {
        Symbol::new(env, "amm_swap_history")
    }

    /// Get all registered pairs
    pub fn get_all_pairs(env: &Env) -> Map<PairKey, AssetPair> {
        env.storage()
            .instance()
            .get(&Self::pairs_key(env))
            .unwrap_or_else(|| Map::new(env))
    }

    /// Save all pairs
    pub fn save_all_pairs(env: &Env, pairs: &Map<PairKey, AssetPair>) {
        env.storage().instance().set(&Self::pairs_key(env), pairs);
    }

    /// Get a specific pair
    pub fn get_pair(env: &Env, asset_a: &Address, asset_b: &Address) -> Option<AssetPair> {
        let pairs = Self::get_all_pairs(env);
        let key = PairKey::new(asset_a.clone(), asset_b.clone());
        pairs.get(key)
    }

    /// Save a specific pair
    pub fn save_pair(env: &Env, pair: &AssetPair) {
        let mut pairs = Self::get_all_pairs(env);
        let key = PairKey::new(pair.asset_a.clone(), pair.asset_b.clone());
        pairs.set(key, pair.clone());
        Self::save_all_pairs(env, &pairs);
    }

    /// Get pair count
    pub fn get_pair_count(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&Self::pair_count_key(env))
            .unwrap_or(0)
    }

    /// Increment pair count
    pub fn increment_pair_count(env: &Env) {
        let count = Self::get_pair_count(env);
        env.storage()
            .instance()
            .set(&Self::pair_count_key(env), &(count + 1));
    }

    /// Get swap history
    pub fn get_swap_history(env: &Env) -> Vec<SwapResult> {
        env.storage()
            .instance()
            .get(&Self::swap_history_key(env))
            .unwrap_or_else(|| Vec::new(env))
    }

    /// Add swap to history
    pub fn add_swap_to_history(env: &Env, swap: &SwapResult) {
        let mut history = Self::get_swap_history(env);
        history.push_back(swap.clone());

        // Keep only last 100 swaps to prevent storage bloat
        if history.len() > 100 {
            history = history.slice(history.len() - 100..);
        }

        env.storage()
            .instance()
            .set(&Self::swap_history_key(env), &history);
    }
}

/// AMM Registry and Swap Hooks Module
pub struct AMMRegistry;

impl AMMRegistry {
    /// Register a new AMM pair
    pub fn register_pair(
        env: &Env,
        asset_a: Address,
        asset_b: Address,
        amm_address: Address,
        pool_address: Option<Address>,
    ) -> Result<(), ProtocolError> {
        // Check if pair already exists
        if AMMStorage::get_pair(env, &asset_a, &asset_b).is_some() {
            return Err(AMMError::PairAlreadyExists.into());
        }

        let timestamp = env.ledger().timestamp();

        // Create the pair
        let pair = if let Some(pool) = pool_address {
            AssetPair::with_pool(
                asset_a.clone(),
                asset_b.clone(),
                amm_address.clone(),
                pool,
                timestamp,
            )
        } else {
            AssetPair::new(
                asset_a.clone(),
                asset_b.clone(),
                amm_address.clone(),
                timestamp,
            )
        };

        // Save the pair
        AMMStorage::save_pair(env, &pair);
        AMMStorage::increment_pair_count(env);

        // Emit registration event (only if we have a contract address)
        // In tests, env.current_contract_address() may not be available
        #[cfg(not(test))]
        {
            ProtocolEvent::AMMLiquidityAdded(
                env.current_contract_address(),
                asset_a,
                asset_b,
                0,
                0,
            )
            .emit(env);
        }

        Ok(())
    }

    /// Check if a pair exists and is active
    pub fn is_pair_registered(env: &Env, asset_a: &Address, asset_b: &Address) -> bool {
        if let Some(pair) = AMMStorage::get_pair(env, asset_a, asset_b) {
            pair.is_active
        } else {
            false
        }
    }

    /// Get pair information
    pub fn get_pair_info(
        env: &Env,
        asset_a: &Address,
        asset_b: &Address,
    ) -> Result<AssetPair, ProtocolError> {
        AMMStorage::get_pair(env, asset_a, asset_b)
            .ok_or_else(|| AMMError::PairNotRegistered.into())
    }

    /// Deactivate a pair
    pub fn deactivate_pair(
        env: &Env,
        asset_a: &Address,
        asset_b: &Address,
    ) -> Result<(), ProtocolError> {
        let mut pair = AMMStorage::get_pair(env, asset_a, asset_b)
            .ok_or_else(|| AMMError::PairNotRegistered)?;

        pair.is_active = false;
        pair.last_updated = env.ledger().timestamp();

        AMMStorage::save_pair(env, &pair);
        Ok(())
    }

    /// Reactivate a pair
    pub fn activate_pair(
        env: &Env,
        asset_a: &Address,
        asset_b: &Address,
    ) -> Result<(), ProtocolError> {
        let mut pair = AMMStorage::get_pair(env, asset_a, asset_b)
            .ok_or_else(|| AMMError::PairNotRegistered)?;

        pair.is_active = true;
        pair.last_updated = env.ledger().timestamp();

        AMMStorage::save_pair(env, &pair);
        Ok(())
    }

    /// Get total number of registered pairs
    pub fn get_total_pairs(env: &Env) -> i128 {
        AMMStorage::get_pair_count(env)
    }

    /// Execute a swap through registered AMM
    pub fn execute_swap(env: &Env, params: SwapParams) -> Result<SwapResult, ProtocolError> {
        ReentrancyGuard::enter(env)?;
        let result = (|| -> Result<SwapResult, ProtocolError> {
            // Validate parameters
            if params.amount_in <= 0 {
                return Err(AMMError::InvalidSwapParams.into());
            }

            if params.min_amount_out < 0 {
                return Err(AMMError::InvalidSwapParams.into());
            }

            // Check deadline
            if params.deadline > 0 && env.ledger().timestamp() > params.deadline {
                return Err(AMMError::SwapFailed.into());
            }

            // Get the pair
            let pair = AMMStorage::get_pair(env, &params.asset_in, &params.asset_out)
                .ok_or_else(|| AMMError::PairNotRegistered)?;

            if !pair.is_active {
                return Err(AMMError::PairNotRegistered.into());
            }

            // In a real implementation, this would call the actual AMM contract
            // For now, we simulate the swap result
            let fee_bps = 30; // 0.3% fee
            let fee = (params.amount_in * fee_bps) / 10000;
            let amount_after_fee = params.amount_in - fee;

            // Simulated exchange rate (1:1 for simplicity - in production would call AMM)
            let amount_out = amount_after_fee;

            // Check slippage
            if amount_out < params.min_amount_out {
                return Err(AMMError::SlippageExceeded.into());
            }

            let timestamp = env.ledger().timestamp();
            let swap_result = SwapResult::new(params.amount_in, amount_out, fee, timestamp);

            // Store swap in history
            AMMStorage::add_swap_to_history(env, &swap_result);

            // Emit swap event (only in non-test environment)
            #[cfg(not(test))]
            {
                ProtocolEvent::AMMSwap(
                    params.user,
                    params.asset_in,
                    params.asset_out,
                    params.amount_in,
                    amount_out,
                )
                .emit(env);
            }

            Ok(swap_result)
        })();

        ReentrancyGuard::exit(env);
        result
    }

    /// Swap hook for liquidation - swaps collateral to debt asset
    pub fn liquidation_swap_hook(
        env: &Env,
        liquidator: &Address,
        collateral_asset: &Address,
        debt_asset: &Address,
        collateral_amount: i128,
        min_debt_amount: i128,
    ) -> Result<SwapResult, ProtocolError> {
        // Create swap params
        let params = SwapParams::new(
            liquidator.clone(),
            collateral_asset.clone(),
            debt_asset.clone(),
            collateral_amount,
            min_debt_amount,
        )
        .with_slippage(200); // 2% slippage tolerance for liquidations

        // Execute the swap
        let swap_result = Self::execute_swap(env, params)?;

        // Update user position with swap results
        if let Some(mut position) = StateHelper::get_position(env, liquidator) {
            // Adjust collateral and debt based on swap
            position.collateral -= collateral_amount;
            position.debt -= swap_result.amount_out;
            StateHelper::save_position(env, &position);
        }

        Ok(swap_result)
    }

    /// Swap hook for deleveraging - reduces debt by swapping assets
    pub fn deleverage_swap_hook(
        env: &Env,
        user: &Address,
        asset_to_sell: &Address,
        debt_asset: &Address,
        sell_amount: i128,
        min_debt_repayment: i128,
    ) -> Result<SwapResult, ProtocolError> {
        // Create swap params
        let params = SwapParams::new(
            user.clone(),
            asset_to_sell.clone(),
            debt_asset.clone(),
            sell_amount,
            min_debt_repayment,
        )
        .with_slippage(150); // 1.5% slippage tolerance for deleveraging

        // Execute the swap
        let swap_result = Self::execute_swap(env, params)?;

        // Update user position
        if let Some(mut position) = StateHelper::get_position(env, user) {
            // Reduce debt by the amount received from swap
            position.debt -= swap_result.amount_out;
            StateHelper::save_position(env, &position);
        }

        Ok(swap_result)
    }

    /// Get swap history for analytics
    pub fn get_swap_history(env: &Env) -> Vec<SwapResult> {
        AMMStorage::get_swap_history(env)
    }

    /// Get all registered pairs
    pub fn get_all_pairs(env: &Env) -> Vec<AssetPair> {
        let pairs_map = AMMStorage::get_all_pairs(env);
        let mut pairs_vec = Vec::new(env);

        for (_, pair) in pairs_map.iter() {
            pairs_vec.push_back(pair);
        }

        pairs_vec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Contract;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    fn create_test_env() -> (Env, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(Contract, ());
        (env, contract_id)
    }

    #[test]
    fn test_register_amm_pair() {
        let (env, contract_id) = create_test_env();

        let asset_a = Address::generate(&env);
        let asset_b = Address::generate(&env);
        let amm_address = Address::generate(&env);

        env.as_contract(&contract_id, || {
            // Register pair
            let result = AMMRegistry::register_pair(
                &env,
                asset_a.clone(),
                asset_b.clone(),
                amm_address.clone(),
                None,
            );
            assert!(result.is_ok());

            // Verify pair is registered
            assert!(AMMRegistry::is_pair_registered(&env, &asset_a, &asset_b));

            // Verify pair count
            assert_eq!(AMMRegistry::get_total_pairs(&env), 1);
        });
    }

    #[test]
    fn test_register_duplicate_pair_fails() {
        let (env, contract_id) = create_test_env();

        let asset_a = Address::generate(&env);
        let asset_b = Address::generate(&env);
        let amm_address = Address::generate(&env);

        env.as_contract(&contract_id, || {
            // Register pair first time
            let result = AMMRegistry::register_pair(
                &env,
                asset_a.clone(),
                asset_b.clone(),
                amm_address.clone(),
                None,
            );
            assert!(result.is_ok());

            // Try to register again - should fail
            let result = AMMRegistry::register_pair(
                &env,
                asset_a.clone(),
                asset_b.clone(),
                amm_address.clone(),
                None,
            );
            assert!(result.is_err());
        });
    }

    #[test]
    fn test_pair_normalization() {
        let (env, contract_id) = create_test_env();

        let asset_a = Address::generate(&env);
        let asset_b = Address::generate(&env);
        let amm_address = Address::generate(&env);

        env.as_contract(&contract_id, || {
            // Register in one order
            let result = AMMRegistry::register_pair(
                &env,
                asset_a.clone(),
                asset_b.clone(),
                amm_address.clone(),
                None,
            );
            assert!(result.is_ok());

            // Check in reverse order - should find the same pair
            assert!(AMMRegistry::is_pair_registered(&env, &asset_b, &asset_a));

            // Get pair info in reverse order
            let pair_info = AMMRegistry::get_pair_info(&env, &asset_b, &asset_a);
            assert!(pair_info.is_ok());
        });
    }

    #[test]
    fn test_execute_swap() {
        let (env, contract_id) = create_test_env();

        let user = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);
        let amm_address = Address::generate(&env);

        env.as_contract(&contract_id, || {
            // Register pair
            AMMRegistry::register_pair(
                &env,
                asset_in.clone(),
                asset_out.clone(),
                amm_address,
                None,
            )
            .unwrap();

            // Create swap params
            let params = SwapParams::new(
                user.clone(),
                asset_in.clone(),
                asset_out.clone(),
                1_000_000,
                900_000,
            );

            // Execute swap
            let result = AMMRegistry::execute_swap(&env, params);
            assert!(result.is_ok());

            let swap_result = result.unwrap();
            assert_eq!(swap_result.amount_in, 1_000_000);
            assert!(swap_result.amount_out > 0);
            assert!(swap_result.fee_paid > 0);
        });
    }

    #[test]
    fn test_liquidation_swap_hook() {
        let (env, contract_id) = create_test_env();

        let liquidator = Address::generate(&env);
        let collateral_asset = Address::generate(&env);
        let debt_asset = Address::generate(&env);
        let amm_address = Address::generate(&env);

        env.as_contract(&contract_id, || {
            // Register pair
            AMMRegistry::register_pair(
                &env,
                collateral_asset.clone(),
                debt_asset.clone(),
                amm_address,
                None,
            )
            .unwrap();

            // Create a position for the liquidator
            let position = Position::new(liquidator.clone(), 2_000_000, 1_000_000);
            StateHelper::save_position(&env, &position);

            // Execute liquidation swap hook
            let result = AMMRegistry::liquidation_swap_hook(
                &env,
                &liquidator,
                &collateral_asset,
                &debt_asset,
                500_000,
                400_000,
            );

            assert!(result.is_ok());
            let swap_result = result.unwrap();
            assert_eq!(swap_result.amount_in, 500_000);
            assert!(swap_result.amount_out >= 400_000);

            // Verify position was updated
            let updated_position = StateHelper::get_position(&env, &liquidator).unwrap();
            assert_eq!(updated_position.collateral, 2_000_000 - 500_000);
            assert_eq!(updated_position.debt, 1_000_000 - swap_result.amount_out);
        });
    }

    #[test]
    fn test_swap_history_tracking() {
        let (env, contract_id) = create_test_env();

        let user = Address::generate(&env);
        let asset_in = Address::generate(&env);
        let asset_out = Address::generate(&env);
        let amm_address = Address::generate(&env);

        env.as_contract(&contract_id, || {
            // Register pair
            AMMRegistry::register_pair(
                &env,
                asset_in.clone(),
                asset_out.clone(),
                amm_address,
                None,
            )
            .unwrap();

            // Execute multiple swaps
            for _ in 0..3 {
                let params = SwapParams::new(
                    user.clone(),
                    asset_in.clone(),
                    asset_out.clone(),
                    1_000_000,
                    900_000,
                );

                let result = AMMRegistry::execute_swap(&env, params);
                assert!(result.is_ok());
            }

            // Get swap history
            let history = AMMRegistry::get_swap_history(&env);
            assert_eq!(history.len(), 3);
        });
    }
}
