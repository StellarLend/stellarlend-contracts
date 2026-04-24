#![allow(dead_code)]
use crate::prelude::*;
use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env, Map, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeConfig {
    /// The address of the bridge contract on Soroban
    pub bridge_address: Address,
    /// Fee in basis points for using the bridge (e.g. 100 = 1%)
    pub fee_bps: i128,
    /// Whether the bridge is currently active
    pub is_active: bool,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BridgeError {
    NotAuthorized = 1,
    BridgeAlreadyExists = 2,
    BridgeNotFound = 3,
    BridgeNotActive = 4,
    InvalidFee = 5,
    InvalidAmount = 6,
    AssetNotSupported = 7,
}

// Storage keys
const BRIDGES: Symbol = symbol_short!("bridges");

fn require_admin(env: &Env, caller: &Address) -> Result<(), BridgeError> {
    crate::admin::require_admin(env, caller).map_err(|_| BridgeError::NotAuthorized)
}

/// List all registered bridges
pub fn list_bridges(env: &Env) -> Map<u32, BridgeConfig> {
    env.storage()
        .persistent()
        .get(&BRIDGES)
        .unwrap_or_else(|| Map::new(env))
}

/// Retrieve the configuration of a specific bridge
pub fn get_bridge_config(env: &Env, network_id: u32) -> Result<BridgeConfig, BridgeError> {
    let bridges = list_bridges(env);
    bridges.get(network_id).ok_or(BridgeError::BridgeNotFound)
}

/// Register a new bridge connection
///
/// # Arguments
/// * `env` - The contract environment
/// * `_caller` - Admin address for authorization (auth is checked)
/// * `network_id` - ID of the remote network
/// * `bridge` - Address of the bridge contract
/// * `fee_bps` - Fee in basis points
#[allow(clippy::too_many_arguments)]
pub fn register_bridge(
    env: &Env,
    _caller: Address,
    network_id: u32,
    bridge: Address,
    fee_bps: i128,
) -> Result<(), BridgeError> {
    require_admin(env, &_caller)?;

    if !(0..=10000).contains(&fee_bps) {
        return Err(BridgeError::InvalidFee);
    }

    let mut bridges = list_bridges(env);
    if bridges.contains_key(network_id) {
        return Err(BridgeError::BridgeAlreadyExists);
    }

    let config = BridgeConfig {
        bridge_address: bridge,
        fee_bps,
        is_active: true,
    };

    bridges.set(network_id, config);
    env.storage().persistent().set(&BRIDGES, &bridges);

    Ok(())
}

/// Update the fee for an existing bridge
///
/// # Arguments
/// * `env` - The contract environment
/// * `_caller` - Admin address for authorization
/// * `network_id` - ID of the remote network
/// * `fee_bps` - New fee in basis points
pub fn set_bridge_fee(
    env: &Env,
    _caller: Address,
    network_id: u32,
    fee_bps: i128,
) -> Result<(), BridgeError> {
    require_admin(env, &_caller)?;

    if !(0..=10000).contains(&fee_bps) {
        return Err(BridgeError::InvalidFee);
    }

    let mut bridges = list_bridges(env);
    let mut config = bridges.get(network_id).ok_or(BridgeError::BridgeNotFound)?;

    config.fee_bps = fee_bps;
    bridges.set(network_id, config);

    env.storage().persistent().set(&BRIDGES, &bridges);
    Ok(())
}

/// Initiate deposit to bridge
///
/// Moves user assets into the lending protocol from a bridge.
///
/// # Arguments
/// * `env` - The contract environment
/// * `user` - User depositing collateral
/// * `network_id` - Remote network ID
/// * `asset` - Asset to deposit
/// * `amount` - Amount to deposit
pub fn bridge_deposit(
    env: &Env,
    user: Address,
    network_id: u32,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, BridgeError> {
    if amount <= 0 {
        return Err(BridgeError::InvalidAmount);
    }

    let config = get_bridge_config(env, network_id)?;
    if !config.is_active {
        return Err(BridgeError::BridgeNotActive);
    }

    // Ensure asset is configured in the protocol
    crate::cross_asset::get_asset_config_by_address(env, asset.clone())
        .map_err(|_| BridgeError::AssetNotSupported)?;

    // Calculate and deduct fee
    let fee = (amount * config.fee_bps) / 10000;
    let deposit_amount = amount - fee;

    // Simulate cross chain bridging by wrapping standard deposit
    crate::cross_asset::cross_asset_deposit(env, user.clone(), asset, deposit_amount)
        .map_err(|_| BridgeError::InvalidAmount)?;

    env.events().publish(
        (
            symbol_short!("bridge"),
            symbol_short!("deposit"),
            network_id,
        ),
        (user, deposit_amount, fee),
    );

    Ok(deposit_amount)
}

/// Initiate withdrawal through a bridge
///
/// Withdraws lending collateral and initiates a bridge transfer to remote chain.
///
/// # Arguments
/// * `env` - The contract environment
/// * `user` - User withdrawing collateral
/// * `network_id` - Remote network ID
/// * `asset` - Asset to withdraw
/// * `amount` - Amount to withdraw
pub fn bridge_withdraw(
    env: &Env,
    user: Address,
    network_id: u32,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, BridgeError> {
    if amount <= 0 {
        return Err(BridgeError::InvalidAmount);
    }

    let config = get_bridge_config(env, network_id)?;
    if !config.is_active {
        return Err(BridgeError::BridgeNotActive);
    }

    // Attempt internal withdrawal
    crate::cross_asset::cross_asset_withdraw(env, user.clone(), asset.clone(), amount)
        .map_err(|_| BridgeError::InvalidAmount)?;

    // Calculate and deduct fee for the withdrawal execution
    let fee = (amount * config.fee_bps) / 10000;
    let withdraw_amount = amount - fee;

    env.events().publish(
        (
            symbol_short!("bridge"),
            symbol_short!("withdraw"),
            network_id,
        ),
        (user, withdraw_amount, fee),
    );

    Ok(withdraw_amount)
}
