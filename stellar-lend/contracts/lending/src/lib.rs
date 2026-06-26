#![no_std]

pub mod rounding_strategy;

#[cfg(test)]
mod interest_drift_regression_test;

#[cfg(test)]
mod liquidate_perf_test;

use soroban_sdk::{contract, contractimpl, contracttype, contracterror, Address, Env, Symbol};
#[cfg(any(test, feature = "testutils"))]
extern crate std;

#[cfg(any(test, feature = "testutils"))]
std::thread_local! {
    pub static STORAGE_READ_COUNT: core::cell::Cell<u32> = core::cell::Cell::new(0);
}

#[cfg(any(test, feature = "testutils"))]
pub fn get_storage_read_count() -> u32 {
    STORAGE_READ_COUNT.with(|c| c.get())
}

#[cfg(any(test, feature = "testutils"))]
pub fn reset_storage_read_count() {
    STORAGE_READ_COUNT.with(|c| c.set(0));
}

fn read_persistent<K, V>(env: &Env, key: &K) -> Option<V>
where
    K: soroban_sdk::IntoVal<Env, soroban_sdk::Val> + core::fmt::Debug,
    V: soroban_sdk::TryFromVal<Env, soroban_sdk::Val>,
{
    #[cfg(any(test, feature = "testutils"))]
    {
        STORAGE_READ_COUNT.with(|c| c.set(c.get() + 1));
    }
    env.storage().persistent().get(key)
}

