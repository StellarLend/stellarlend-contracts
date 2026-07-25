use soroban_sdk::{contracterror, contracttype, Address, Bytes, Env, IntoVal, Symbol, Val};

const BPS_DENOM: i128 = 10_000;

#[contracttype]
pub enum FlashLoanDataKey {
    Treasury(Option<Address>),
}
const DEFAULT_FLASH_FEE_BPS: i128 = 5;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashLoanConfig {
    pub fee_bps: i128,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FlashLoanError {
    Unauthorized = 1,
    InvalidFeeBps = 2,
    InsufficientLiquidity = 3,
    FlashLoanReentrancy = 4,
    InsufficientRepayment = 5,
    InvalidAmount = 6,
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), FlashLoanError> {
    let admin = crate::admin::get_admin(env).ok_or(FlashLoanError::Unauthorized)?;
    if caller != &admin {
        return Err(FlashLoanError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn get_flash_fee_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&Symbol::new(env, "FlashFeeBps"))
        .unwrap_or(DEFAULT_FLASH_FEE_BPS)
}

fn require_no_active_flash_loan(env: &Env) -> Result<(), FlashLoanError> {
    let active: bool = env
        .storage()
        .instance()
        .get(&Symbol::new(env, "FlashActive"))
        .unwrap_or(false);
    if active {
        return Err(FlashLoanError::FlashLoanReentrancy);
    }
    Ok(())
}

pub fn configure_flash_loan(
    env: &Env,
    caller: Address,
    config: FlashLoanConfig,
) -> Result<(), FlashLoanError> {
    require_admin(env, &caller)?;
    if config.fee_bps < 0 || config.fee_bps > 1000 {
        return Err(FlashLoanError::InvalidFeeBps);
    }
    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashFeeBps"), &config.fee_bps);
    Ok(())
}

pub fn set_flash_loan_fee(
    env: &Env,
    caller: Address,
    fee_bps: i128,
) -> Result<(), FlashLoanError> {
    require_admin(env, &caller)?;
    if fee_bps < 0 || fee_bps > 1000 {
        return Err(FlashLoanError::InvalidFeeBps);
    }
    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashFeeBps"), &fee_bps);
    Ok(())
}

pub fn execute_flash_loan(
    env: &Env,
    initiator: Address,
    receiver: Address,
    asset: Option<Address>,
    amount: i128,
    params: Bytes,
) -> Result<(), FlashLoanError> {
    if amount <= 0 {
        return Err(FlashLoanError::InvalidAmount);
    }

    require_no_active_flash_loan(env)?;

    let tre_key = FlashLoanDataKey::Treasury(asset.clone());
    let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
    if amount > tre_bal {
        return Err(FlashLoanError::InsufficientLiquidity);
    }

    initiator.require_auth();

    let fee_bps = get_flash_fee_bps(env);
    let fee = amount
        .checked_mul(fee_bps)
        .map(|v| v / BPS_DENOM)
        .expect("flash_loan: fee calculation overflow");

    let new_tre_bal = tre_bal
        .checked_sub(amount)
        .expect("flash_loan: treasury underflow during transfer");
    env.storage().persistent().set(&tre_key, &new_tre_bal);

    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashActive"), &true);

    let method = Symbol::new(env, "on_flash_loan");
    env.invoke_contract::<Val>(
        &receiver,
        &method,
        soroban_sdk::vec![
            env,
            initiator.into_val(env),
            asset.into_val(env),
            amount.into_val(env),
            fee.into_val(env),
            params.into_val(env)
        ],
    );

    env.storage()
        .instance()
        .set(&Symbol::new(env, "FlashActive"), &false);

    let final_tre: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
    let required_balance = tre_bal
        .checked_add(fee)
        .expect("flash_loan: fee addition overflow");
    if final_tre < required_balance {
        return Err(FlashLoanError::InsufficientRepayment);
    }

    Ok(())
}

pub fn repay_flash_loan(
    env: &Env,
    payer: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(), FlashLoanError> {
    if amount <= 0 {
        return Err(FlashLoanError::InvalidAmount);
    }

    payer.require_auth();

    let tre_key = FlashLoanDataKey::Treasury(asset.clone());
    let tre_bal: i128 = env.storage().persistent().get(&tre_key).unwrap_or(0);
    let new_tre_bal = tre_bal
        .checked_add(amount)
        .expect("repay_flash_loan: treasury balance overflow");
    env.storage().persistent().set(&tre_key, &new_tre_bal);

    Ok(())
}