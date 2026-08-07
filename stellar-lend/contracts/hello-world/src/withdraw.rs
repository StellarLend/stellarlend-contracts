use soroban_sdk::{Address, Env};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WithdrawError {
    InvalidAmount,
}

pub fn withdraw_collateral(_env: &Env, _user: Address, _asset: Option<Address>, _amount: i128) -> Result<i128, WithdrawError> {
    Ok(0)
}
