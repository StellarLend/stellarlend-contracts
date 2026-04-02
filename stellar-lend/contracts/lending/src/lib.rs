#![no_std]
#![allow(deprecated)]
#![allow(clippy::absurd_extreme_comparisons)]
#![allow(unexpected_cfgs)]
use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env, Val, Vec};
mod borrow;
mod constants;
mod cross_asset;
mod deposit;
mod flash_loan;
mod liquidate;
mod oracle;
mod pause;
mod token_receiver;
mod withdraw;

use borrow::{
    borrow as borrow_impl, deposit as borrow_deposit, get_admin as get_protocol_admin,
    get_close_factor_bps as get_close_factor_impl,
    get_liquidation_incentive_bps as get_liquidation_incentive_bps_impl,
    get_user_collateral as get_borrow_collateral, get_user_debt as get_user_debt_impl,
    initialize_borrow_settings as init_borrow_settings_impl, repay as borrow_repay,
    set_admin as set_protocol_admin, set_close_factor_bps as set_close_factor_impl,
    set_liquidation_incentive_bps as set_liquidation_incentive_bps_impl,
    set_liquidation_threshold_bps as set_liq_threshold_impl, set_oracle as set_oracle_impl,
    BorrowCollateral, BorrowError, DebtPosition,
};
    set_admin as set_protocol_admin, set_close_factor_bps as set_close_factor_impl,
    set_liquidation_incentive_bps as set_liquidation_incentive_bps_impl,
    set_liquidation_threshold_bps as set_liq_threshold_impl, set_oracle as set_oracle_impl,
    BorrowCollateral, BorrowError, DebtPosition,
};
use cross_asset::{
    borrow_asset as cross_borrow_asset, deposit_collateral_asset as cross_deposit_collateral,
    get_cross_position_summary as cross_position_summary, initialize_admin as cross_init_admin,
    repay_asset as cross_repay_asset, set_asset_params as cross_set_asset_params,
    withdraw_asset as cross_withdraw_asset, AssetParams, CrossAssetError, PositionSummary,
};
use deposit::{
    deposit as deposit_impl, get_user_collateral as get_deposit_collateral_impl,
    initialize_deposit_settings as init_deposit_settings_impl, DepositCollateral, DepositError,
};
use flash_loan::{
    flash_loan as flash_loan_impl, set_flash_loan_fee_bps as set_flash_loan_fee_impl,
    FlashLoanError,
};
use oracle::{OracleConfig, OracleError};
use pause::{
    blocks_high_risk_ops, complete_recovery as complete_recovery_logic,
    get_emergency_state as get_emergency_state_logic, get_guardian as get_guardian_logic,
    get_pause_state as get_pause_state_logic, is_paused, is_recovery,
    set_guardian as set_guardian_logic, set_pause as set_pause_impl,
    start_recovery as start_recovery_logic, trigger_shutdown as trigger_shutdown_logic,
    EmergencyState, PauseType,
};
use token_receiver::receive as receive_impl;

mod views;
use views::{
    get_collateral_balance as view_collateral_balance,
    get_collateral_value as view_collateral_value, get_debt_balance as view_debt_balance,
    get_debt_value as view_debt_value, get_health_factor as view_health_factor,
    get_liquidation_incentive_amount as view_liquidation_incentive_amount,
    get_max_liquidatable_amount as view_max_liquidatable_amount,
    get_user_position as view_user_position, UserPositionSummary,
};

use withdraw::{
    initialize_withdraw_settings as initialize_withdraw_logic, withdraw as withdraw_logic,
    WithdrawError,
};

mod data_store;
use stellarlend_common::upgrade;
pub use stellarlend_common::upgrade::{UpgradeError, UpgradeStage, UpgradeStatus};

#[cfg(test)]
mod borrow_test;
#[cfg(test)]
mod cross_asset_test;
#[cfg(test)]
mod deposit_test;
#[cfg(test)]
mod emergency_shutdown_test;
#[cfg(test)]
mod flash_adversarial_test;
#[cfg(test)]
mod flash_loan_test;
#[cfg(test)]
mod pause_test;
#[cfg(test)]
mod token_receiver_test;
#[cfg(test)]
mod views_test;

