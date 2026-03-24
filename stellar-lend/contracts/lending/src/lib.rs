//! # Lending Contract Entry Point
//!
//! Registers and exposes the core functions of the StellarLend protocol.
//!
//! [Issue #321] Modularized liquidation into a separate crate/module.

mod borrow;
mod deposit;
mod flash_loan;
mod liquidate;
mod pause;
mod token_receiver;
mod withdraw;

use borrow::{borrow as borrow_impl, deposit as borrow_deposit};
use borrow::{
    get_admin as get_protocol_admin, get_liquidation_threshold_bps as get_liq_threshold_impl,
    get_oracle as get_oracle_impl, get_total_debt, get_user_collateral as get_borrow_collateral,
    get_user_debt as get_user_debt_impl, initialize_borrow_settings as init_borrow_settings_impl,
    repay as borrow_repay, set_admin as set_protocol_admin,
    set_liquidation_close_factor as set_close_factor_impl,
    set_liquidation_incentive as set_incentive_impl,
    set_liquidation_threshold_bps as set_liq_threshold_impl, set_oracle as set_oracle_impl,
    BorrowCollateral, BorrowError, DebtPosition,
};
use deposit::{
    deposit as deposit_impl, get_user_collateral as get_deposit_collateral_impl,
    initialize_deposit_settings as init_deposit_settings_impl, set_deposit_cap as set_cap_impl,
};
use flash_loan::{
    flash_loan as flash_loan_impl, get_flash_loan_fee_bps as get_flash_fee_impl,
    set_flash_loan_fee_bps as set_flash_fee_impl,
};
use liquidate::liquidate as liquidate_impl;
use pause::{is_paused, set_pause};
use soroban_sdk::{contract, contractimpl, Address, Env};
use withdraw::withdraw as withdraw_impl;

pub use borrow::BorrowDataKey;
pub use flash_loan::FlashLoanAction;

#[cfg(test)]
mod borrow_test;
#[cfg(test)]
mod data_store_test;
#[cfg(test)]
mod deposit_test;
#[cfg(test)]
mod emergency_shutdown_test;
#[cfg(test)]
mod flash_loan_test;
#[cfg(test)]
mod math_safety_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod token_receiver_test;
#[cfg(test)]
mod upgrade_migration_safety_test;
#[cfg(test)]
mod upgrade_test;
#[cfg(test)]
mod views;
#[cfg(test)]
mod views_test;
#[cfg(test)]
mod withdraw_test;

#[cfg(test)]
mod liquidate_test;
#[cfg(test)]
mod stress_test;

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    /// Initializes the protocol with administrative settings.
    pub fn initialize(env: Env, admin: Address, debt_ceiling: i128, min_borrow: i128) {
        set_protocol_admin(&env, &admin);
        init_borrow_settings_impl(&env, debt_ceiling, min_borrow).unwrap();
        init_deposit_settings_impl(&env, i128::MAX).unwrap();
    }

    // --- Borrow ---
    pub fn borrow(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
        collateral_asset: Address,
        collateral_amount: i128,
    ) -> Result<(), BorrowError> {
        borrow_impl(
            &env,
            user,
            asset,
            amount,
            collateral_asset,
            collateral_amount,
        )
    }

    pub fn repay(env: Env, user: Address, asset: Address, amount: i128) -> Result<(), BorrowError> {
        borrow_repay(&env, user, asset, amount)
    }

    // --- Deposit/Withdraw ---
    pub fn deposit(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        // We use the borrow-compatible deposit which tracks it as collateral
        borrow_deposit(&env, user, asset, amount)
    }

    pub fn withdraw(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
    ) -> Result<(), BorrowError> {
        withdraw_impl(&env, user, asset, amount)
    }

    // --- Liquidation ---
    pub fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        repay_amount: i128,
    ) -> Result<(), BorrowError> {
        liquidate_impl(
            &env,
            liquidator,
            borrower,
            debt_asset,
            collateral_asset,
            repay_amount,
        )
    }

    // --- Admin/Settings ---
    pub fn set_oracle(env: Env, admin: Address, oracle: Address) -> Result<(), BorrowError> {
        set_oracle_impl(&env, &admin, oracle)
    }

    pub fn set_liquidation_threshold_bps(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_liq_threshold_impl(&env, &admin, bps)
    }

    pub fn set_liquidation_close_factor(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_close_factor_impl(&env, &admin, bps)
    }

    pub fn set_liquidation_incentive(
        env: Env,
        admin: Address,
        bps: i128,
    ) -> Result<(), BorrowError> {
        set_incentive_impl(&env, &admin, bps)
    }

    pub fn set_deposit_cap(env: Env, admin: Address, cap: i128) -> Result<(), BorrowError> {
        set_cap_impl(&env, &admin, cap)
    }

    pub fn set_flash_loan_fee(env: Env, admin: Address, bps: i128) -> Result<(), BorrowError> {
        set_flash_fee_impl(&env, &admin, bps)
    }

    pub fn set_protocol_pause(
        env: Env,
        admin: Address,
        pause_type: pause::PauseType,
        paused: bool,
    ) -> Result<(), BorrowError> {
        let current_admin = get_protocol_admin(&env).ok_or(BorrowError::Unauthorized)?;
        if admin != current_admin {
            return Err(BorrowError::Unauthorized);
        }
        admin.require_auth();
        set_pause(&env, pause_type, paused);
        Ok(())
    }

    // --- Views ---
    pub fn get_health_factor(env: Env, user: Address) -> i128 {
        views::get_health_factor(&env, &user)
    }

    pub fn get_debt_balance(env: Env, user: Address) -> i128 {
        views::get_debt_balance(&env, &user)
    }

    pub fn get_collateral_balance(env: Env, user: Address) -> i128 {
        views::get_collateral_balance(&env, &user)
    }

    pub fn get_user_debt(env: Env, user: Address) -> DebtPosition {
        get_user_debt_impl(&env, &user)
    }

    pub fn get_user_collateral(env: Env, user: Address) -> BorrowCollateral {
        get_borrow_collateral(&env, &user)
    }

    pub fn get_total_protocol_debt(env: Env) -> i128 {
        get_total_debt(&env)
    }

    pub fn is_protocol_paused(env: Env, pause_type: pause::PauseType) -> bool {
        is_paused(&env, pause_type)
    }

    pub fn get_flash_loan_fee(env: Env) -> i128 {
        get_flash_fee_impl(&env)
    }
}