fn write_persistent<K, V>(env: &Env, key: &K, val: &V)
where
    K: soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
    V: soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
{
    env.storage().persistent().set(key, val);
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PositionSummary {
    pub collateral: i128,
    pub debt: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AssetParams {
    pub is_active: bool,
    pub collateral_factor: i128, // in BPS, e.g. 8000
    pub liquidation_bonus: i128, // in BPS, e.g. 1000
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidationResult {
    pub collateral_seized: i128,
    pub debt_repaid: i128,
    pub bad_debt: i128,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum LendingError {
    BelowMinimumBorrow = 1008,
    InvalidAmount = 1009,
    MarketNotFound = 1010,
    PositionSolvent = 1011,
}

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    /// Initialize the lending contract with an admin.
    pub fn initialize(env: Env, admin: Address) {
        env.storage().instance().set(&"admin", &admin);
    }

    /// Get the configured admin (or panic if uninitialized).
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&"admin").unwrap()
    }

    /// Set the minimum borrow amount (admin-only).
    pub fn set_min_borrow(env: Env, min_borrow: i128) {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        env.storage().instance().set(&Symbol::new(&env, "BorrowMinAmount"), &min_borrow);
    }

    /// Get the minimum borrow amount.
    pub fn get_min_borrow(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "BorrowMinAmount"))
            .unwrap_or(0)
    }

    /// Deposit collateral for a user.
    pub fn deposit(env: Env, user: Address, amount: i128) -> i128 {
        user.require_auth();
        let key = ("col", user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_balance = current + amount;
        env.storage().persistent().set(&key, &new_balance);
        new_balance
    }

    /// Withdraw collateral for a user.
    pub fn withdraw(env: Env, user: Address, amount: i128) -> i128 {
        user.require_auth();
        let key = ("col", user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_balance = current - amount;
        env.storage().persistent().set(&key, &new_balance);
        new_balance
    }

    /// Borrow against deposited collateral.
    pub fn borrow(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        user.require_auth();
        let min_borrow = Self::get_min_borrow(env.clone());
        if amount < min_borrow {
            return Err(LendingError::BelowMinimumBorrow);
        }
        let key = ("debt", user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_debt = current + amount;
        env.storage().persistent().set(&key, &new_debt);
        Ok(new_debt)
    }

    /// Repay debt.
    pub fn repay(env: Env, user: Address, amount: i128) -> i128 {
        user.require_auth();
        let key = ("debt", user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_debt = current - amount;
        env.storage().persistent().set(&key, &new_debt);
        new_debt
    }

    /// Get the user's current position summary.
    pub fn get_position(env: Env, user: Address) -> PositionSummary {
        let col: i128 = env
            .storage()
            .persistent()
            .get(&("col", user.clone()))
            .unwrap_or(0);
        let debt: i128 = env
            .storage()
            .persistent()
            .get(&("debt", user.clone()))
            .unwrap_or(0);
        PositionSummary {
            collateral: col,
            debt,
        }
    }

    /// Set asset parameters (admin-only).
    pub fn set_asset_params(
        env: Env,
        asset: Address,
        is_active: bool,
        collateral_factor: i128,
        liquidation_bonus: i128,
    ) {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        let params = AssetParams {
            is_active,
            collateral_factor,
            liquidation_bonus,
        };
        write_persistent(&env, &("params", asset), &params);
    }

    /// Get asset parameters.
    pub fn get_asset_params(env: Env, asset: Address) -> Option<AssetParams> {
        read_persistent(&env, &("params", asset))
    }

    /// Set asset price (admin-only).
    pub fn set_asset_price(env: Env, asset: Address, price: i128) {
        let admin = Self::get_admin(env.clone());
        admin.require_auth();
        write_persistent(&env, &("price", asset), &price);
    }

    /// Get asset price.
    pub fn get_asset_price(env: Env, asset: Address) -> i128 {
        read_persistent(&env, &("price", asset)).unwrap_or(0)
    }

    /// Deposit collateral for a specific asset.
    pub fn deposit_asset(env: Env, user: Address, asset: Address, amount: i128) -> i128 {
        user.require_auth();
        let key = ("col", user.clone(), asset.clone());
        let current: i128 = read_persistent(&env, &key).unwrap_or(0);
        let new_balance = current + amount;
        write_persistent(&env, &key, &new_balance);
        new_balance
    }

    /// Borrow a specific asset.
    pub fn borrow_asset(env: Env, user: Address, asset: Address, amount: i128) -> Result<i128, LendingError> {
        user.require_auth();
        let min_borrow = Self::get_min_borrow(env.clone());
        if amount < min_borrow {
            return Err(LendingError::BelowMinimumBorrow);
        }
        let key = ("debt", user.clone(), asset.clone());
        let current: i128 = read_persistent(&env, &key).unwrap_or(0);
        let new_debt = current + amount;
        write_persistent(&env, &key, &new_debt);
        Ok(new_debt)
    }

    /// See LIQUIDATION_MECHANICS.md for detailed liquidation arithmetic.
    /// Liquidate an undercollateralized position.
    ///
    /// This function is optimized to minimize storage reads. It loads all required
    /// parameters and balances in exactly 6 persistent storage reads.
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        amount: i128,
    ) -> Result<LiquidationResult, LendingError> {
        liquidator.require_auth();
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        // --- BATCHED STORAGE READS (Exactly 6 reads) ---
        // 1. Read debt asset parameters
        let debt_params: AssetParams = read_persistent(&env, &("params", debt_asset.clone()))
            .ok_or(LendingError::MarketNotFound)?;
        // 2. Read collateral asset parameters
        let col_params: AssetParams = read_persistent(&env, &("params", collateral_asset.clone()))
            .ok_or(LendingError::MarketNotFound)?;
        // 3. Read borrower collateral balance
        let col_balance: i128 = read_persistent(&env, &("col", borrower.clone(), collateral_asset.clone()))
            .unwrap_or(0);
        // 4. Read borrower debt balance
        let debt_balance: i128 = read_persistent(&env, &("debt", borrower.clone(), debt_asset.clone()))
            .unwrap_or(0);
        // 5. Read debt asset price
        let debt_price: i128 = read_persistent(&env, &("price", debt_asset.clone()))
            .unwrap_or(0);
        // 6. Read collateral asset price
        let col_price: i128 = read_persistent(&env, &("price", collateral_asset.clone()))
            .unwrap_or(0);
        // -----------------------------------------------

        // Guard: active markets
        if !debt_params.is_active || !col_params.is_active {
            return Err(LendingError::MarketNotFound);
        }

        // Guard: borrower has debt
        if debt_balance <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        // Verify position is unhealthy (health factor < 1.0)
        let borrow_value = debt_balance
            .checked_mul(debt_price)
            .ok_or(LendingError::InvalidAmount)?;
        let col_value = col_balance
            .checked_mul(col_price)
            .ok_or(LendingError::InvalidAmount)?;
        let max_borrow = col_value
            .checked_mul(col_params.collateral_factor)
            .ok_or(LendingError::InvalidAmount)?
            / 10000;

        if borrow_value <= max_borrow {
            return Err(LendingError::PositionSolvent);
        }

        // Apply close factor (50%)
        let max_repay = debt_balance
            .checked_mul(5000)
            .ok_or(LendingError::InvalidAmount)?
            / 10000;
        let actual_repay = amount.min(max_repay);
        if actual_repay <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        // Compute collateral to seize (including liquidation bonus)
        let bonus_factor = 10000i128
            .checked_add(col_params.liquidation_bonus)
            .ok_or(LendingError::InvalidAmount)?;
        let seized_value = actual_repay
            .checked_mul(debt_price)
            .ok_or(LendingError::InvalidAmount)?
            .checked_mul(bonus_factor)
            .ok_or(LendingError::InvalidAmount)?
            / 10000;
        
        if col_price <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        let collateral_to_seize = seized_value / col_price;
        let actual_seized = collateral_to_seize.min(col_balance);

        // Check for shortfall (bad debt)
        let mut bad_debt = 0i128;
        if actual_seized < collateral_to_seize {
            let shortfall_usd = (collateral_to_seize - actual_seized)
                .checked_mul(col_price)
                .ok_or(LendingError::InvalidAmount)?;
            if debt_price > 0 {
                bad_debt = shortfall_usd / debt_price;
            }
        }

        // Update balances in storage
        let new_col = col_balance - actual_seized;
        let new_debt = debt_balance - actual_repay;

        write_persistent(&env, &("col", borrower.clone(), collateral_asset.clone()), &new_col);
        write_persistent(&env, &("debt", borrower.clone(), debt_asset.clone()), &new_debt);

        Ok(LiquidationResult {
            collateral_seized: actual_seized,
            debt_repaid: actual_repay,
            bad_debt,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, user)
    }

    #[test]
    fn test_initialize_and_get_admin() {
        let (_env, client, admin, _user) = setup();
        assert_eq!(client.get_admin(), admin);
    }

    #[test]
    fn test_deposit_increases_balance() {
        let (_env, client, _admin, user) = setup();
        let result = client.deposit(&user, &100);
        assert_eq!(result, 100);
        let again = client.deposit(&user, &50);
        assert_eq!(again, 150);
    }

    #[test]
    fn test_withdraw_decreases_balance() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &100);
        let result = client.withdraw(&user, &40);
        assert_eq!(result, 60);
    }

    #[test]
    fn test_borrow_increases_debt() {
        let (_env, client, _admin, user) = setup();
        let result = client.borrow(&user, &50);
        assert_eq!(result, 50);
    }

    #[test]
    fn test_repay_decreases_debt() {
        let (_env, client, _admin, user) = setup();
        client.borrow(&user, &100);
        let result = client.repay(&user, &30);
        assert_eq!(result, 70);
    }

    #[test]
    fn test_position_summary_reflects_state() {
        let (_env, client, _admin, user) = setup();
        client.deposit(&user, &200);
        client.borrow(&user, &75);
        let pos = client.get_position(&user);
        assert_eq!(pos.collateral, 200);
        assert_eq!(pos.debt, 75);
    }

    #[test]
    fn test_position_summary_default_zero() {
        let (_env, client, _admin, user) = setup();
        let pos = client.get_position(&user);
        assert_eq!(pos.collateral, 0);
        assert_eq!(pos.debt, 0);
    }

    #[test]
    fn test_borrow_below_minimum_rejected() {
        let (_env, client, _admin, user) = setup();
        client.set_min_borrow(&50);
        let res = client.try_borrow(&user, &40);
        assert!(res.is_err());
    }

    #[test]
    fn test_borrow_exactly_minimum_accepted() {
        let (_env, client, _admin, user) = setup();
        client.set_min_borrow(&50);
        let res = client.borrow(&user, &50);
        assert_eq!(res, 50);
    }

    #[test]
    fn test_set_min_borrow_admin_only() {
        let (_env, client, _admin, _user) = setup();
        assert_eq!(client.get_min_borrow(), 0);
        client.set_min_borrow(&100);
        assert_eq!(client.get_min_borrow(), 100);
    }
    #[test]
    fn test_liquidation_example1() {
        let (env, client, _admin, user) = setup();
        // asset addresses
        let coll_asset = Address::generate(&env);
        let debt_asset = Address::generate(&env);
        // set asset params and prices (collateral_factor 8000 bps, liquidation_bonus 1000 bps)
        client.set_asset_params(&coll_asset, true, 8000, 1000);
        client.set_asset_params(&debt_asset, true, 8000, 1000);
        client.set_asset_price(&coll_asset, 1);
        client.set_asset_price(&debt_asset, 1);
        // user deposits collateral and borrows
        client.deposit_asset(&user, &coll_asset, 1000);
        client.borrow_asset(&user, &debt_asset, 900);
        // perform liquidation: request 500 repay
        let res = client.liquidate(&user, &user, &debt_asset, &coll_asset, 500).unwrap();
        // Expected values from Example #1 (close factor caps at 450)
        assert_eq!(res.debt_repaid, 450);
        assert_eq!(res.collateral_seized, 495);
        assert_eq!(res.bad_debt, 0);
    }
}
