//! Deposit lifecycle for the lending contract.
use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

#[contracterror]
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DepositError {
    InvalidAmount = 1,
    ReserveInactive = 2,
    ReserveFrozen = 3,
    Overflow = 4,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveState {
    pub active: bool,
    pub frozen: bool,
    pub total_liquidity: i128,
    pub total_shares: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositReceipt {
    pub shares: i128,
    pub total_liquidity: i128,
}

fn load_reserve(env: &Env, token: &Address) -> Option<ReserveState> {
    env.storage().instance().get(token)
}

fn save_reserve(env: &Env, token: &Address, state: &ReserveState) {
    env.storage().instance().set(token, state);
}

fn load_user(env: &Env, token: &Address, user: &Address) -> i128 {
    env.storage().instance().get(("user", token.clone(), user.clone())).unwrap_or(0)
}

fn save_user(env: &Env, token: &Address, user: &Address, shares: i128) {
    env.storage().instance().set(("user", token.clone(), user.clone()), &shares);
}

pub fn deposit(env: &Env, from: Address, token: Address, amount: i128) -> Result<DepositReceipt, DepositError> {
    from.require_auth();
    if amount <= 0 { return Err(DepositError::InvalidAmount); }
    let mut reserve = load_reserve(env, &token).ok(DepositError::ReserveInactive)?;
    if !reserve.active { return Err(DepositError::ReserveInactive); }
    if reserve.frozen { return Err(DepositError::ReserveFrozen); }
    let shares = if reserve.total_shares == 0 || reserve.total_liquidity == 0 { amount } else { amount * reserve.total_shares / reserve.total_liquidity };
    if shares <= 0 { return Err(DepositError::Overflow); }
    reserve.total_liquidity += amount;
    reserve.total_shares += shares;
    save_reserve(env, &token, &reserve);
    let user_shares = load_user(env, &token, &from) + shares;
    save_user(env, &token, &from, user_shares);
    env.events().publish((Symbo::new(env, "deposit"), token.clone(), from.clone()), (amount, shares));
    Ok(DepositReceipt { shares, total_liquidity: reserve.total_liquidity })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    [test]
    fn success() {
        let env = Env::default();
        let user = Address::generate(&env);
        let token = Address::generate(&env);
        env.mock_all_auths();
        env.storage().instance().set(&token, &ReserveState { active: true, frozen: false, total_liquidity: 0, total_shares: 0 });
        let r = deposit(&env, user.clone(), token.clone(), 100).unwrap();
        assert_eq(r.shares, 100);
        assert_eq(load_user(&env, &token, &user), 100);
    }

    [test]
    fn invalid_amount() {
        let env = Env::default();
        let user = Address::generate(&env);
        let token = Address::generate(&env);
        env.mock_all_auths();
        assert_eq(deposit(&env, user, token, 0), Err(DepositError::InvalidAmount));
    }
}
