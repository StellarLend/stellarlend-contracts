use soroban_sdk::{Address, Env};

pub fn borrow_asset(_env: &Env, _user: Address, _asset: Option<Address>, _amount: i128) -> Result<i128, crate::errors::GovernanceError> {
    Ok(0)
}