#[cfg(test)]
mod constants_test;
#[cfg(test)]
mod data_store_test;
#[cfg(test)]
mod math_safety_test;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod upgrade_migration_safety_test;
#[cfg(test)]
mod upgrade_test;
#[cfg(test)]
mod withdraw_test;

#[cfg(test)]
mod bad_debt_test;
#[cfg(test)]
mod liquidation_boundary_test;
#[cfg(test)]
mod multi_user_contention_test;
#[cfg(test)]
mod oracle_test;
#[cfg(test)]
mod stress_test;

#[contract]
pub struct LendingContract;

#[contractimpl]
impl LendingContract {
    /// Initialize the protocol with admin and settings
    pub fn initialize(
        env: Env,
        admin: Address,
        debt_ceiling: i128,
        min_borrow_amount: i128,
    ) -> Result<(), BorrowError> {
        if get_protocol_admin(&env).is_some() {
            return Err(BorrowError::Unauthorized);
        }
        set_protocol_admin(&env, &admin);
        init_borrow_settings_impl(&env, debt_ceiling, min_borrow_amount)?;
        Ok(())
    }

    /// Borrow assets against deposited collateral
    pub fn borrow(
        env: Env,
        user: Address,
        asset: Address,
        amount: i128,
        collateral_asset: Address,
        collateral_amount: i128,
    ) -> Result<(), BorrowError> {
        if blocks_high_risk_ops(&env) {
            return Err(BorrowError::ProtocolPaused);
        }
        borrow_impl(
            &env,
            user,
            asset,
            amount,
            collateral_asset,
            collateral_amount,
        )
    }

    /// Set protocol pause state for a specific operation (admin only)
    pub fn set_pause(
        env: Env,
        admin: Address,
        pause_type: PauseType,
        paused: bool,
    ) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        set_pause_impl(&env, admin, pause_type, paused);
        Ok(())
    }

    /// Configure guardian address authorized to trigger emergency shutdown.
    pub fn set_guardian(env: Env, admin: Address, guardian: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        set_guardian_logic(&env, admin, guardian);
        Ok(())
    }

    /// Return current guardian address if configured.
    pub fn get_guardian(env: Env) -> Option<Address> {
        get_guardian_logic(&env)
    }

    /// Trigger emergency shutdown (admin or guardian).
    pub fn emergency_shutdown(env: Env, caller: Address) -> Result<(), BorrowError> {
        ensure_shutdown_authorized(&env, &caller)?;
        caller.require_auth();
        trigger_shutdown_logic(&env, caller);
        Ok(())
    }

    /// Move from hard shutdown into controlled user recovery.
    pub fn start_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        if get_emergency_state_logic(&env) != EmergencyState::Shutdown {
            return Err(BorrowError::ProtocolPaused);
        }
        start_recovery_logic(&env, admin);
        Ok(())
    }

    /// Return protocol to normal operation after recovery procedures.
    pub fn complete_recovery(env: Env, admin: Address) -> Result<(), BorrowError> {
        ensure_admin(&env, &admin)?;
        complete_recovery_logic(&env, admin);
        Ok(())
    }

    /// Read current emergency lifecycle state.
    pub fn get_emergency_state(env: Env) -> EmergencyState {
        get_emergency_state_logic(&env)
    }

// ...existing code...

fn ensure_admin(env: &Env, admin: &Address) -> Result<(), BorrowError> {
    let current_admin = get_protocol_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *admin != current_admin {
        return Err(BorrowError::Unauthorized);
    }
    admin.require_auth();
    Ok(())
}

fn ensure_shutdown_authorized(env: &Env, caller: &Address) -> Result<(), BorrowError> {
    let admin = get_protocol_admin(env).ok_or(BorrowError::Unauthorized)?;
    if *caller == admin {
        return Ok(());
    }

    let guardian = get_guardian_logic(env).ok_or(BorrowError::Unauthorized)?;
    if *caller != guardian {
        return Err(BorrowError::Unauthorized);
    }

    Ok(())
}
