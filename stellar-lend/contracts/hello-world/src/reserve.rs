use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env};

use crate::admin::require_admin;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum reserve factor (50%).
pub const MAX_RESERVE_FACTOR_BPS: i128 = 5000;

/// Default reserve factor (10%).
pub const DEFAULT_RESERVE_FACTOR_BPS: i128 = 1000;

/// Basis points scale (100%).
pub const BASIS_POINTS_SCALE: i128 = 10000;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ReserveError {
    Unauthorized = 1,
    InvalidReserveFactor = 2,
    InsufficientReserve = 3,
    InvalidAsset = 4,
    InvalidTreasury = 5,
    InvalidAmount = 6,
    Overflow = 7,
    TreasuryNotSet = 8,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
pub enum ReserveDataKey {
    ReserveBalance(Option<Address>),
    ReserveFactor(Option<Address>),
    TreasuryAddress,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveInitializedEvent {
    pub asset: Option<Address>,
    pub reserve_factor_bps: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveFactorUpdatedEvent {
    pub caller: Address,
    pub asset: Option<Address>,
    pub new_factor_bps: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveAccruedEvent {
    pub asset: Option<Address>,
    pub amount: i128,
    pub new_balance: i128,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreasuryAddressSetEvent {
    pub caller: Address,
    pub treasury: Address,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveWithdrawnEvent {
    pub caller: Address,
    pub asset: Option<Address>,
    pub treasury: Address,
    pub amount: i128,
    pub new_balance: i128,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn get_reserve_balance_internal(env: &Env, asset: &Option<Address>) -> i128 {
    env.storage()
        .persistent()
        .get(&ReserveDataKey::ReserveBalance(asset.clone()))
        .unwrap_or(0)
}

fn get_reserve_factor_internal(env: &Env, asset: &Option<Address>) -> i128 {
    env.storage()
        .persistent()
        .get(&ReserveDataKey::ReserveFactor(asset.clone()))
        .unwrap_or(DEFAULT_RESERVE_FACTOR_BPS)
}

fn get_treasury_address_internal(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get(&ReserveDataKey::TreasuryAddress)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn initialize_reserve_config(
    env: &Env,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError> {
    if reserve_factor_bps < 0 || reserve_factor_bps > MAX_RESERVE_FACTOR_BPS {
        return Err(ReserveError::InvalidReserveFactor);
    }

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ReserveFactor(asset.clone()), &reserve_factor_bps);

    if !env
        .storage()
        .persistent()
        .has(&ReserveDataKey::ReserveBalance(asset.clone()))
    {
        env.storage()
            .persistent()
            .set(&ReserveDataKey::ReserveBalance(asset.clone()), &0i128);
    }

    ReserveInitializedEvent {
        asset,
        reserve_factor_bps,
    }
    .publish(env);

    Ok(())
}

pub fn set_reserve_factor(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    reserve_factor_bps: i128,
) -> Result<(), ReserveError> {
    require_admin(env, &caller).map_err(|_| ReserveError::Unauthorized)?;

    if reserve_factor_bps < 0 || reserve_factor_bps > MAX_RESERVE_FACTOR_BPS {
        return Err(ReserveError::InvalidReserveFactor);
    }

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ReserveFactor(asset.clone()), &reserve_factor_bps);

    ReserveFactorUpdatedEvent {
        caller,
        asset,
        new_factor_bps: reserve_factor_bps,
    }
    .publish(env);

    Ok(())
}

pub fn get_reserve_factor(env: &Env, asset: Option<Address>) -> i128 {
    get_reserve_factor_internal(env, &asset)
}

pub fn accrue_reserve(
    env: &Env,
    asset: Option<Address>,
    interest_amount: i128,
) -> Result<(i128, i128), ReserveError> {
    let factor = get_reserve_factor_internal(env, &asset);

    let reserve_amount = interest_amount
        .checked_mul(factor)
        .ok_or(ReserveError::Overflow)?
        .checked_div(BASIS_POINTS_SCALE)
        .ok_or(ReserveError::Overflow)?;

    let lender_amount = interest_amount
        .checked_sub(reserve_amount)
        .ok_or(ReserveError::Overflow)?;

    if reserve_amount > 0 {
        let current = get_reserve_balance_internal(env, &asset);
        let new_balance = current
            .checked_add(reserve_amount)
            .ok_or(ReserveError::Overflow)?;
        env.storage()
            .persistent()
            .set(&ReserveDataKey::ReserveBalance(asset.clone()), &new_balance);

        ReserveAccruedEvent {
            asset,
            amount: reserve_amount,
            new_balance,
        }
        .publish(env);
    }

    Ok((reserve_amount, lender_amount))
}

pub fn get_reserve_balance(env: &Env, asset: Option<Address>) -> i128 {
    get_reserve_balance_internal(env, &asset)
}

pub fn set_treasury_address(
    env: &Env,
    caller: Address,
    treasury: Address,
) -> Result<(), ReserveError> {
    require_admin(env, &caller).map_err(|_| ReserveError::Unauthorized)?;

    if treasury == env.current_contract_address() {
        return Err(ReserveError::InvalidTreasury);
    }

    env.storage()
        .instance()
        .set(&ReserveDataKey::TreasuryAddress, &treasury);

    TreasuryAddressSetEvent { caller, treasury }.publish(env);

    Ok(())
}

pub fn get_treasury_address(env: &Env) -> Option<Address> {
    get_treasury_address_internal(env)
}

pub fn withdraw_reserve_to_treasury(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, ReserveError> {
    require_admin(env, &caller).map_err(|_| ReserveError::Unauthorized)?;

    let treasury = get_treasury_address_internal(env).ok_or(ReserveError::TreasuryNotSet)?;

    if amount <= 0 {
        return Err(ReserveError::InvalidAmount);
    }

    let current = get_reserve_balance_internal(env, &asset);
    if amount > current {
        return Err(ReserveError::InsufficientReserve);
    }

    let new_balance = current
        .checked_sub(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ReserveBalance(asset.clone()), &new_balance);

    ReserveWithdrawnEvent {
        caller,
        asset: asset.clone(),
        treasury,
        amount,
        new_balance,
    }
    .publish(env);

    Ok(amount)
}

/// Debit the reserve balance for `asset` by `amount` (admin only).
///
/// This is an accounting-only operation used by the legacy
/// [`claim_reserves`](crate::HelloContract::claim_reserves) entrypoint.
/// Unlike [`withdraw_reserve_to_treasury`], it does **not** check for a
/// configured treasury address — the caller is responsible for transferring
/// tokens to the intended recipient.
pub fn claim_reserves(
    env: &Env,
    caller: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<i128, ReserveError> {
    require_admin(env, &caller).map_err(|_| ReserveError::Unauthorized)?;

    if amount <= 0 {
        return Err(ReserveError::InvalidAmount);
    }

    let current = get_reserve_balance_internal(env, &asset);
    if amount > current {
        return Err(ReserveError::InsufficientReserve);
    }

    let new_balance = current
        .checked_sub(amount)
        .ok_or(ReserveError::Overflow)?;

    env.storage()
        .persistent()
        .set(&ReserveDataKey::ReserveBalance(asset.clone()), &new_balance);

    Ok(amount)
}

pub fn get_reserve_stats(env: &Env, asset: Option<Address>) -> (i128, i128, Option<Address>) {
    let balance = get_reserve_balance_internal(env, &asset);
    let factor = get_reserve_factor_internal(env, &asset);
    let treasury = get_treasury_address_internal(env);
    (balance, factor, treasury)
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    struct ReserveTestContract;

    #[contractimpl]
    impl ReserveTestContract {
        pub fn initialize(env: Env, admin: Address) {
            crate::admin::set_admin(&env, admin, None).unwrap();
        }

        pub fn initialize_reserve_config(
            env: Env,
            asset: Option<Address>,
            reserve_factor_bps: i128,
        ) -> Result<(), ReserveError> {
            super::initialize_reserve_config(&env, asset, reserve_factor_bps)
        }

        pub fn set_reserve_factor(
            env: Env,
            caller: Address,
            asset: Option<Address>,
            reserve_factor_bps: i128,
        ) -> Result<(), ReserveError> {
            super::set_reserve_factor(&env, caller, asset, reserve_factor_bps)
        }

        pub fn get_reserve_factor(env: Env, asset: Option<Address>) -> i128 {
            super::get_reserve_factor(&env, asset)
        }

        pub fn accrue_reserve(
            env: Env,
            asset: Option<Address>,
            interest_amount: i128,
        ) -> Result<(i128, i128), ReserveError> {
            super::accrue_reserve(&env, asset, interest_amount)
        }

        pub fn get_reserve_balance(env: Env, asset: Option<Address>) -> i128 {
            super::get_reserve_balance(&env, asset)
        }

        pub fn set_treasury_address(
            env: Env,
            caller: Address,
            treasury: Address,
        ) -> Result<(), ReserveError> {
            super::set_treasury_address(&env, caller, treasury)
        }

        pub fn get_treasury_address(env: Env) -> Option<Address> {
            super::get_treasury_address(&env)
        }

        pub fn withdraw_reserve_to_treasury(
            env: Env,
            caller: Address,
            asset: Option<Address>,
            amount: i128,
        ) -> Result<i128, ReserveError> {
            super::withdraw_reserve_to_treasury(&env, caller, asset, amount)
        }

        pub fn get_reserve_stats(
            env: Env,
            asset: Option<Address>,
        ) -> (i128, i128, Option<Address>) {
            super::get_reserve_stats(&env, asset)
        }
    }

    fn setup() -> (Env, ReserveTestContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(ReserveTestContract, ());
        let client = ReserveTestContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, user)
    }

    fn assert_unauthorized<T: std::fmt::Debug>(
        result: Result<Result<T, ReserveError>, soroban_sdk::Err>,
    ) {
        assert_eq!(result, Err(Ok(ReserveError::Unauthorized)));
    }

    // ── Initialization Tests ─────────────────────────────────────────────

    #[test]
    fn test_initialize_reserve_config_success() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        let r = client.try_initialize_reserve_config(&Some(asset.clone()), &1000);
        assert!(r.is_ok());
        assert_eq!(client.get_reserve_factor(&Some(asset.clone())), 1000);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 0);
    }

    #[test]
    fn test_initialize_reserve_config_native_asset() {
        let (_env, client, _admin, _user) = setup();

        let r = client.try_initialize_reserve_config(&None, &1500);
        assert!(r.is_ok());
        assert_eq!(client.get_reserve_factor(&None), 1500);
        assert_eq!(client.get_reserve_balance(&None), 0);
    }

    #[test]
    fn test_initialize_reserve_config_invalid_factor_negative() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        let r = client.try_initialize_reserve_config(&Some(asset), &-1);
        assert_eq!(r, Err(Ok(ReserveError::InvalidReserveFactor)));
    }

    #[test]
    fn test_initialize_reserve_config_invalid_factor_too_high() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        let r = client.try_initialize_reserve_config(&Some(asset), &5001);
        assert_eq!(r, Err(Ok(ReserveError::InvalidReserveFactor)));
    }

    #[test]
    fn test_initialize_reserve_config_edge_zero() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        let r = client.try_initialize_reserve_config(&Some(asset.clone()), &0);
        assert!(r.is_ok());
        assert_eq!(client.get_reserve_factor(&Some(asset)), 0);
    }

    #[test]
    fn test_initialize_reserve_config_edge_max() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        let r = client.try_initialize_reserve_config(&Some(asset.clone()), &5000);
        assert!(r.is_ok());
        assert_eq!(client.get_reserve_factor(&Some(asset)), 5000);
    }

    #[test]
    fn test_initialize_reserve_config_event_emitted() {
        let (env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        let event_count_before = env.events().all().len();
        let r = client.try_initialize_reserve_config(&Some(asset), &1000);
        assert!(r.is_ok());
        let event_count_after = env.events().all().len();
        assert!(
            event_count_after > event_count_before,
            "expected events to be emitted"
        );
    }

    // ── Factor Management Tests ──────────────────────────────────────────

    #[test]
    fn test_set_reserve_factor_success() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        let r = client.try_set_reserve_factor(&admin, &Some(asset.clone()), &2000);
        assert!(r.is_ok());
        assert_eq!(client.get_reserve_factor(&Some(asset)), 2000);
    }

    #[test]
    fn test_set_reserve_factor_native_asset() {
        let (_env, client, admin, _user) = setup();

        client.initialize_reserve_config(&None, &1000);
        let r = client.try_set_reserve_factor(&admin, &None, &2500);
        assert!(r.is_ok());
        assert_eq!(client.get_reserve_factor(&None), 2500);
    }

    #[test]
    fn test_set_reserve_factor_unauthorized() {
        let (_env, client, _admin, user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        assert_unauthorized(client.try_set_reserve_factor(&user, &Some(asset), &2000));
    }

    #[test]
    fn test_set_reserve_factor_invalid_negative() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        let r = client.try_set_reserve_factor(&admin, &Some(asset), &-500);
        assert_eq!(r, Err(Ok(ReserveError::InvalidReserveFactor)));
    }

    #[test]
    fn test_set_reserve_factor_invalid_too_high() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        let r = client.try_set_reserve_factor(&admin, &Some(asset), &5500);
        assert_eq!(r, Err(Ok(ReserveError::InvalidReserveFactor)));
    }

    #[test]
    fn test_get_reserve_factor_default() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        assert_eq!(
            client.get_reserve_factor(&Some(asset)),
            DEFAULT_RESERVE_FACTOR_BPS
        );
    }

    #[test]
    fn test_get_reserve_factor_after_init() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &500);
        assert_eq!(client.get_reserve_factor(&Some(asset)), 500);
    }

    // ── Accrual Tests ────────────────────────────────────────────────────

    #[test]
    fn test_accrue_reserve_basic() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);

        let (reserve, lender) = client.accrue_reserve(&Some(asset.clone()), &1000).unwrap();
        assert_eq!(reserve, 100);
        assert_eq!(lender, 900);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 100);
    }

    #[test]
    fn test_accrue_reserve_zero_interest() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        let (reserve, lender) = client.accrue_reserve(&Some(asset.clone()), &0).unwrap();
        assert_eq!(reserve, 0);
        assert_eq!(lender, 0);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 0);
    }

    #[test]
    fn test_accrue_reserve_multiple_accruals() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);

        let (r1, _) = client.accrue_reserve(&Some(asset.clone()), &1000).unwrap();
        assert_eq!(r1, 100);
        assert_eq!(client.get_reserve_balance(&Some(asset.clone())), 100);

        let (r2, _) = client.accrue_reserve(&Some(asset.clone()), &2000).unwrap();
        assert_eq!(r2, 200);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 300);
    }

    #[test]
    fn test_accrue_reserve_max_factor() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &5000);

        let (reserve, lender) = client.accrue_reserve(&Some(asset.clone()), &1000).unwrap();
        assert_eq!(reserve, 500);
        assert_eq!(lender, 500);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 500);
    }

    #[test]
    fn test_accrue_reserve_zero_factor() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &0);

        let (reserve, lender) = client.accrue_reserve(&Some(asset.clone()), &1000).unwrap();
        assert_eq!(reserve, 0);
        assert_eq!(lender, 1000);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 0);
    }

    #[test]
    fn test_accrue_reserve_large_amount() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);

        let amount = 10_000_000_000i128;
        let (reserve, lender) = client.accrue_reserve(&Some(asset.clone()), &amount).unwrap();
        assert_eq!(reserve, 1_000_000_000);
        assert_eq!(lender, 9_000_000_000);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 1_000_000_000);
    }

    #[test]
    fn test_accrue_reserve_multiple_assets() {
        let (_env, client, _admin, _user) = setup();
        let asset1 = Address::generate(&_env);
        let asset2 = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset1.clone()), &1000);
        client.initialize_reserve_config(&Some(asset2.clone()), &2000);

        let (r1, _) = client.accrue_reserve(&Some(asset1.clone()), &1000).unwrap();
        assert_eq!(r1, 100);

        let (r2, _) = client.accrue_reserve(&Some(asset2.clone()), &1000).unwrap();
        assert_eq!(r2, 200);

        assert_eq!(client.get_reserve_balance(&Some(asset1)), 100);
        assert_eq!(client.get_reserve_balance(&Some(asset2)), 200);
    }

    // ── Treasury Management Tests ────────────────────────────────────────

    #[test]
    fn test_set_treasury_address_success() {
        let (_env, client, admin, _user) = setup();
        let treasury = Address::generate(&_env);

        let r = client.try_set_treasury_address(&admin, &treasury);
        assert!(r.is_ok());
        assert_eq!(client.get_treasury_address(), Some(treasury));
    }

    #[test]
    fn test_set_treasury_address_unauthorized() {
        let (_env, client, _admin, user) = setup();
        let treasury = Address::generate(&_env);

        assert_unauthorized(client.try_set_treasury_address(&user, &treasury));
    }

    #[test]
    fn test_set_treasury_address_self_contract() {
        let (env, client, admin, _user) = setup();
        let contract_addr = env.current_contract_address();

        let r = client.try_set_treasury_address(&admin, &contract_addr);
        assert_eq!(r, Err(Ok(ReserveError::InvalidTreasury)));
    }

    #[test]
    fn test_get_treasury_address_not_set() {
        let (_env, client, _admin, _user) = setup();

        assert_eq!(client.get_treasury_address(), None);
    }

    #[test]
    fn test_get_treasury_address_after_set() {
        let (_env, client, admin, _user) = setup();
        let treasury = Address::generate(&_env);

        client.set_treasury_address(&admin, &treasury);
        assert_eq!(client.get_treasury_address(), Some(treasury));
    }

    #[test]
    fn test_set_treasury_address_event_emitted() {
        let (env, client, admin, _user) = setup();
        let treasury = Address::generate(&_env);

        let event_count_before = env.events().all().len();
        let r = client.try_set_treasury_address(&admin, &treasury);
        assert!(r.is_ok());
        let event_count_after = env.events().all().len();
        assert!(event_count_after > event_count_before);
    }

    // ── Withdrawal Tests ─────────────────────────────────────────────────

    #[test]
    fn test_withdraw_reserve_to_treasury_success() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let withdrawn = client
            .try_withdraw_reserve_to_treasury(&admin, &Some(asset.clone()), &500)
            .unwrap();
        assert_eq!(withdrawn, 500);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 500);
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_unauthorized() {
        let (_env, client, _admin, user) = setup();
        let asset = Address::generate(&_env);

        assert_unauthorized(client.try_withdraw_reserve_to_treasury(
            &user,
            &Some(asset),
            &100,
        ));
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_treasury_not_set() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let r = client.try_withdraw_reserve_to_treasury(&admin, &Some(asset), &100);
        assert_eq!(r, Err(Ok(ReserveError::TreasuryNotSet)));
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_insufficient_reserve() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &1_000).unwrap();

        let r = client.try_withdraw_reserve_to_treasury(&admin, &Some(asset), &200);
        assert_eq!(r, Err(Ok(ReserveError::InsufficientReserve)));
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_zero_amount() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let r = client.try_withdraw_reserve_to_treasury(&admin, &Some(asset), &0);
        assert_eq!(r, Err(Ok(ReserveError::InvalidAmount)));
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_negative_amount() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let r = client.try_withdraw_reserve_to_treasury(&admin, &Some(asset), &-50);
        assert_eq!(r, Err(Ok(ReserveError::InvalidAmount)));
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_full_balance() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let withdrawn = client
            .try_withdraw_reserve_to_treasury(&admin, &Some(asset.clone()), &1000)
            .unwrap();
        assert_eq!(withdrawn, 1000);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 0);
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_native_asset() {
        let (_env, client, admin, _user) = setup();
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&None, &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&None, &10_000).unwrap();

        let withdrawn = client
            .try_withdraw_reserve_to_treasury(&admin, &None, &500)
            .unwrap();
        assert_eq!(withdrawn, 500);
    }

    #[test]
    fn test_withdraw_reserve_to_treasury_event_emitted() {
        let (env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let event_count_before = env.events().all().len();
        let r = client.try_withdraw_reserve_to_treasury(&admin, &Some(asset), &500);
        assert!(r.is_ok());
        let event_count_after = env.events().all().len();
        assert!(event_count_after > event_count_before);
    }

    // ── Statistics Tests ─────────────────────────────────────────────────

    #[test]
    fn test_get_reserve_stats() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let (balance, factor, treasury_addr) = client.get_reserve_stats(&Some(asset));
        assert_eq!(balance, 1000);
        assert_eq!(factor, 1000);
        assert_eq!(treasury_addr, Some(treasury));
    }

    #[test]
    fn test_get_reserve_stats_no_config() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        let (balance, factor, treasury) = client.get_reserve_stats(&Some(asset));
        assert_eq!(balance, 0);
        assert_eq!(factor, DEFAULT_RESERVE_FACTOR_BPS);
        assert_eq!(treasury, None);
    }

    // ── Integration Tests ────────────────────────────────────────────────

    #[test]
    fn test_complete_reserve_lifecycle() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &2000);
        assert_eq!(client.get_reserve_factor(&Some(asset.clone())), 2000);

        client.set_treasury_address(&admin, &treasury);
        assert_eq!(client.get_treasury_address(), Some(treasury.clone()));

        let (reserve, lender) = client.accrue_reserve(&Some(asset.clone()), &5_000).unwrap();
        assert_eq!(reserve, 1000);
        assert_eq!(lender, 4000);
        assert_eq!(client.get_reserve_balance(&Some(asset.clone())), 1000);

        let (r2, _) = client.accrue_reserve(&Some(asset.clone()), &5_000).unwrap();
        assert_eq!(r2, 1000);
        assert_eq!(client.get_reserve_balance(&Some(asset.clone())), 2000);

        client.set_reserve_factor(&admin, &Some(asset.clone()), &1000);
        assert_eq!(client.get_reserve_factor(&Some(asset.clone())), 1000);

        let withdrawn1 = client
            .try_withdraw_reserve_to_treasury(&admin, &Some(asset.clone()), &500)
            .unwrap();
        assert_eq!(withdrawn1, 500);
        assert_eq!(client.get_reserve_balance(&Some(asset.clone())), 1500);

        let withdrawn2 = client
            .try_withdraw_reserve_to_treasury(&admin, &Some(asset.clone()), &1500)
            .unwrap();
        assert_eq!(withdrawn2, 1500);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 0);
    }

    #[test]
    fn test_multi_asset_reserves() {
        let (_env, client, admin, _user) = setup();
        let asset1 = Address::generate(&_env);
        let asset2 = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset1.clone()), &1000);
        client.initialize_reserve_config(&Some(asset2.clone()), &2000);
        client.initialize_reserve_config(&None, &1500);
        client.set_treasury_address(&admin, &treasury);

        client.accrue_reserve(&Some(asset1.clone()), &10_000).unwrap();
        client.accrue_reserve(&Some(asset2.clone()), &10_000).unwrap();
        client.accrue_reserve(&None, &10_000).unwrap();

        assert_eq!(client.get_reserve_balance(&Some(asset1)), 1000);
        assert_eq!(client.get_reserve_balance(&Some(asset2)), 2000);
        assert_eq!(client.get_reserve_balance(&None), 1500);
    }

    #[test]
    fn test_reserve_withdraw_then_reaccrue() {
        let (_env, client, admin, _user) = setup();
        let asset = Address::generate(&_env);
        let treasury = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &1000);
        client.set_treasury_address(&admin, &treasury);
        client.accrue_reserve(&Some(asset.clone()), &10_000).unwrap();

        let withdrawn = client
            .try_withdraw_reserve_to_treasury(&admin, &Some(asset.clone()), &500)
            .unwrap();
        assert_eq!(withdrawn, 500);
        assert_eq!(client.get_reserve_balance(&Some(asset.clone())), 500);

        let (reserve, _) = client.accrue_reserve(&Some(asset.clone()), &5_000).unwrap();
        assert_eq!(reserve, 500);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 1000);
    }

    #[test]
    fn test_error_code_stability() {
        assert_eq!(ReserveError::Unauthorized as u32, 1);
        assert_eq!(ReserveError::InvalidReserveFactor as u32, 2);
        assert_eq!(ReserveError::InsufficientReserve as u32, 3);
        assert_eq!(ReserveError::InvalidAsset as u32, 4);
        assert_eq!(ReserveError::InvalidTreasury as u32, 5);
        assert_eq!(ReserveError::InvalidAmount as u32, 6);
        assert_eq!(ReserveError::Overflow as u32, 7);
        assert_eq!(ReserveError::TreasuryNotSet as u32, 8);
    }

    #[test]
    fn test_constant_stability() {
        assert_eq!(MAX_RESERVE_FACTOR_BPS, 5000);
        assert_eq!(DEFAULT_RESERVE_FACTOR_BPS, 1000);
        assert_eq!(BASIS_POINTS_SCALE, 10000);
    }

    #[test]
    fn test_accrue_reserve_rounding() {
        let (_env, client, _admin, _user) = setup();
        let asset = Address::generate(&_env);

        client.initialize_reserve_config(&Some(asset.clone()), &333);

        let (reserve, lender) = client.accrue_reserve(&Some(asset.clone()), &100).unwrap();
        assert_eq!(reserve, 3);
        assert_eq!(lender, 97);
        assert_eq!(client.get_reserve_balance(&Some(asset)), 3);
    }
}
