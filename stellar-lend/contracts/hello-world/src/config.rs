use soroban_sdk::{contracterror, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ConfigError {
    NotFound = 1,
}

pub fn config_backup(_env: &Env) {}
pub fn config_get(_env: &Env) {}
pub fn config_restore(_env: &Env) {}
pub fn config_set(_env: &Env) {}
