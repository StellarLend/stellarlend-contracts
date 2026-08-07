use soroban_sdk::{contracterror, Address, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LiquidationError {
    InvalidAmount = 1,
    Unauthorized = 2,
    PositionNotFound = 3,
    NotEligibleForLiquidation = 4,
}

pub fn liquidate(
    _env: &Env,
    _liquidator: Address,
    _borrower: Address,
    _debt_asset: Option<Address>,
    _collateral_asset: Option<Address>,
    _amount: i128,
) -> Result<(i128, i128, i128), LiquidationError> {
    Ok((0, 0, 0))
}
