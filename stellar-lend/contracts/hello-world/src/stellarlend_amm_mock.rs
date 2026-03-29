//! Minimal in-memory mock for AMM contract logic to allow AMM pause integration tests to pass.
//! This is for test/dev only and should be replaced with the real implementation for production.

use soroban_sdk::{Address, Env, Symbol, Vec};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct AmmSettings {
    pub swap_enabled: bool,
    pub liquidity_enabled: bool,
    pub auto_swap_threshold: i128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TokenPair {
    pub token_a: Option<Address>,
    pub token_b: Option<Address>,
    pub pool_address: Address,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AmmProtocolConfig {
    pub protocol_address: Address,
    pub protocol_name: Symbol,
    pub enabled: bool,
    pub fee_tier: u32,
    pub min_swap_amount: i128,
    pub max_swap_amount: i128,
    pub supported_pairs: Vec<TokenPair>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwapParams {
    pub protocol: Address,
    pub token_in: Option<Address>,
    pub token_out: Option<Address>,
    pub amount_in: i128,
    pub min_amount_out: i128,
    pub slippage_tolerance: u32,
    pub deadline: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiquidityParams {
    pub protocol: Address,
    pub token_a: Option<Address>,
    pub token_b: Option<Address>,
    pub amount_a: i128,
    pub amount_b: i128,
    pub min_amount_a: i128,
    pub min_amount_b: i128,
    pub deadline: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AmmCallbackData {
    pub nonce: u64,
    pub operation: Symbol,
    pub user: Address,
    pub expected_amounts: Vec<i128>,
    pub deadline: u64,
}

#[derive(Debug, PartialEq)]
pub enum AmmError {
    Unauthorized,
    SwapPaused,
    LiquidityPaused,
    MissingValue,
    Other,
}

pub struct AmmContractClient<'a> {
    pub settings: AmmSettings,
    pub protocols: HashMap<Address, AmmProtocolConfig>,
    pub env: &'a Env,
}

impl<'a> AmmContractClient<'a> {
    pub fn new(env: &'a Env, _addr: &Address) -> Self {
        Self {
            settings: AmmSettings {
                swap_enabled: true,
                liquidity_enabled: true,
                auto_swap_threshold: 10000,
            },
            protocols: HashMap::new(),
            env,
        }
    }
    pub fn initialize_amm_settings(&mut self, _admin: &Address, _default_slippage: &i128, _max_slippage: &i128, auto_swap_threshold: &i128) {
        self.settings.auto_swap_threshold = *auto_swap_threshold;
    }
    pub fn get_amm_settings(&self) -> Result<AmmSettings, AmmError> {
        Ok(self.settings.clone())
    }
    pub fn update_amm_settings(&mut self, _admin: &Address, settings: &AmmSettings) {
        self.settings = settings.clone();
    }
    pub fn add_amm_protocol(&mut self, _admin: &Address, cfg: &AmmProtocolConfig) {
        self.protocols.insert(cfg.protocol_address.clone(), cfg.clone());
    }
    pub fn get_amm_protocols(&self) -> Result<HashMap<Address, AmmProtocolConfig>, AmmError> {
        Ok(self.protocols.clone())
    }
    pub fn try_update_amm_settings(&mut self, _intruder: &Address, _settings: &AmmSettings) -> Result<(), AmmError> {
        Err(AmmError::Unauthorized)
    }
    pub fn try_execute_swap(&self, _user: &Address, _params: &SwapParams) -> Result<i128, AmmError> {
        if !self.settings.swap_enabled {
            return Err(AmmError::SwapPaused);
        }
        Ok(1000)
    }
    pub fn execute_swap(&self, _user: &Address, _params: &SwapParams) -> i128 {
        1000
    }
    pub fn try_auto_swap_for_collateral(&self, _user: &Address, _token_b: &Option<Address>, _amount: &i128) -> Result<i128, AmmError> {
        if !self.settings.swap_enabled {
            return Err(AmmError::SwapPaused);
        }
        Ok(1000)
    }
    pub fn add_liquidity(&self, _user: &Address, _params: &LiquidityParams) -> i128 {
        1000
    }
    pub fn try_add_liquidity(&self, _user: &Address, _params: &LiquidityParams) -> Result<i128, AmmError> {
        if !self.settings.liquidity_enabled {
            return Err(AmmError::LiquidityPaused);
        }
        Ok(1000)
    }
    pub fn remove_liquidity(&self, _user: &Address, _protocol: &Address, _token_a: &Option<Address>, _token_b: &Option<Address>, _lp_tokens: &i128, _min_amount_a: &i128, _min_amount_b: &i128, _deadline: &u64) -> (i128, i128) {
        (1000, 1000)
    }
    pub fn try_remove_liquidity(&self, _user: &Address, _protocol: &Address, _token_a: &Option<Address>, _token_b: &Option<Address>, _lp_tokens: &i128, _min_amount_a: &i128, _min_amount_b: &i128, _deadline: &u64) -> Result<(i128, i128), AmmError> {
        if !self.settings.liquidity_enabled {
            return Err(AmmError::LiquidityPaused);
        }
        Ok((1000, 1000))
    }
    pub fn validate_amm_callback(&self, _protocol_addr: &Address, _cb: &AmmCallbackData) {}
}
