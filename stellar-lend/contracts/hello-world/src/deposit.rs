use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug)]
pub enum DepositDataKey {
    Admin,
}

pub struct ProtocolAnalytics;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DepositError {
    Unauthorized,
}

pub fn set_native_asset_address(_env: &Env, _caller: Address, _native_asset: Address) -> Result<(), DepositError> {
    Ok(())
}

pub fn deposit_collateral(_env: &Env) -> i128 {
    0
}
