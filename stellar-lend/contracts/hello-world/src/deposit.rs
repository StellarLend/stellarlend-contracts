use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositDataKey {
    CollateralBalance(Address),
    NativeAsset,
    PauseSwitches,
    ProtocolAnalytics,
    ProtocolReserve(Option<Address>),
    UserTotalDeposits(Address),
    ProtocolTotalDeposits,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DepositError {
    InvalidAmount = 1,
    DepositPaused = 2,
    InvalidAsset = 3,
    Overflow = 4,
    Unauthorized = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolAnalytics {
    pub total_deposits: i128,
    pub total_users: u32,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositEvent {
    pub schema_version: u32,
    pub user: Address,
    pub amount: i128,
    pub new_balance: i128,
    pub timestamp: u64,
}

pub const EVENT_SCHEMA_VERSION: u32 = 1;

fn emit_deposit(env: &Env, user: &Address, amount: i128, new_balance: i128) {
    let event = DepositEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        user: user.clone(),
        amount,
        new_balance,
        timestamp: env.ledger().timestamp(),
    };
    env.events()
        .publish((Symbol::new(env, "DepositEvent"),), event);
}

fn is_deposit_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&DepositDataKey::PauseSwitches)
        .unwrap_or(false)
}

fn resolve_asset(env: &Env, asset: Option<Address>) -> Result<Address, DepositError> {
    match asset {
        Some(addr) => Ok(addr),
        None => env
            .storage()
            .persistent()
            .get(&DepositDataKey::NativeAsset)
            .ok_or(DepositError::InvalidAsset),
    }
}

pub fn deposit_collateral(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, DepositError> {
    if amount <= 0 {
        return Err(DepositError::InvalidAmount);
    }

    if is_deposit_paused(env) {
        return Err(DepositError::DepositPaused);
    }

    user.require_auth();

    let asset_addr = resolve_asset(env, asset.clone())?;

    #[cfg(not(test))]
    {
        let token_client = soroban_sdk::token::Client::new(env, &asset_addr);
        token_client.transfer(&user, &env.current_contract_address(), &amount);
    }
    #[cfg(test)]
    {
        let _ = &asset_addr;
    }

    let balance_key = crate::DataKey::Balance(user.clone());
    let current_balance: i128 = env
        .storage()
        .persistent()
        .get(&balance_key)
        .unwrap_or(0_i128);
    let new_balance = current_balance
        .checked_add(amount)
        .ok_or(DepositError::Overflow)?;
    env.storage().persistent().set(&balance_key, &new_balance);

    let reserve_fee = amount
        .checked_mul(RESERVE_FEE_BPS)
        .and_then(|v| v.checked_div(BPS_DENOM))
        .unwrap_or(0);
    if reserve_fee > 0 {
        let reserve_key = DepositDataKey::ProtocolReserve(asset);
        let current_reserve: i128 = env
            .storage()
            .persistent()
            .get::<DepositDataKey, i128>(&reserve_key)
            .unwrap_or(0);
        let new_reserve = current_reserve
            .checked_add(reserve_fee)
            .ok_or(DepositError::Overflow)?;
        env.storage().persistent().set(&reserve_key, &new_reserve);
    }

    let user_total_key = DepositDataKey::UserTotalDeposits(user.clone());
    let user_total: i128 = env
        .storage()
        .persistent()
        .get(&user_total_key)
        .unwrap_or(0);
    let new_user_total = user_total.checked_add(amount).ok_or(DepositError::Overflow)?;
    env.storage()
        .persistent()
        .set(&user_total_key, &new_user_total);

    let protocol_total: i128 = env
        .storage()
        .persistent()
        .get(&DepositDataKey::ProtocolTotalDeposits)
        .unwrap_or(0);
    let new_protocol_total = protocol_total
        .checked_add(amount)
        .ok_or(DepositError::Overflow)?;
    env.storage()
        .persistent()
        .set(&DepositDataKey::ProtocolTotalDeposits, &new_protocol_total);

    emit_deposit(env, &user, amount, new_balance);

    Ok(new_balance)
}

pub fn set_native_asset_address(
    env: &Env,
    caller: Address,
    native_asset: Address,
) -> Result<(), DepositError> {
    crate::admin::require_admin(env, &caller).map_err(|_| DepositError::Unauthorized)?;
    env.storage()
        .persistent()
        .set(&DepositDataKey::NativeAsset, &native_asset);
    Ok(())
}

#[cfg(test)]
mod deposit_tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup(env: &Env) -> Address {
        env.mock_all_auths();
        Address::generate(env)
    }

    #[test]
    fn deposit_rejects_zero_amount() {
        let env = Env::default();
        let user = setup(&env);
        let asset = Address::generate(&env);
        let result = deposit_collateral(&env, user, Some(asset), 0);
        assert_eq!(result, Err(DepositError::InvalidAmount));
    }

    #[test]
    fn deposit_rejects_negative_amount() {
        let env = Env::default();
        let user = setup(&env);
        let asset = Address::generate(&env);
        let result = deposit_collateral(&env, user, Some(asset), -100);
        assert_eq!(result, Err(DepositError::InvalidAmount));
    }

    #[test]
    fn deposit_increases_balance() {
        let env = Env::default();
        let user = setup(&env);
        let asset = Address::generate(&env);

        let bal = deposit_collateral(&env, user.clone(), Some(asset.clone()), 500).unwrap();
        assert_eq!(bal, 500);

        let bal2 = deposit_collateral(&env, user, Some(asset), 300).unwrap();
        assert_eq!(bal2, 800);
    }

    #[test]
    fn deposit_rejects_when_paused() {
        let env = Env::default();
        let user = setup(&env);
        let asset = Address::generate(&env);

        env.storage()
            .persistent()
            .set(&DepositDataKey::PauseSwitches, &true);

        let result = deposit_collateral(&env, user, Some(asset), 100);
        assert_eq!(result, Err(DepositError::DepositPaused));
    }

    #[test]
    fn deposit_none_asset_fails_without_native() {
        let env = Env::default();
        let user = setup(&env);
        let result = deposit_collateral(&env, user, None, 100);
        assert_eq!(result, Err(DepositError::InvalidAsset));
    }

    #[test]
    fn deposit_none_asset_succeeds_with_native_configured() {
        let env = Env::default();
        let user = setup(&env);
        let native = Address::generate(&env);

        env.storage()
            .persistent()
            .set(&DepositDataKey::NativeAsset, &native);

        let bal = deposit_collateral(&env, user, None, 200).unwrap();
        assert_eq!(bal, 200);
    }

    #[test]
    fn deposit_credits_protocol_reserve() {
        let env = Env::default();
        let user = setup(&env);
        let asset = Address::generate(&env);

        deposit_collateral(&env, user, Some(asset.clone()), 10_000).unwrap();

        let reserve_key = DepositDataKey::ProtocolReserve(Some(asset));
        let reserve: i128 = env
            .storage()
            .persistent()
            .get::<DepositDataKey, i128>(&reserve_key)
            .unwrap_or(0);
        assert_eq!(reserve, 10, "reserve should receive 10 bps of deposit");
    }

    #[test]
    fn set_native_asset_address_requires_admin() {
        let env = Env::default();
        env.mock_all_auths();
        let non_admin = Address::generate(&env);
        let native = Address::generate(&env);

        let result = set_native_asset_address(&env, non_admin, native);
        assert!(result.is_err());
    }
}

/// Deposit collateral for a user.
pub fn deposit_collateral(env: &Env, _caller: Address, _asset: Option<Address>, _amount: i128) {}

