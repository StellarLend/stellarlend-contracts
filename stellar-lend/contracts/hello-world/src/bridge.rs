use soroban_sdk::{contracterror, contracttype, Address, Env, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BridgeError {
    Invalid = 1,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct BridgeConfig;

pub fn bridge_deposit(_env: &Env) {}
pub fn bridge_withdraw(_env: &Env) {}
pub fn get_bridge_config(_env: &Env) -> Option<BridgeConfig> { None }
pub fn list_bridges(env: &Env) -> Vec<BridgeConfig> { Vec::new(env) }
pub fn register_bridge(_env: &Env) {}
pub fn set_bridge_fee(_env: &Env) {}
