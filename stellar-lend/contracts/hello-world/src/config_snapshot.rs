use soroban_sdk::{contracttype, Env};

#[contracttype]
#[derive(Clone, Debug)]
pub struct ConfigSnapshot;

pub fn get_config_snapshot(_env: &Env) -> ConfigSnapshot {
    ConfigSnapshot
}
